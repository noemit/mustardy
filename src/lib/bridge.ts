import type { Caption } from "./vision";
import type { Settings, VideoInfo } from "../types";

// Tauri v2 detection (works without the global flag).
export const native =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const platform = /mac/i.test(
  (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ||
    navigator.platform ||
    navigator.userAgent
)
  ? "darwin"
  : "web";

// Lazy imports keep the plain-web dev build working without a Tauri runtime.
async function api() {
  return import("@tauri-apps/api/core");
}
async function dialog() {
  return import("@tauri-apps/plugin-dialog");
}

export type EngineEvent = {
  engine: "eyes" | "brain" | "ears" | "models";
  state: "idle" | "downloading" | "loading" | "ready" | "error";
  progress: number;
  label: string;
};

export async function onEngineStatus(cb: (e: EngineEvent) => void) {
  if (!native) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<EngineEvent>("engine-status", (ev) => cb(ev.payload));
  return () => un();
}

/** Append one line to the shared mustardy.log (fire-and-forget). */
export function logUi(text: string) {
  if (!native) return;
  void api().then(({ invoke }) => invoke("log_line", { line: text })).catch(() => {});
}

export type ModelStatus = {
  id: string;
  file: string;
  present: boolean;
  bytes: number;
  path: string | null;
};

export async function modelStatus(): Promise<ModelStatus[]> {
  if (!native) return [];
  const { invoke } = await api();
  return invoke<ModelStatus[]>("model_status");
}

let downloading: Promise<void> | null = null;

/** Ensure the GGUF/ggml model files exist; downloads them once if missing. */
export function ensureModels(): Promise<void> {
  if (!native) return Promise.resolve();
  if (!downloading) {
    downloading = (async () => {
      const { invoke } = await api();
      const missing = (await modelStatus()).filter((m) => !m.present);
      if (missing.length) await invoke("download_models");
    })().finally(() => {
      downloading = null;
    });
  }
  return downloading;
}

export async function warmEngine(engine: "eyes" | "brain" | "ears") {
  if (!native) throw new Error("Native engines need the desktop app.");
  await ensureModels();
  const { invoke } = await api();
  await invoke("warm_engine", { engine });
}

export async function captionFramesNative(
  frames: Array<{ t: number; dataUrl: string }>,
  maxTokens = 32
): Promise<Caption[]> {
  const { invoke } = await api();
  const payload = frames.map((f) => ({
    t: f.t,
    data: f.dataUrl.slice(f.dataUrl.indexOf(",") + 1),
  }));
  return invoke<Caption[]>("caption_frames", { frames: payload, maxTokens });
}

export async function brainPlan(
  system: string,
  user: string,
  maxTokens = 420,
  attempts = 3,
  refine?: { path: string; silences: Array<[number, number]> }
): Promise<string | null> {
  const { invoke } = await api();
  return invoke<string | null>("brain_plan", {
    system,
    user,
    maxTokens,
    attempts,
    path: refine?.path ?? null,
    silences: refine?.silences ?? null,
  });
}

export async function transcribeNative(path: string, model?: string) {
  const { invoke } = await api();
  return invoke<{ text: string; words: Array<{ t: number; end: number; text: string }> }>(
    "transcribe",
    { path, model: model ?? null }
  );
}

export async function pickVideo(): Promise<VideoInfo | null> {
  const picked = await pickOpenPath({
    title: "Open video",
    filters: [{ name: "Video", extensions: ["mp4", "mov", "mkv", "webm", "m4v", "avi"] }],
    accept: "video/*",
  });
  if (!picked) return null;
  return loadVideoAt(picked);
}

export async function pickOpen(): Promise<
  { type: "video"; video: VideoInfo } | { type: "project"; path: string } | null
> {
  const picked = await pickOpenPath({
    title: "Open video or project",
    filters: [
      { name: "Video or Mustardy project", extensions: ["mp4", "mov", "mkv", "webm", "m4v", "avi", "json"] },
    ],
    accept: "video/*,.json",
  });
  if (!picked) return null;
  if (/\.json$/i.test(picked) || /\.mustardy\.json$/i.test(picked)) {
    return { type: "project", path: picked };
  }
  const video = await loadVideoAt(picked);
  return video ? { type: "video", video } : null;
}

async function pickOpenPath(opts: {
  title: string;
  filters: Array<{ name: string; extensions: string[] }>;
  accept: string;
}): Promise<string | null> {
  if (native) {
    const { open } = await dialog();
    const picked = await open({ title: opts.title, multiple: false, filters: opts.filters });
    return typeof picked === "string" ? picked : null;
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = opts.accept;
    input.onchange = () => {
      const file = input.files?.[0];
      resolve(file ? file.name : null);
    };
    input.click();
  });
}

export async function loadVideoAt(path: string): Promise<VideoInfo | null> {
  if (native && path.startsWith("/")) {
    const { invoke, convertFileSrc } = await api();
    const meta = await invoke<Omit<VideoInfo, "url" | "path">>("probe", { path });
    return { ...meta, path, url: convertFileSrc(path) };
  }
  return null;
}

export async function savePath(opts: {
  title: string;
  defaultPath: string;
  filters: Array<{ name: string; extensions: string[] }>;
}): Promise<string | null> {
  if (!native) return null;
  const { save } = await dialog();
  const out = await save({ title: opts.title, defaultPath: opts.defaultPath, filters: opts.filters });
  return out || null;
}

export async function writeText(path: string, contents: string) {
  const { invoke } = await api();
  await invoke("write_text", { path, contents });
}

export async function readText(path: string) {
  const { invoke } = await api();
  return invoke<string>("read_text", { path });
}

export async function quitApp() {
  if (!native) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().close();
}

export async function detectSilence(
  video: VideoInfo,
  opts?: { noise?: string; duration?: number }
) {
  if (native && video.path.startsWith("/")) {
    const { invoke } = await api();
    return invoke<Array<{ start: number; end: number }>>("detect_silence", {
      path: video.path,
      noise: opts?.noise,
      duration: opts?.duration,
    });
  }
  return detectSilenceWeb(video.url, opts?.duration ?? 0.45);
}

/** Per silence range, the times where the picture changes mid-pause
 * (pixel-diff scan in the core; aligned with the input ranges). */
export async function visualChangeTimes(
  path: string,
  ranges: Array<{ start: number; end: number }>
): Promise<number[][]> {
  const { invoke } = await api();
  return invoke<number[][]>("visual_change_times", { path, ranges });
}

async function detectSilenceWeb(url: string, minDur: number) {
  const ctx = new AudioContext();
  try {
    const buf = await (await fetch(url)).arrayBuffer();
    const audio = await ctx.decodeAudioData(buf);
    const data = audio.getChannelData(0);
    const sr = audio.sampleRate;
    const hop = Math.max(1, Math.floor(sr * 0.02));
    const ranges: Array<{ start: number; end: number }> = [];
    let start: number | null = null;
    for (let i = 0; i < data.length; i += hop) {
      let peak = 0;
      for (let j = 0; j < hop && i + j < data.length; j++) {
        const s = Math.abs(data[i + j]);
        if (s > peak) peak = s;
      }
      const t = i / sr;
      if (peak < 0.012) {
        if (start == null) start = t;
      } else if (start != null) {
        if (t - start >= minDur) ranges.push({ start, end: t });
        start = null;
      }
    }
    if (start != null && audio.duration - start >= minDur) {
      ranges.push({ start, end: audio.duration });
    }
    return ranges;
  } catch {
    return [];
  } finally {
    void ctx.close();
  }
}

export async function exportProject(payload: Record<string, unknown>) {
  if (!native) {
    throw new Error("Export needs the desktop app so FFmpeg can bake the cut.");
  }
  const { save } = await dialog();
  const output = await save({
    title: "Export video",
    defaultPath: (payload.suggestedName as string) || "mustardy-export.mp4",
    filters: [{ name: "MP4", extensions: ["mp4"] }],
  });
  if (!output) return { canceled: true };
  const { invoke } = await api();
  await invoke("export_project", {
    payload: {
      input: payload.input,
      duration: payload.duration,
      changes: payload.changes,
      output,
    },
  });
  return { canceled: false, path: output };
}

export async function reveal(filePath: string) {
  if (!native) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(filePath);
}

export async function chat(payload: Record<string, unknown>) {
  if (native) {
    const { invoke } = await api();
    return invoke<{ text: string; provider: string; model: string }>("chat_ai", {
      provider: payload.provider,
      settings: payload.settings,
      messages: payload.messages,
      system: payload.system,
    });
  }
  const settings = (payload.settings || {}) as Settings;
  const provider = payload.provider as string;
  if (provider === "kimi" || provider === "local") {
    throw new Error("Kimi calls run from the desktop app so the API key stays off the page.");
  }
  const url = `${String(settings.ollamaUrl || "http://127.0.0.1:11434").replace(/\/$/, "")}/api/chat`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      model: settings.qwenModel || "qwen2.5:7b",
      stream: false,
      format: "json",
      messages: [
        { role: "system", content: payload.system },
        ...((payload.messages as Array<{ role: string; content: string }>) || []),
      ],
      options: { temperature: 0.4 },
    }),
  });
  if (!res.ok) {
    throw new Error(
      res.status === 404
        ? `Ollama is missing ${settings.qwenModel}. Run ollama pull ${settings.qwenModel || "qwen2.5:7b"}`
        : await res.text()
    );
  }
  const data = await res.json();
  return { text: data.message?.content || "", provider: "qwen", model: settings.qwenModel };
}

export async function listModels(payload: { provider: string; settings: Settings }) {
  if (native) {
    const { invoke } = await api();
    return invoke<{ models: string[]; online: boolean }>("list_ai_models", {
      provider: payload.provider,
      settings: payload.settings,
    });
  }
  try {
    const res = await fetch(
      `${String(payload.settings.ollamaUrl || "http://127.0.0.1:11434").replace(/\/$/, "")}/api/tags`
    );
    if (!res.ok) return { models: [], online: false };
    const data = await res.json();
    return { models: (data.models || []).map((m: { name: string }) => m.name), online: true };
  } catch {
    return { models: [], online: false };
  }
}

export async function loadSettings(): Promise<Settings> {
  const stored = JSON.parse(localStorage.getItem("mustardy-settings") || "{}");
  const legacy = stored.provider === "qwen" ? "local" : stored.provider;
  return {
    provider: legacy || "local",
    ollamaUrl: stored.ollamaUrl || "http://127.0.0.1:11434",
    qwenModel: stored.qwenModel || "qwen2.5:7b",
    kimiKey: stored.kimiKey || "",
    kimiBaseUrl: stored.kimiBaseUrl || "https://api.moonshot.ai/v1",
    kimiModel: stored.kimiModel || "moonshot-v1-32k-vision-preview",
    silenceNoise: stored.silenceNoise || "-30dB",
    silenceMin: stored.silenceMin ?? 0.6,
    whisperModel: stored.whisperModel === "small.en" ? "small.en" : "tiny.en",
  };
}

export async function saveSettings(next: Settings) {
  localStorage.setItem("mustardy-settings", JSON.stringify(next));
}
