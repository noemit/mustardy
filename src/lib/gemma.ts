import type { Change, VideoInfo } from "../types";
import { normalizeDraft, parseAgentJson } from "./edits";
import type { Caption } from "./vision";
import { brainPlan, native, onEngineStatus, warmEngine } from "./bridge";
import { toolListPrompt } from "./tools";

export type BrainStatus = {
  state: "idle" | "downloading" | "ready" | "error";
  progress: number;
  label: string;
  model: string;
};

const MODEL_ID = "SmolLM2-1.7B";
const listeners = new Set<(s: BrainStatus) => void>();

let status: BrainStatus = {
  state: "idle",
  progress: 0,
  label: "Editor not loaded",
  model: MODEL_ID,
};
let boot: Promise<void> | null = null;
let eventsBound = false;

export function getBrainStatus() {
  return status;
}

export function onBrainStatus(fn: (s: BrainStatus) => void) {
  listeners.add(fn);
  fn(status);
  return () => {
    listeners.delete(fn);
  };
}

function setStatus(next: Partial<BrainStatus>) {
  status = { ...status, ...next };
  for (const fn of listeners) fn(status);
}

function bindEvents() {
  if (eventsBound) return;
  eventsBound = true;
  void onEngineStatus((e) => {
    if (e.engine !== "brain" && e.engine !== "models") return;
    if (e.state === "downloading") {
      setStatus({ state: "downloading", progress: e.progress, label: e.label });
    } else if (e.engine === "brain" && e.state === "loading") {
      setStatus({ state: "downloading", progress: 98, label: e.label });
    } else if (e.engine === "brain" && e.state === "ready") {
      setStatus({ state: "ready", progress: 100, label: "SmolLM2 ready" });
    } else if (e.state === "error") {
      setStatus({ state: "error", progress: 0, label: e.label });
    }
  });
}

export function warmBrain() {
  if (!boot) boot = loadBrain();
  return boot;
}

async function loadBrain() {
  if (!native) {
    const err = new Error("The local brain runs in the desktop app.");
    setStatus({ state: "error", progress: 0, label: err.message });
    boot = null;
    throw err;
  }
  bindEvents();
  setStatus({ state: "downloading", progress: 2, label: "Warming SmolLM2…" });
  try {
    await warmEngine("brain");
  } catch (err) {
    setStatus({
      state: "error",
      progress: 0,
      label: err instanceof Error ? err.message : "SmolLM2 failed to load",
    });
    boot = null;
    throw err;
  }
}

/**
 * The brain only chooses tools — it returns a changes JSON; the deterministic
 * describePlan() in the UI writes the user-facing message. The Rust side
 * retries generation until the JSON parses with at least one change.
 */
export async function planWithGemma(input: {
  prompt: string;
  video: VideoInfo;
  silences: Array<{ start: number; end: number }>;
  captions: Caption[];
  transcript?: string;
  existing: Array<Pick<Change, "type" | "start" | "end" | "status" | "label">>;
  silencesHandled?: boolean;
}): Promise<{ message: string; changes: Change[] }> {
  if (!native) return { message: "", changes: [] };
  await warmBrain();

  const user = [
    `Video ${input.video.duration.toFixed(1)}s ${input.video.width}x${input.video.height}.`,
    input.silences.length
      ? `Silences: ${JSON.stringify(input.silences.map((s) => [round(s.start), round(s.end)]))}`
      : "No silences.",
    ...(input.silencesHandled
      ? ["Silence trims are already queued as cuts — do NOT emit cuts for silences."]
      : []),
    input.captions.length
      ? `Frames: ${input.captions.map((c) => `${round(c.t)}s ${c.text}`).join(" | ")}`
      : "No captions.",
    input.transcript ? `Transcript: ${input.transcript}` : "No transcript.",
    input.existing.length
      ? `Already queued: ${input.existing.map((c) => `${c.type}@${round(c.start)}`).join(", ")}`
      : "No edits yet.",
    `User: ${input.prompt}`,
  ].join("\n");

  const refine = input.video.path.startsWith("/")
    ? { path: input.video.path, silences: input.silences.map((s) => [s.start, s.end] as [number, number]) }
    : undefined;
  const json = await brainPlan(BRAIN_PROMPT, user, 420, 3, refine);
  if (!json) return { message: "", changes: [] };
  const parsed = parseAgentJson(json);
  return {
    message: "",
    changes: normalizeDraft(parsed.draft, input.video.duration).slice(0, 10),
  };
}

function round(n: number) {
  return Number(n.toFixed(2));
}

const BRAIN_PROMPT = `You are Mustardy, a punchy video editor.
Pick tools. Return ONLY JSON:
{"changes":[{"type":"cut","start":0,"end":1,"label":"","rationale":"","text":"","style":"youtube","pan":{"kind":"zoom-in"},"rate":0.5}]}

Tools:
${toolListPrompt()}

Rules:
- Times are source seconds. Stay inside the video.
- 3–8 changes. Stronger > more.
- cut only on listed silences or filler words from the transcript unless the user names a range.
- Asked to trim silences/pauses? Emit one cut per listed silence, using the listed times.
- text / textcard / impact need short ALL-CAPS copy, max 6 words.
- Match tools to what the frames show and what the user asked.
- Funny panic → shake or impact. Reveal → freeze then textcard. Talking head → overlay + punch on the joke.`;
