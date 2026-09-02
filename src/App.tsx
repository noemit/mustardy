import { useCallback, useEffect, useRef, useState } from "react";
import { Timeline } from "./components/Timeline";
import { Titlebar } from "./components/Titlebar";
import { VideoStage } from "./components/VideoStage";
import {
  exportProject,
  loadAudioEnvelope,
  loadSettings,
  loadVideoAt,
  logUi,
  native,
  pickOpen,
  quitApp,
  readText,
  reveal,
  savePath,
  saveSettings,
  visualChangeTimes,
  writeText,
} from "./lib/bridge";
import {
  editedDuration,
  editedToSource,
  formatTime,
  invertCuts,
  isInAcceptedCut,
  isSilenceTrim,
  manualCut,
  sanitizeChanges,
  silenceCuts,
} from "./lib/edits";
import { startCutPreview } from "./lib/cutPreview";
import {
  DEFAULT_SILENCE_DROP,
  DEFAULT_SILENCE_MIN,
  silencesFromEnvelope,
  type AudioEnvelope,
} from "./lib/silence";
import type { Change, ProjectFile, Settings, SilenceRange, VideoInfo } from "./types";

export function App() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const standbyRef = useRef<HTMLVideoElement>(null);
  const activeIdx = useRef(0);
  const previewCtl = useRef<{ destroy: () => void; preroll: () => void; mark: () => void } | null>(null);
  const lastHydrate = useRef<{ key: string; at: number } | null>(null);
  const blobUrl = useRef<string | null>(null);
  const saveTimer = useRef(0);

  function adoptBlobUrl(url: string | null) {
    const prev = blobUrl.current;
    blobUrl.current = url;
    if (prev && prev !== url) URL.revokeObjectURL(prev);
  }

  const [video, setVideo] = useState<VideoInfo | null>(null);
  const [silences, setSilences] = useState<SilenceRange[]>([]);
  const [envelope, setEnvelope] = useState<AudioEnvelope | null>(null);
  const [changes, setChanges] = useState<Change[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [current, setCurrent] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [previewCuts, setPreviewCuts] = useState(true);
  const [busy, setBusy] = useState(false);
  const [activity, setActivity] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);

  const changesRef = useRef(changes);
  const previewCutsRef = useRef(previewCuts);
  changesRef.current = changes;
  previewCutsRef.current = previewCuts;

  useEffect(() => {
    void loadSettings().then(setSettings);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = settings?.theme || "light";
  }, [settings]);

  function toggleTheme() {
    if (!settings) return;
    const next: Settings = { ...settings, theme: settings.theme === "dark" ? "light" : "dark" };
    setSettings(next);
    void saveSettings(next);
  }

  function activeVideo() {
    return activeIdx.current === 0 ? videoRef.current : standbyRef.current;
  }

  useEffect(() => {
    const a = videoRef.current;
    const b = standbyRef.current;
    if (!a || !video) return;
    activeIdx.current = 0;
    const onPlay = (e: Event) => {
      if (e.target === activeVideo()) setPlaying(true);
    };
    const onPause = (e: Event) => {
      if (e.target === activeVideo()) setPlaying(false);
    };
    const onTime = (e: Event) => {
      if (e.target === activeVideo()) setCurrent((e.target as HTMLVideoElement).currentTime);
    };
    a.addEventListener("play", onPlay);
    a.addEventListener("pause", onPause);
    a.addEventListener("timeupdate", onTime);
    b?.addEventListener("play", onPlay);
    b?.addEventListener("pause", onPause);
    b?.addEventListener("timeupdate", onTime);
    previewCtl.current?.destroy();
    previewCtl.current = startCutPreview({
      getA: () => videoRef.current,
      getB: () => standbyRef.current,
      getIdx: () => activeIdx.current,
      setIdx: (i) => {
        activeIdx.current = i;
      },
      getKeeps: () => invertCuts(video.duration, changesRef.current, previewCutsRef.current),
      getFps: () => video.fps || 30,
      enabled: () => previewCutsRef.current,
      onTime: setCurrent,
    });
    previewCtl.current.mark();
    previewCtl.current.preroll();
    return () => {
      a.removeEventListener("play", onPlay);
      a.removeEventListener("pause", onPause);
      a.removeEventListener("timeupdate", onTime);
      b?.removeEventListener("play", onPlay);
      b?.removeEventListener("pause", onPause);
      b?.removeEventListener("timeupdate", onTime);
      previewCtl.current?.destroy();
      previewCtl.current = null;
    };
  }, [video]);

  useEffect(() => {
    previewCtl.current?.preroll();
  }, [changes]);

  useEffect(() => {
    previewCtl.current?.mark();
    previewCtl.current?.preroll();
  }, [previewCuts]);

  const seek = useCallback((t: number) => {
    const el = activeVideo();
    if (!el) return;
    el.currentTime = t;
    setCurrent(t);
    previewCtl.current?.preroll();
  }, []);

  const toggle = useCallback(() => {
    const el = activeVideo();
    if (!el) return;
    if (el.paused) void el.play();
    else el.pause();
  }, []);

  async function loadFromPicker() {
    try {
      const picked = await pickOpen();
      if (!picked) return;
      if (picked.type === "project") {
        try {
          await loadProject(picked.path);
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          setNote(`Couldn't open project (${msg}).`);
          logUi(`open project failed: ${msg}`);
        }
        return;
      }
      await hydrate(picked.video);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNote(`Couldn't open video (${msg}).`);
      logUi(`open video failed: ${msg}`);
    }
  }

  async function loadFile(file: File) {
    try {
      const url = URL.createObjectURL(file);
      adoptBlobUrl(url);
      const probeEl = document.createElement("video");
      probeEl.preload = "metadata";
      probeEl.src = url;
      let loadOk = false;
      await Promise.race([
        new Promise<void>((resolve) => {
          probeEl.onloadedmetadata = () => {
            loadOk = true;
            resolve();
          };
          probeEl.onerror = () => resolve();
        }),
        new Promise<void>((resolve) => setTimeout(resolve, 5000)),
      ]);
      if (!loadOk && (!probeEl.duration || Number.isNaN(probeEl.duration))) {
        logUi(`drop probe fallback: duration missing for ${file.name} (using 0)`);
      }
      await hydrate({
        path: file.name,
        url,
        name: file.name,
        duration: probeEl.duration || 0,
        width: probeEl.videoWidth || 0,
        height: probeEl.videoHeight || 0,
        fps: 30,
        hasAudio: true,
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNote(`Couldn't open dropped file (${msg}).`);
      logUi(`drop failed: ${msg}`);
    }
  }

  function applySilenceCuts(ranges: SilenceRange[], duration: number) {
    const cuts = silenceCuts(ranges);
    setChanges((all) => {
      const kept = all.filter((c) => !isSilenceTrim(c));
      return sanitizeChanges([...kept, ...cuts], duration).sort((a, b) => a.start - b.start);
    });
  }

  async function scanAudio(next: VideoInfo) {
    setActivity("Scanning audio…");
    try {
      const env = await loadAudioEnvelope(next);
      setEnvelope(env);
      let ranges = env
        ? silencesFromEnvelope(
            env,
            settings?.silenceDrop ?? DEFAULT_SILENCE_DROP,
            settings?.silenceMin ?? DEFAULT_SILENCE_MIN
          )
        : [];
      if (env && ranges.length && native && next.path.startsWith("/")) {
        setActivity("Checking quiet stretches for visual changes…");
        try {
          const visual = await visualChangeTimes(next.path, ranges);
          ranges = ranges.map((r, i) => ({ ...r, visual: visual[i] || [] }));
        } catch (e) {
          logUi(`visual scan failed: ${e instanceof Error ? e.message : String(e)}`);
        } finally {
          setActivity("Scanning audio…");
        }
      }
      setSilences(ranges);
      if (env) applySilenceCuts(ranges, next.duration);
    } catch (e) {
      setEnvelope(null);
      setSilences([]);
      setNote(`Silence scan failed (${e instanceof Error ? e.message : String(e)}).`);
    } finally {
      setActivity(null);
    }
  }

  async function hydrate(next: VideoInfo) {
    const key = `${next.path}|${next.duration}`;
    const now = Date.now();
    if (lastHydrate.current && lastHydrate.current.key === key && now - lastHydrate.current.at < 2000) {
      return;
    }
    lastHydrate.current = { key, at: now };
    setVideo(next);
    setChanges([]);
    setSelectedId(null);
    setCurrent(0);
    setEnvelope(null);
    setSilences([]);
    setNote(null);
    logUi(`open ${next.name} (${next.duration.toFixed(1)}s, ${next.width}x${next.height}, audio=${next.hasAudio})`);
    await scanAudio(next);
  }

  function persistSettings(next: Settings) {
    setSettings(next);
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => void saveSettings(next), 400);
  }

  function retuneSilences(dropDb: number, minDur: number) {
    if (!envelope || !video || playing) return;
    const next = silencesFromEnvelope(envelope, dropDb, minDur);
    setSilences(next);
    applySilenceCuts(next, video.duration);
  }

  function setSilenceDrop(db: number) {
    if (!settings) return;
    persistSettings({ ...settings, silenceDrop: db });
    retuneSilences(db, settings.silenceMin);
  }

  function setSilenceMin(minDur: number) {
    if (!settings) return;
    persistSettings({ ...settings, silenceMin: minDur });
    retuneSilences(settings.silenceDrop ?? DEFAULT_SILENCE_DROP, minDur);
  }

  function setNormalize(on: boolean) {
    if (!settings) return;
    persistSettings({ ...settings, normalizeAudio: on });
  }

  function setNormalizeAmount(n: number) {
    if (!settings) return;
    persistSettings({ ...settings, normalizeAmount: n });
  }

  function upsertManualCut(id: string, start: number, end: number) {
    if (!video || playing) return;
    const next = { ...manualCut(start, end), id };
    setChanges((all) =>
      sanitizeChanges([...all.filter((c) => c.id !== id), next], video.duration).sort(
        (a, b) => a.start - b.start
      )
    );
    setSelectedId(id);
  }

  function adjustManualCut(id: string, start: number, end: number) {
    if (!video || playing) return;
    setChanges((all) =>
      sanitizeChanges(
        all.map((c) =>
          c.id === id
            ? {
                ...c,
                start,
                end,
                label:
                  c.origin === "manual" ? `Cut ${formatTime(start)}–${formatTime(end)}` : c.label,
              }
            : c
        ),
        video.duration
      ).sort((a, b) => a.start - b.start)
    );
    setSelectedId(id);
  }

  function deleteChange(id: string) {
    if (playing) return;
    setChanges((all) => all.filter((c) => c.id !== id));
    setSelectedId((sel) => (sel === id ? null : sel));
  }

  async function loadProject(path: string) {
    const raw = await readText(path);
    const proj = JSON.parse(raw) as ProjectFile;
    if (proj.version !== 1 || !proj.video?.path) throw new Error("not a Mustardy project");
    const next = await loadVideoAt(proj.video.path);
    if (!next) throw new Error(`video missing: ${proj.video.path}`);
    lastHydrate.current = { key: `${next.path}|${next.duration}`, at: Date.now() };
    adoptBlobUrl(null);
    setVideo(next);
    setEnvelope(null);
    setSilences(proj.silences || []);
    setChanges(
      sanitizeChanges(proj.changes || [], next.duration)
        .filter((c) => c.status !== "rejected")
        .map((c) => (c.status === "accepted" ? c : { ...c, status: "accepted" as const }))
    );
    setSelectedId(null);
    setCurrent(0);
    logUi(`open project ${path} → ${next.name}`);
    await scanAudio(next);
  }

  async function saveProject() {
    if (!video || !native) {
      setNote("Save needs the desktop app and a loaded video.");
      return;
    }
    const dest = await savePath({
      title: "Save project",
      defaultPath: video.name.replace(/\.[^.]+$/, "") + ".mustardy.json",
      filters: [{ name: "Mustardy project", extensions: ["json"] }],
    });
    if (!dest) return;
    const proj: ProjectFile = {
      version: 1,
      video: {
        path: video.path,
        name: video.name,
        duration: video.duration,
        width: video.width,
        height: video.height,
        fps: video.fps,
        hasAudio: video.hasAudio,
      },
      silences,
      transcript: { text: "", words: [], source: "none" },
      changes,
      tags: [],
    };
    await writeText(dest, JSON.stringify(proj, null, 2));
    logUi(`saved project ${dest}`);
    setNote(`Saved ${dest}`);
  }

  async function onExport() {
    if (!video || !native) {
      setNote("Export needs the desktop app.");
      return;
    }
    const accepted = changes.filter((c) => c.status === "accepted");
    const normalize = Boolean(settings?.normalizeAudio && video.hasAudio);
    if (!accepted.length && !normalize) {
      setNote("Drag a cut on the timeline, or turn on Normalize, before export.");
      return;
    }
    setBusy(true);
    setActivity("Export 0% — preparing…");
    try {
      const result = await exportProject(
        {
          input: video.path,
          duration: video.duration,
          changes,
          normalize,
          normalizeAmount: settings?.normalizeAmount ?? 0.7,
          suggestedName: video.name.replace(/\.[^.]+$/, "") + "-mustardy.mp4",
        },
        (p) => setActivity(`Export ${p.progress}% — ${p.label}`)
      );
      if (result.path) {
        setNote(`Exported to ${result.path}`);
        await reveal(result.path);
      }
    } catch (err) {
      setNote(`Export failed: ${err instanceof Error ? err.message : "unknown error"}`);
    } finally {
      setBusy(false);
      setActivity(null);
    }
  }

  function selectChange(id: string) {
    setSelectedId(id);
    const c = changes.find((x) => x.id === id);
    if (c) seek(c.start + 0.01);
  }

  // Tauri native file drops: HTML5 onDrop never fires for Finder drags,
  // so also listen to the webview's drag-drop event and open via probe.
  useEffect(() => {
    if (!native) return;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop" || !event.payload.paths.length) return;
          for (const p of event.payload.paths) {
            try {
              if (/\.json$/i.test(p)) {
                await loadProject(p);
              } else {
                const vid = await loadVideoAt(p);
                if (!vid) {
                  const msg = `unsupported file: ${p}`;
                  setNote(msg);
                  logUi(msg);
                  continue;
                }
                await hydrate(vid);
              }
              break; // only first file
            } catch (e) {
              const msg = e instanceof Error ? e.message : String(e);
              setNote(`Couldn't open dropped file (${msg}).`);
              logUi(`tauri drop failed ${p}: ${msg}`);
            }
          }
        })
      )
      .then((fn) => {
        unlisten = fn;
      })
      .catch((e) => logUi(`drop listen failed: ${e instanceof Error ? e.message : String(e)}`));
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      if (e.key === "q") {
        e.preventDefault();
        void quitApp();
      }
      if (e.key === "s") {
        e.preventDefault();
        void saveProject();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const keepDur = video ? editedDuration(video.duration, changes, true) : 0;

  return (
    <div className="app">
      <Titlebar
        theme={settings?.theme || "light"}
        onToggleTheme={toggleTheme}
        onOpen={loadFromPicker}
        onSave={() => void saveProject()}
        onExport={onExport}
        onQuit={() => void quitApp()}
        canSave={Boolean(video)}
        canExport={Boolean(video) && !busy}
      />
      <div className="workspace">
        <div className="stage-col">
          <VideoStage
            video={video}
            videoRef={videoRef}
            standbyRef={standbyRef}
            current={current}
            playing={playing}
            keepDur={keepDur}
            loader={activity}
            note={note}
            onToggle={toggle}
            onSeek={seek}
            previewCuts={previewCuts}
            onTogglePreview={() => setPreviewCuts((v) => !v)}
            onOpen={loadFromPicker}
            onDropFile={loadFile}
          />
          <Timeline
            video={video}
            current={current}
            changes={changes}
            silences={silences}
            selectedId={selectedId}
            envelope={envelope}
            dropDb={settings?.silenceDrop ?? DEFAULT_SILENCE_DROP}
            minDur={settings?.silenceMin ?? DEFAULT_SILENCE_MIN}
            onDrop={setSilenceDrop}
            onMinDur={setSilenceMin}
            playing={playing}
            normalize={Boolean(settings?.normalizeAudio)}
            normalizeAmount={settings?.normalizeAmount ?? 0.7}
            onNormalize={setNormalize}
            onNormalizeAmount={setNormalizeAmount}
            keepDur={keepDur}
            onSeek={(t) => {
              const src = isInAcceptedCut(t, changes) ? editedToSource(t, video?.duration || 0, changes) : t;
              seek(src);
            }}
            onSelect={selectChange}
            onManualCut={upsertManualCut}
            onAdjustManualCut={adjustManualCut}
            onDeleteChange={deleteChange}
          />
        </div>
      </div>
    </div>
  );
}
