import { describe, expect, it } from "vitest";
import { hardSeekNeeded, keepContaining, snapKeepsToFps } from "./cutPreview";

describe("keepContaining", () => {
  const keeps = [
    { start: 0, end: 10 },
    { start: 12, end: 20 },
  ];
  it("finds the keep that contains t", () => {
    expect(keepContaining(keeps, 5)).toBe(0);
    expect(keepContaining(keeps, 15)).toBe(1);
  });
  it("points at the next keep when t sits in a cut", () => {
    expect(keepContaining(keeps, 11)).toBe(1);
  });
});

describe("snapKeepsToFps", () => {
  it("leaves exact frame edges alone", () => {
    expect(snapKeepsToFps([{ start: 0, end: 10 }], 30)).toEqual([{ start: 0, end: 10 }]);
  });

  it("snaps mid-frame edges up to the next frame boundary", () => {
    const [k] = snapKeepsToFps([{ start: 10.01, end: 12.21 }], 30);
    expect(k.start).toBeCloseTo(301 / 30, 5);
    expect(k.end).toBeCloseTo(367 / 30, 5);
  });

  it("drops keeps that collapse inside one frame", () => {
    expect(snapKeepsToFps([{ start: 1.001, end: 1.01 }], 30)).toEqual([]);
  });
});

describe("hardSeekNeeded", () => {
  const tol = 0.025;

  it("does not hard-seek for a one-frame overrun", () => {
    expect(hardSeekNeeded(3, 0.03, tol)).toBe(false);
  });

  it("hard-seeks once the audible player is well past the cut edge", () => {
    expect(hardSeekNeeded(3, 0.08, tol)).toBe(true);
  });

  it("hard-seeks when the target is ahead but the playhead entered the gap", () => {
    expect(hardSeekNeeded(3, -0.05, tol)).toBe(true);
  });
});
