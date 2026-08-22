import { useCallback, useEffect, useRef, useState } from "react";
import { ChangeRail } from "./components/ChangeRail";
import { Chat } from "./components/Chat";
import { PackageModal } from "./components/PackageModal";
import { SettingsModal } from "./components/SettingsModal";
import { Timeline } from "./components/Timeline";
import { Titlebar } from "./components/Titlebar";
import { VideoStage } from "./components/VideoStage";
import { EyesBar } from "./components/EyesBar";
import {
  detectSilence,
  exportProject,
  kimiKeySaved,
  listModels,
  loadSettings,
  loadVideoAt,
  logUi,
  native,
  pickOpen,
  quitApp,
  readText,
  reveal,
  saveKimiKey,
  savePath,
  saveSettings,
  visualChangeTimes,
  writeText,
} from "./lib/bridge";
import {
  editedToSource,
  formatTime,
  isInAcceptedCut,
  nextTagName,
  sanitizeChanges,
  skippableCuts,
  uid,
} from "./lib/edits";
import {
  getBrainStatus,
  onBrainStatus,
  type BrainStatus,
} from "./lib/gemma";
import { planEdits } from "./lib/plan";
import { grabFrames } from "./lib/frames";
import { keptTranscript, sampleKeepTimes, suggestPackage, type PackIdea } from "./lib/package";
import { describePlan } from "./lib/planner";
import {
  fillerCuts,
  transcriptToSrt,
  transcriptToTxt,
  transcribeWhisper,
  wordRangeCut,
} from "./lib/transcribe";
import { getEyesStatus, onEyesStatus, type EyesStatus } from "./lib/vision";
import type { Caption } from "./lib/vision";
import type {
  Change,
  ChatMessage,
  ProjectFile,
  Settings,
  SilenceRange,
  Tag,
  Transcript,
  VideoInfo,
} from "./types";

export function App() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const lastHydrate = useRef<{ key: string; at: number } | null>(null);
  const blobUrl = useRef<string | null>(null);

  /** Swap the tracked drag-drop blob URL, revoking the one it replaces so
   * repeated drops don't pin every old video in memory. */
  function adoptBlobUrl(url: string | null) {
    const prev = blobUrl.current;
    blobUrl.current = url;
    if (prev && prev !== url) URL.revokeObjectURL(prev);
  }
  const [video, setVideo] = useState<VideoInfo | null>(null);
  const [silences, setSilences] = useState<SilenceRange[]>([]);
  const [captions, setCaptions] = useState<Caption[]>([]);
  const [transcript, setTranscript] = useState<Transcript>({ text: "", words: [], source: "none" });
  const [changes, setChanges] = useState<Change[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([
    {
      id: "hello",
      role: "assistant",
      text: "please load a video.",
    },
  ]);
  const [activity, setActivity] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [current, setCurrent] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [previewCuts, setPreviewCuts] = useState(true);
  const [busy, setBusy] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [pack, setPack] = useState<{ ideas: PackIdea[]; frames: Array<{ t: number; dataUrl: string }> } | null>(null);
  const [eyes, setEyes] = useState<EyesStatus>(getEyesStatus());
  const [brain, setBrain] = useState<BrainStatus>(getBrainStatus());
  const [kimiSaved, setKimiSaved] = useState(false);

  useEffect(() => {
    void (async () => {
      const loaded = await loadSettings();
      // Migrate: older builds kept the Kimi key in webview localStorage.
      // Push it to the desktop-side secret store once, then scrub it here —
      // localStorage is readable by anything that ever runs in the page.
      if (loaded.kimiKey && native) {
        try {
          await saveKimiKey(loaded.kimiKey);
        } catch (e) {
          logUi(`kimi key migration failed: ${e instanceof Error ? e.message : String(e)}`);
        }
      }
      const cleaned = { ...loaded, kimiKey: "" };
      if (loaded.kimiKey) await saveSettings(cleaned);
      setSettings(cleaned);
      setKimiSaved(await kimiKeySaved().catch(() => false));
    })();
    const offEyes = onEyesStatus(setEyes);
    const offBrain = onBrainStatus(setBrain);
    return () => {
      offEyes();
      offBrain();
    };
  }, []);

  useEffect(() => {
    if (!settings) return;
    // Engines load on demand. Silence + visual scan run at open; whisper
    // only when a request needs the transcript.
    if (settings.provider === "ollama") {
      listModels({ provider: "ollama", settings }).then((r) => setModels(r.models));
    }
  }, [settings]);

  useEffect(() => {
    document.documentElement.dataset.theme = settings?.theme || "light";
  }, [settings]);

  function toggleTheme() {
    if (!settings) return;
    const next: Settings = { ...settings, theme: settings.theme === "dark" ? "light" : "dark" };
    setSettings(next);
    void saveSettings(next);
  }

  useEffect(() => {
    const el = videoRef.current;
    if (!el) return;
    const onTime = () => {
      const t = el.currentTime;
      if (isInAcceptedCut(t, changes, previewCuts)) {
        const next = skippableCuts(changes, previewCuts)
          .filter((c) => c.end > t)
          .sort((a, b) => a.start - b.start)[0];
        if (next && t >= next.start && t < next.end) {
          el.currentTime = next.end + 0.01;
          return;
        }
      }
      setCurrent(t);
    };
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    el.addEventListener("timeupdate", onTime);
    el.addEventListener("play", onPlay);
    el.addEventListener("pause", onPause);
    return () => {
      el.removeEventListener("timeupdate", onTime);
      el.removeEventListener("play", onPlay);
      el.removeEventListener("pause", onPause);
    };
  }, [changes, previewCuts, video]);

  const seek = useCallback((t: number) => {
    const el = videoRef.current;
    if (!el) return;
    el.currentTime = t;
    setCurrent(t);
  }, []);

  const toggle = useCallback(() => {
    const el = videoRef.current;
    if (!el) return;
    if (el.paused) void el.play();
    else el.pause();
  }, []);

  function addTag() {
    if (!video) return;
    const name = nextTagName(tags, current);
    const tag: Tag = { id: uid("tag"), name, t: current };
    setTags((all) => [...all, tag]);
    setMessages((m) => [
      ...m,
      {
        id: uid("m"),
        role: "assistant",
        icon: "check" as const,
        text: `Tagged ${name} at ${formatTime(current)} — refer to it in chat as @${name.toLowerCase()}.`,
      },
    ]);
  }

  function removeTag(id: string) {
    setTags((all) => all.filter((t) => t.id !== id));
  }

  async function loadFromPicker() {
    const picked = await pickOpen();
    if (!picked) return;
    if (picked.type === "project") {
      try {
        await loadProject(picked.path);
      } catch (e) {
        setMessages((m) => [
          ...m,
          {
            id: uid("m"),
            role: "assistant",
            text: `Couldn't open project (${e instanceof Error ? e.message : String(e)}).`,
          },
        ]);
      }
      return;
    }
    await hydrate(picked.video);
  }

  async function loadFile(file: File) {
    const url = URL.createObjectURL(file);
    adoptBlobUrl(url);
    const probeEl = document.createElement("video");
    probeEl.preload = "metadata";
    probeEl.src = url;
    // A stalled or corrupt file must not hang the drop handler forever.
    await Promise.race([
      new Promise<void>((resolve) => {
        probeEl.onloadedmetadata = () => resolve();
        probeEl.onerror = () => resolve();
      }),
      new Promise<void>((resolve) => setTimeout(resolve, 5000)),
    ]);
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
  }

  async function hydrate(next: VideoInfo) {
    // Drop/open can fire twice for one file; don't run the analysis twice.
    const key = `${next.path}|${next.duration}`;
    const now = Date.now();
    if (lastHydrate.current && lastHydrate.current.key === key && now - lastHydrate.current.at < 2000) {
      return;
    }
    lastHydrate.current = { key, at: now };
    const replacing = video !== null; // changing videos, not first load
    setVideo(next);
    setChanges([]);
    setTags([]);
    setSelectedId(null);
    setCurrent(0);
    setCaptions([]);
    setTranscript({ text: "", words: [], source: "none" });
    logUi(
      `open ${next.name} (${next.duration.toFixed(1)}s, ${next.width}x${next.height}, audio=${next.hasAudio})`
    );
    setMessages((m) => [
      ...m,
      ...(replacing
        ? [
            {
              id: uid("div"),
              role: "system" as const,
              kind: "divider" as const,
              text: `new video — ${next.name}`,
            },
          ]
        : []),
      { id: uid("m"), role: "assistant" as const, icon: "check" as const, text: "Video loaded" },
    ]);
    try {
      setActivity("Scanning audio for quiet stretches…");
      try {
        const ranges = await detectSilence(next, {
          noise: settings?.silenceNoise,
          duration: settings?.silenceMin,
        });
        let merged: SilenceRange[] = ranges;
        if (ranges.length && native && next.path.startsWith("/")) {
          // A pause that hides a visual change shouldn't be cut through —
          // find those moments now so silence trims split around them.
          setActivity("Checking quiet stretches for visual changes…");
          try {
            const visual = await visualChangeTimes(next.path, ranges);
            merged = ranges.map((r, i) => ({ ...r, visual: visual[i] || [] }));
            const n = visual.filter((v) => v?.length).length;
            if (n) {
              setMessages((m) => [
                ...m,
                {
                  id: uid("m"),
                  role: "assistant",
                  text: `${n} quiet stretch${n === 1 ? "" : "es"} change${n === 1 ? "s" : ""} on screen — I'll keep those moments when trimming.`,
                },
              ]);
            }
          } catch (e) {
            logUi(`visual scan failed: ${e instanceof Error ? e.message : String(e)}`);
          }
        }
        setSilences(merged);
        if (ranges.length) {
          setMessages((m) => [
            ...m,
            {
              id: uid("m"),
              role: "assistant",
              text: `Found ${ranges.length} quiet stretch${ranges.length === 1 ? "" : "es"}.`,
            },
          ]);
        } else if (next.hasAudio) {
          setMessages((m) => [
            ...m,
            {
              id: uid("m"),
              role: "assistant",
              text: "No quiet stretches found. Silence floor/min are in Settings if the gaps are subtle.",
            },
          ]);
        }
      } catch (e) {
        setSilences([]);
        setMessages((m) => [
          ...m,
          {
            id: uid("m"),
            role: "assistant",
            text: `Silence scan failed (${e instanceof Error ? e.message : String(e)}). FFmpeg missing? Run npm run prep.`,
          },
        ]);
      }
    } finally {
      setActivity(null);
    }
    setMessages((m) => [
      ...m,
      {
        id: uid("m"),
        role: "assistant",
        text: "What do you want to edit? Trim silences is ready. Transcript is optional — about 3 minutes locally — and unlocks the script view plus “after I say…”.",
      },
    ]);
  }

  function patchStatus(id: string, status: Change["status"]) {
    setChanges((all) => all.map((c) => (c.id === id ? { ...c, status } : c)));
  }

  async function ensureTranscript(v: VideoInfo): Promise<Transcript> {
    if (transcript.source === "whisper" && (transcript.words.length || transcript.text)) {
      return transcript;
    }
    setActivity("Transcribing audio…");
    try {
      const whisper = await transcribeWhisper(v, settings?.whisperModel);
      setTranscript(whisper);
      if (whisper.text) {
        const ums = fillerCuts(whisper.words).length;
        setMessages((m) => [
          ...m,
          {
            id: uid("m"),
            role: "assistant",
            text: `Transcript ready${ums ? ` · ${ums} filler word${ums === 1 ? "" : "s"}` : ""}.`,
          },
        ]);
      }
      return whisper;
    } catch (e) {
      const empty: Transcript = { text: "", words: [], source: "none" };
      setTranscript(empty);
      setMessages((m) => [
        ...m,
        {
          id: uid("m"),
          role: "assistant",
          text: `Transcription failed (${e instanceof Error ? e.message : String(e)}). Requests like “trim after I say …” need the transcript.`,
        },
      ]);
      return empty;
    } finally {
      setActivity(null);
    }
  }

  async function loadProject(path: string) {
    const raw = await readText(path);
    const proj = JSON.parse(raw) as ProjectFile;
    if (proj.version !== 1 || !proj.video?.path) {
      throw new Error("not a Mustardy project");
    }
    const next = await loadVideoAt(proj.video.path);
    if (!next) throw new Error(`video missing: ${proj.video.path}`);
    lastHydrate.current = { key: `${next.path}|${next.duration}`, at: Date.now() };
    adoptBlobUrl(null); // switching to a file-path source — release any blob
    setVideo(next);
    setSilences(proj.silences || []);
    setTranscript(proj.transcript || { text: "", words: [], source: "none" });
    setChanges(sanitizeChanges(proj.changes || [], next.duration));
    setTags(proj.tags || []);
    setCaptions([]);
    setSelectedId(null);
    setCurrent(0);
    logUi(`open project ${path} → ${next.name}`);
    setMessages((m) => [
      ...m,
      { id: uid("div"), role: "system", kind: "divider", text: `project — ${next.name}` },
      {
        id: uid("m"),
        role: "assistant",
        icon: "check",
        text: `Reloaded ${next.name}: ${proj.changes?.length || 0} edit(s), ${proj.tags?.length || 0} tag(s)${proj.transcript?.words?.length ? `, ${proj.transcript.words.length} words` : ""}.`,
      },
    ]);
  }

  async function saveProject() {
    if (!video || !native) {
      setMessages((m) => [
        ...m,
        { id: uid("m"), role: "assistant", text: "Save needs the desktop app and a loaded video." },
      ]);
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
      transcript,
      changes,
      tags,
    };
    await writeText(dest, JSON.stringify(proj, null, 2));
    logUi(`saved project ${dest}`);
    setMessages((m) => [
      ...m,
      { id: uid("m"), role: "assistant", icon: "check", text: `Saved project to ${dest}` },
    ]);
  }

  async function exportTranscript() {
    if (!video) return;
    const live = await ensureTranscript(video);
    if (!live.text && !live.words.length) {
      setMessages((m) => [
        ...m,
        { id: uid("m"), role: "assistant", text: "No transcript to export." },
      ]);
      return;
    }
    const dest = await savePath({
      title: "Export transcript",
      defaultPath: video.name.replace(/\.[^.]+$/, "") + ".txt",
      filters: [
        { name: "Text", extensions: ["txt"] },
        { name: "SubRip", extensions: ["srt"] },
      ],
    });
    if (!dest) return;
    const body = /\.srt$/i.test(dest) ? transcriptToSrt(live) : transcriptToTxt(live);
    await writeText(dest, body);
    setMessages((m) => [
      ...m,
      { id: uid("m"), role: "assistant", icon: "check", text: `Wrote transcript to ${dest}` },
    ]);
  }

  async function send(text: string) {
    if (!video || !settings) return;
    setMessages((m) => [...m, { id: uid("m"), role: "user", text }]);
    setBusy(true);
    setActivity("Planning edits…");
    try {
      const { changes: incoming, reply } = await planEdits({
        prompt: text,
        video,
        silences,
        captions,
        transcript,
        changes,
        tags,
        settings,
        ensureTranscript: () => ensureTranscript(video),
      });

      if (incoming.length) {
        setChanges((all) => [...all, ...incoming]);
        setSelectedId(incoming[0].id);
        seek(incoming[0].start);
      }
      logUi(
        `plan "${text}" (${settings.provider}) → ${incoming.length} change(s): ${incoming
          .map((c) => `${c.type}@${c.start.toFixed(1)}-${c.end.toFixed(1)}`)
          .join(", ") || "none"}`
      );
      setMessages((m) => [
        ...m,
        {
          id: uid("m"),
          role: "assistant",
          text: reply || describePlan(incoming),
          changeIds: incoming.map((c) => c.id),
        },
      ]);
    } finally {
      setBusy(false);
      setActivity(null);
    }
  }

  async function onExport() {
    if (!video || !native) {
      setMessages((m) => [
        ...m,
        {
          id: uid("m"),
          role: "assistant",
          text: "Export bakes accepted edits with FFmpeg in the desktop app. Preview here is live; package Mustardy on your Mac to write the file.",
        },
      ]);
      return;
    }
    const accepted = changes.filter((c) => c.status === "accepted");
    if (!accepted.length) {
      setMessages((m) => [
        ...m,
        { id: uid("m"), role: "assistant", text: "Accept at least one change before export — or you’ll just re-wrap the source." },
      ]);
      return;
    }
    setBusy(true);
    try {
      const result = await exportProject({
        input: video.path,
        duration: video.duration,
        changes,
        suggestedName: video.name.replace(/\.[^.]+$/, "") + "-mustardy.mp4",
      });
      if (result.path) {
        setMessages((m) => [
          ...m,
          { id: uid("m"), role: "assistant", text: `Exported to ${result.path}` },
        ]);
        await reveal(result.path);
      }
    } catch (err) {
      setMessages((m) => [
        ...m,
        {
          id: uid("m"),
          role: "assistant",
          text: `Export failed: ${err instanceof Error ? err.message : "unknown error"}`,
        },
      ]);
    } finally {
      setBusy(false);
    }
  }

  async function onPackage() {
    if (!video) return;
    setBusy(true);
    setActivity("Looking at the trimmed cut…");
    try {
      const live = await ensureTranscript(video);
      const text = keptTranscript(live, video.duration, changes);
      const times = sampleKeepTimes(video.duration, changes, 10);
      const shots = await grabFrames(video.url, times, 720);
      setPack({ ideas: suggestPackage(text || live.text || video.name, video.name), frames: shots });
    } catch (e) {
      setMessages((m) => [
        ...m,
        {
          id: uid("m"),
          role: "assistant",
          text: `Couldn't build the package (${e instanceof Error ? e.message : String(e)}).`,
        },
      ]);
    } finally {
      setBusy(false);
      setActivity(null);
    }
  }

  function showBanner(eyes: EyesStatus, brain: BrainStatus) {
    return (
      eyes.state === "downloading" ||
      eyes.state === "error" ||
      brain.state === "downloading" ||
      brain.state === "error"
    );
  }

  function barStatus(eyes: EyesStatus, brain: BrainStatus): EyesStatus {
    if (eyes.state === "downloading") return eyes;
    if (brain.state === "downloading") {
      return { state: "downloading", progress: brain.progress, label: brain.label, model: brain.model };
    }
    if (eyes.state === "error") return eyes;
    if (brain.state === "error") {
      return { state: "error", progress: 0, label: brain.label, model: brain.model };
    }
    return eyes;
  }

  function selectChange(id: string) {
    setSelectedId(id);
    const c = changes.find((x) => x.id === id);
    if (c) seek(c.start + 0.01);
  }

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

  return (
    <div className={`app ${showBanner(eyes, brain) ? "has-banner" : ""}`}>
      <Titlebar
        provider={settings?.provider || "local"}
        eyes={eyes}
        brain={brain}
        theme={settings?.theme || "light"}
        onToggleTheme={toggleTheme}
        onOpen={loadFromPicker}
        onSave={() => void saveProject()}
        onTranscript={() => void (transcript.words.length ? exportTranscript() : ensureTranscript(video!))}
        onSettings={() => setSettingsOpen(true)}
        onExport={onExport}
        onPackage={() => void onPackage()}
        onQuit={() => void quitApp()}
        canSave={Boolean(video)}
        canTranscript={Boolean(video)}
        hasTranscript={transcript.words.length > 0}
        canExport={Boolean(video)}
        canPackage={Boolean(video)}
      />
      <EyesBar status={barStatus(eyes, brain)} />
      <div className="workspace">
        <div className="stage-col">
          <VideoStage
            video={video}
            videoRef={videoRef}
            current={current}
            playing={playing}
            changes={changes}
            onToggle={toggle}
            onSeek={seek}
            onTag={addTag}
            previewCuts={previewCuts}
            onTogglePreview={() => setPreviewCuts((v) => !v)}
            onOpen={loadFromPicker}
            onDropFile={loadFile}
          />
          <Timeline
            video={video}
            current={current}
            changes={changes}
            tags={tags}
            selectedId={selectedId}
            onSeek={(t) => {
              const src = isInAcceptedCut(t, changes) ? editedToSource(t, video?.duration || 0, changes) : t;
              seek(src);
            }}
            onSelect={selectChange}
            onRemoveTag={removeTag}
            onAccept={(id) => patchStatus(id, "accepted")}
            onReject={(id) => patchStatus(id, "rejected")}
          />
        </div>
        <div className="dock">
          <Chat messages={messages} busy={busy} activity={activity} disabled={!video} onSend={send} />
          <ChangeRail
            changes={changes}
            transcript={transcript}
            current={current}
            selectedId={selectedId}
            onSelect={selectChange}
            onSeek={seek}
            onAccept={(id) => patchStatus(id, "accepted")}
            onReject={(id) => patchStatus(id, "rejected")}
            onAcceptAll={() =>
              setChanges((all) => all.map((c) => (c.status === "pending" ? { ...c, status: "accepted" } : c)))
            }
            onRejectAll={() =>
              setChanges((all) => all.map((c) => (c.status === "pending" ? { ...c, status: "rejected" } : c)))
            }
            onTranscribe={() => {
              if (video) void ensureTranscript(video);
            }}
            transcribing={activity === "Transcribing audio…"}
            onCutWords={(from, to) => {
              const raw = wordRangeCut(transcript.words, from, to);
              if (!raw) return;
              const [cut] = sanitizeChanges([raw], video?.duration || 0);
              if (!cut) return;
              setChanges((all) => [...all, cut]);
              setSelectedId(cut.id);
              seek(cut.start);
            }}
          />
        </div>
      </div>
      {pack && (
        <PackageModal
          ideas={pack.ideas}
          frames={pack.frames}
          duration={video?.duration || 0}
          onClose={() => setPack(null)}
        />
      )}
      {settingsOpen && settings && (
        <SettingsModal
          settings={settings}
          models={models}
          kimiSaved={kimiSaved}
          onClose={() => setSettingsOpen(false)}
          onSave={async (next) => {
            // A freshly typed key goes to the desktop-side store; the
            // webview keeps it in memory only.
            if (native && next.kimiKey.trim()) {
              try {
                await saveKimiKey(next.kimiKey.trim());
                setKimiSaved(true);
              } catch (e) {
                logUi(`saving kimi key failed: ${e instanceof Error ? e.message : String(e)}`);
              }
            }
            const persisted = { ...next, kimiKey: "" };
            setSettings(persisted);
            await saveSettings(persisted);
          }}
        />
      )}
    </div>
  );
}
