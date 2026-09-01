import { describe, expect, it } from "vitest";
import { envelopeFromSamples, silencesFromEnvelope, wavePeaks } from "./silence";

function tone(seconds: number, amp: number, sr = 16000) {
  const n = Math.floor(seconds * sr);
  const data = new Float32Array(n);
  data.fill(amp);
  return data;
}

describe("silencesFromEnvelope", () => {
  it("flags a quiet stretch well below talking", () => {
    const sr = 16000;
    const data = new Float32Array(sr * 2);
    data.fill(0.2, 0, sr);
    data.fill(0.0001, sr, sr * 2);
    const env = envelopeFromSamples(data, sr);
    const ranges = silencesFromEnvelope(env, 16, 0.2);
    expect(ranges.length).toBe(1);
    expect(ranges[0].start).toBeGreaterThan(0.8);
    expect(ranges[0].end).toBeGreaterThan(1.5);
  });

  it("keeps quieter speech that is still well above room tone", () => {
    const sr = 16000;
    const data = new Float32Array(sr * 4);
    data.fill(0.2, 0, sr * 3);
    data.fill(0.04, sr * 3, Math.floor(sr * 3.5));
    data.fill(0.0001, Math.floor(sr * 3.5), sr * 4);
    const env = envelopeFromSamples(data, sr);
    const ranges = silencesFromEnvelope(env, 18, 0.2);
    expect(ranges.length).toBe(1);
    expect(ranges[0].start).toBeGreaterThan(3.3);
  });

  it("keeps pauses shorter than 0.1s when min length allows", () => {
    const sr = 16000;
    const data = tone(1, 0.3, sr);
    data.fill(0.00001, Math.floor(sr * 0.4), Math.floor(sr * 0.46));
    const env = envelopeFromSamples(data, sr);
    expect(silencesFromEnvelope(env, 16, 0.2)).toHaveLength(0);
    const short = silencesFromEnvelope(env, 16, 0.04);
    expect(short.length).toBe(1);
    expect(short[0].end - short[0].start).toBeGreaterThan(0.04);
    expect(short[0].end - short[0].start).toBeLessThan(0.15);
  });

  it("a larger drop (must be quieter) finds less silence", () => {
    const sr = 16000;
    const data = tone(2, 0.15, sr);
    data.fill(0.003, sr, Math.floor(sr * 1.6));
    const env = envelopeFromSamples(data, sr);
    const tight = silencesFromEnvelope(env, 24, 0.15);
    const loose = silencesFromEnvelope(env, 10, 0.15);
    expect(silenceSpan(loose)).toBeGreaterThanOrEqual(silenceSpan(tight));
  });
});

describe("wavePeaks", () => {
  it("downsamples without emptying", () => {
    expect(wavePeaks([-20, -30, -10, -80], 2).length).toBe(2);
  });
});

function silenceSpan(ranges: Array<{ start: number; end: number }>) {
  return ranges.reduce((n, r) => n + (r.end - r.start), 0);
}
