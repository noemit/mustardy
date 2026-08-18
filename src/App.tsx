import { useCallback, useEffect, useRef, useState } from "react";
import { ChangeRail } from "./components/ChangeRail";
import { Chat } from "./components/Chat";
import { SettingsModal } from "./components/SettingsModal";
import { Timeline } from "./components/Timeline";
import { Titlebar } from "./components/Titlebar";
import { VideoStage } from "./components/VideoStage";
import { EyesBar } from "./components/EyesBar";
import {
  chat,
  detectSilence,
  exportProject,
  listModels,
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
  SYSTEM_PROMPT,
  dedupeCuts,
  editedToSource,
  explicitTimeCuts,
  formatTime,
  isInAcceptedCut,
  nextTagName,
  skippableCuts,
  normalizeDraft,
  parseAgentJson,
  silenceCuts,
  uid,
  unknownTagMentions,
} from "./lib/edits";
import {
  getBrainStatus,
  onBrainStatus,
  planWithGemma,
  type BrainStatus,
} from "./lib/gemma";
import { describePlan, describeScene, localPlan } from "./lib/planner";
import {
  fillerCuts,
  formatTranscript,
  phraseTimeCuts,
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
  const [eyes, setEyes] = useState<EyesStatus>(getEyesStatus());
  const [brain, setBrain] = useState<BrainStatus>(getBrainStatus());

  useEffect(() => {
    loadSettings().then(setSettings);
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
    const probeEl = document.createElement("video");
    probeEl.preload = "metadata";
    probeEl.src = url;
    await new Promise<void>((resolve) => {
      probeEl.onloadedmetadata = () => resolve();
      probeEl.onerror = () => resolve();
    });
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
    setVideo(next);
    setSilences(proj.silences || []);
    setTranscript(proj.transcript || { text: "", words: [], source: "none" });
    setChanges(proj.changes || []);
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
      const context = [
        `Video: ${video.name}, duration ${video.duration.toFixed(2)}s, ${video.width}x${video.height}.`,
        silences.length
          ? `Detected silences: ${JSON.stringify(silences.map((s) => [Number(s.start.toFixed(2)), Number(s.end.toFixed(2))]))}`
          : "No silence ranges detected (or FFmpeg not available in this session).",
        changes.length
          ? `Existing changes: ${JSON.stringify(changes.map((c) => ({ type: c.type, start: c.start, end: c.end, status: c.status, label: c.label })))}`
          : "No edits yet.",
      ].join("\n");

      let reply = "";
      let incoming: Change[] = [];
      let seen = captions;

      // Deterministic cuts first — user-named times ("trim everything after
      // 17:07", "cut between @a45 and @b34") and transcript phrases ("trim
      // after I say thanks") never go through a model. Silence trims are
      // mechanical too: built straight from the audio scan, but only when the
      // user actually names silences (a bare "trim …" is not a silence request).
      const needsWords =
        /\b(after|before)\s+(?:i|we)\s+say\b|\bfiller\b|\bum+\b|\bstutter\b|\btranscrib/i.test(
          text
        );
      const liveTranscript = needsWords ? await ensureTranscript(video) : transcript;
      const explicitCuts = explicitTimeCuts(text, video.duration, tags);
      const unknownTags = unknownTagMentions(text, tags);
      const phraseResult = phraseTimeCuts(text, liveTranscript, video.duration);
      const wantsSilence = /silence|dead air|tight|pause|quiet|gap/.test(text.toLowerCase());
      const silencePreCuts = dedupeCuts(wantsSilence ? silenceCuts(silences) : [], changes);
      const preCuts = [...explicitCuts, ...phraseResult.cuts, ...silencePreCuts];
      const plannerSilences = preCuts.length ? [] : silences; // planners add their own otherwise
      const hushNote =
        wantsSilence && silences.length
          ? "\nSilence trims are already queued as cuts — do NOT emit cuts for silences."
          : "";

      try {
        if (settings.provider === "kimi" || settings.provider === "ollama") {
          const result = await chat({
            provider: settings.provider,
            settings,
            system: SYSTEM_PROMPT,
            images: [],
            messages: [
              {
                role: "user",
                content: `${context}${hushNote}\n\nTranscript: ${formatTranscript(liveTranscript)}\n\nUser: ${text}`,
              },
            ],
          });
          const parsed = parseAgentJson(result.text);
          reply = parsed.message;
          incoming = normalizeDraft(parsed.draft, video.duration);
        } else {
          // Built-in vocabulary first — most edits come from the transcript,
          // the audio scan, and what the user literally asked. The local
          // brain only spins up when nothing matched (loads on demand).
          incoming = localPlan(text, video, plannerSilences, seen, liveTranscript);
          if (!incoming.length && !preCuts.length) {
            const planned = await planWithGemma({
              prompt: text,
              video,
              silences,
              captions: seen,
              transcript: formatTranscript(liveTranscript),
              existing: changes,
              silencesHandled: wantsSilence && silences.length > 0,
            });
            incoming = planned.changes;
          }
          reply = describePlan(incoming, describeScene(seen));
        }
      } catch (err) {
        incoming = localPlan(text, video, plannerSilences, seen, liveTranscript);
        reply = `${describePlan(incoming, describeScene(seen))}\n\n(${err instanceof Error ? err.message : "model unavailable"} — used the built-in editor.)`;
      }

      if (!incoming.length && !preCuts.length) {
        const fallback = localPlan(text, video, plannerSilences, seen, liveTranscript);
        if (fallback.length) {
          incoming = fallback;
          if (!reply) reply = describePlan(fallback);
        }
      }

      // Drop planner cuts that redo a range we already queued deterministically.
      if (preCuts.length) {
        incoming = incoming.filter(
          (c) => c.type !== "cut" || !preCuts.some((p) => c.start < p.end && c.end > p.start)
        );
      }
      const plannerChanges = incoming;
      incoming = [...preCuts, ...plannerChanges].sort((a, b) => a.start - b.start);

      // Report deterministic cuts in the app's own words, and keep whatever
      // the planner said about its own changes instead of burying it.
      if (preCuts.length || wantsSilence || phraseResult.phrase || unknownTags.length) {
        const notes: string[] = [];
        for (const c of [...explicitCuts, ...phraseResult.cuts]) notes.push(`Queued: ${c.label}.`);
        if (unknownTags.length) {
          notes.push(
            `No tag ${unknownTags.map((n) => `@${n}`).join(", ")} — press T to tag the playhead first.`
          );
        }
        if (phraseResult.phrase && !phraseResult.cuts.length) {
          notes.push(
            liveTranscript.words.length
              ? `I couldn't find “${phraseResult.phrase}” in the transcript.`
              : `Can't look for “${phraseResult.phrase}” — no transcript.`
          );
        }
        if (silencePreCuts.length) {
          notes.push(
            `Queued ${silencePreCuts.length} silence trim${silencePreCuts.length === 1 ? "" : "s"} from the audio scan.`
          );
        } else if (wantsSilence) {
          notes.push(
            silences.length ? "Those silences are already queued." : "No quiet stretches on file — nothing to trim."
          );
        }
        const rest = plannerChanges.length
          ? reply || describePlan(plannerChanges, describeScene(seen))
          : "";
        reply = [notes.join(" "), rest].filter(Boolean).join("\n\n");
      }

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
        onOpen={loadFromPicker}
        onSave={() => void saveProject()}
        onTranscript={() => void (transcript.words.length ? exportTranscript() : ensureTranscript(video!))}
        onSettings={() => setSettingsOpen(true)}
        onExport={onExport}
        onQuit={() => void quitApp()}
        canSave={Boolean(video)}
        canTranscript={Boolean(video)}
        hasTranscript={transcript.words.length > 0}
        canExport={Boolean(video)}
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
              const cut = wordRangeCut(transcript.words, from, to);
              if (!cut) return;
              setChanges((all) => [...all, cut]);
              setSelectedId(cut.id);
              seek(cut.start);
            }}
          />
        </div>
      </div>
      {settingsOpen && settings && (
        <SettingsModal
          settings={settings}
          models={models}
          onClose={() => setSettingsOpen(false)}
          onSave={async (next) => {
            setSettings(next);
            await saveSettings(next);
          }}
        />
      )}
    </div>
  );
}
