import type { FrameShot } from "./frames";
import { captionFramesNative, native, onEngineStatus, warmEngine } from "./bridge";

export type EyesState = "idle" | "downloading" | "ready" | "error";
export type EyesStatus = {
  state: EyesState;
  progress: number;
  label: string;
  model: string;
};

export type Caption = { t: number; text: string };

const MODEL_ID = "SmolVLM-256M";
const listeners = new Set<(s: EyesStatus) => void>();

let status: EyesStatus = {
  state: "idle",
  progress: 0,
  label: "Eyes not loaded yet",
  model: MODEL_ID,
};
let boot: Promise<void> | null = null;
let eventsBound = false;

export function getEyesStatus() {
  return status;
}

export function onEyesStatus(fn: (s: EyesStatus) => void) {
  listeners.add(fn);
  fn(status);
  return () => {
    listeners.delete(fn);
  };
}

function setStatus(next: Partial<EyesStatus>) {
  status = { ...status, ...next };
  for (const fn of listeners) fn(status);
}

function bindEvents() {
  if (eventsBound) return;
  eventsBound = true;
  void onEngineStatus((e) => {
    if (e.engine !== "eyes" && e.engine !== "models") return;
    if (e.state === "downloading") {
      setStatus({ state: "downloading", progress: e.progress, label: e.label });
    } else if (e.engine === "eyes" && e.state === "loading") {
      setStatus({ state: "downloading", progress: 98, label: e.label });
    } else if (e.engine === "eyes" && e.state === "ready") {
      setStatus({ state: "ready", progress: 100, label: "Local eyes ready" });
    } else if (e.state === "error") {
      setStatus({ state: "error", progress: 0, label: e.label });
    }
  });
}

export function warmEyes() {
  if (!boot) boot = loadEyes();
  return boot;
}

async function loadEyes() {
  if (!native) {
    const err = new Error("Local eyes run in the desktop app.");
    setStatus({ state: "error", progress: 0, label: err.message });
    boot = null;
    throw err;
  }
  bindEvents();
  setStatus({ state: "downloading", progress: 2, label: "Warming the local eyes…" });
  try {
    await warmEngine("eyes");
  } catch (err) {
    setStatus({
      state: "error",
      progress: 0,
      label: err instanceof Error ? err.message : "Could not load local eyes",
    });
    boot = null;
    throw err;
  }
}

export async function captionFrames(frames: FrameShot[]): Promise<Caption[]> {
  await warmEyes();
  if (!native || !frames.length) return [];
  try {
    return await captionFramesNative(frames, 32);
  } catch {
    return [];
  }
}
