import type { Settings, VideoInfo } from "../types";
import {
  envelopeFromSamples,
  DEFAULT_SILENCE_DROP,
  DEFAULT_SILENCE_MIN,
  type AudioEnvelope,
} from "./silence";

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

export type ExportProgress = {
  progress: number;
  label: string;
};

export async function onExportProgress(cb: (p: ExportProgress) => void) {
  if (!native) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<ExportProgress>("export-progress", (ev) => cb(ev.payload));
  return () => un();
}

/** Append one line to the shared mustardy.log (fire-and-forget). */
export function logUi(text: string) {
  if (!native) return;
  void api().then(({ invoke }) => invoke("log_line", { line: text })).catch(() => {});
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
  if (!video) throw new Error(`probe failed for ${picked} — ffprobe couldn't read the file`);
  return { type: "video", video };
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
    try {
      const { invoke, convertFileSrc } = await api();
      const meta = await invoke<Omit<VideoInfo, "url" | "path">>("probe", { path });
      return { ...meta, path, url: convertFileSrc(path) };
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      logUi(`probe failed for ${path}: ${msg}`);
      throw e;
    }
  }
  // Web fallback or non-absolute path: let caller fall back to blob handling.
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

export async function loadAudioEnvelope(video: VideoInfo): Promise<AudioEnvelope | null> {
  if (native && video.path.startsWith("/")) {
    try {
      const { invoke } = await api();
      return await invoke<AudioEnvelope>("audio_envelope", { path: video.path });
    } catch (e) {
      logUi(`audio envelope failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  return envelopeFromUrl(video.url);
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

async function envelopeFromUrl(url: string): Promise<AudioEnvelope | null> {
  // Decode straight into a 16 kHz mono context — decodeAudioData resamples
  // to the context rate, so long videos cost ~1/6 the RAM of decoding at the
  // file's own stereo/full rate.
  const ctx = new OfflineAudioContext(1, 1, 16000);
  try {
    const buf = await (await fetch(url)).arrayBuffer();
    const audio = await ctx.decodeAudioData(buf);
    return envelopeFromSamples(audio.getChannelData(0), audio.sampleRate);
  } catch {
    return null;
  }
}

export async function exportProject(
  payload: Record<string, unknown>,
  onProgress?: (p: ExportProgress) => void
) {
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
  const unProgress = onProgress ? await onExportProgress(onProgress) : () => {};
  try {
    await invoke("export_project", {
      payload: {
        input: payload.input,
        duration: payload.duration,
        changes: payload.changes,
        output,
        normalize: Boolean(payload.normalize),
        normalizeAmount: Number(payload.normalizeAmount ?? 0.7),
      },
    });
  } finally {
    unProgress();
  }
  return { canceled: false, path: output };
}

export async function reveal(filePath: string) {
  if (!native) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(filePath);
}

export async function loadSettings(): Promise<Settings> {
  const stored = JSON.parse(localStorage.getItem("mustardy-settings") || "{}");
  return {
    silenceMin: stored.silenceMin ?? DEFAULT_SILENCE_MIN,
    silenceDrop:
      typeof stored.silenceDrop === "number" && Number.isFinite(stored.silenceDrop)
        ? Math.min(28, Math.max(8, stored.silenceDrop))
        : DEFAULT_SILENCE_DROP,
    normalizeAudio: stored.normalizeAudio !== false,
    normalizeAmount:
      typeof stored.normalizeAmount === "number" && Number.isFinite(stored.normalizeAmount)
        ? Math.min(1, Math.max(0, stored.normalizeAmount))
        : 0.7,
    theme: stored.theme === "dark" ? "dark" : "light",
  };
}

export async function saveSettings(next: Settings) {
  localStorage.setItem("mustardy-settings", JSON.stringify(next));
}
