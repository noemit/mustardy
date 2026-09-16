import type { SilenceRange } from "../types";

/** RMS windows over the file, in dBFS. One scan; sliders re-threshold this. */
export type AudioEnvelope = {
  hop: number;
  dbs: number[];
};

export const ENVELOPE_HOP_S = 0.02;
export const MIN_SILENCE_S = 0.02;
/** dB below talking before a gap counts as a pause. Higher = keep quieter speech. */
export const DEFAULT_SILENCE_DROP = 30;
export const DEFAULT_SILENCE_MIN = 0.9;
const HYST_DB = 4;
const NOISE_HEADROOM_DB = 3;

export function parseSilenceDb(value: string | undefined, fallback = -30): number {
  const n = Number(String(value || "").replace(/dB$/i, "").trim());
  return Number.isFinite(n) ? n : fallback;
}

export function envelopeFromSamples(data: Float32Array, sampleRate: number): AudioEnvelope {
  const hopSamples = Math.max(1, Math.round(sampleRate * ENVELOPE_HOP_S));
  const hop = hopSamples / sampleRate;
  const dbs: number[] = [];
  for (let i = 0; i < data.length; i += hopSamples) {
    let sum = 0;
    let n = 0;
    for (let j = 0; j < hopSamples && i + j < data.length; j++) {
      const s = data[i + j];
      sum += s * s;
      n++;
    }
    const rms = Math.sqrt(sum / Math.max(1, n));
    dbs.push(20 * Math.log10(Math.max(1e-9, rms)));
  }
  return { hop, dbs };
}

function percentile(sorted: number[], p: number) {
  if (!sorted.length) return -80;
  const i = Math.min(sorted.length - 1, Math.max(0, Math.round((p / 100) * (sorted.length - 1))));
  return sorted[i];
}

/**
 * Pause = a stretch near room tone, not "anything quieter than a loud word."
 * Talking level is the 80th percentile; a gap must fall `dropDb` below that
 * (and stay a bit above the noise floor so the threshold can't dive into hiss).
 */
export function silencesFromEnvelope(
  env: AudioEnvelope,
  dropDb: number,
  minDur: number
): SilenceRange[] {
  const { hop, dbs } = env;
  if (!dbs.length || !(hop > 0)) return [];
  const min = Math.max(0, minDur);
  const drop = Math.min(35, Math.max(8, dropDb));
  const sorted = [...dbs].sort((a, b) => a - b);
  const noise = percentile(sorted, 12);
  const speech = percentile(sorted, 80);
  const contrast = speech - noise;
  const thr =
    contrast < 8 ? speech - drop : Math.max(noise + NOISE_HEADROOM_DB, speech - drop);

  const ranges: SilenceRange[] = [];
  let start: number | null = null;
  let inPause = false;
  for (let i = 0; i < dbs.length; i++) {
    const t = i * hop;
    if (!inPause) {
      if (dbs[i] < thr) {
        inPause = true;
        start = t;
      }
    } else if (dbs[i] > thr + HYST_DB) {
      if (start != null && t - start >= min) ranges.push({ start, end: t });
      inPause = false;
      start = null;
    }
  }
  if (inPause && start != null) {
    const end = dbs.length * hop;
    if (end - start >= min) ranges.push({ start, end });
  }

  const gap = Math.min(0.05, min);
  const merged: SilenceRange[] = [];
  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && r.start - last.end <= gap) last.end = r.end;
    else merged.push({ start: r.start, end: r.end });
  }
  return merged;
}

export function silenceTotal(ranges: SilenceRange[]) {
  return ranges.reduce((n, r) => n + Math.max(0, r.end - r.start), 0);
}

export function wavePeaks(dbs: number[], buckets = 280): number[] {
  if (!dbs.length) return [];
  const n = Math.min(buckets, dbs.length);
  const out = new Array<number>(n);
  const step = dbs.length / n;
  for (let i = 0; i < n; i++) {
    let peak = -120;
    const a = Math.floor(i * step);
    const b = Math.max(a + 1, Math.floor((i + 1) * step));
    for (let j = a; j < b && j < dbs.length; j++) if (dbs[j] > peak) peak = dbs[j];
    out[i] = peak;
  }
  return out;
}
