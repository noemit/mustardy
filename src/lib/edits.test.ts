import { describe, expect, it } from "vitest";
import {
  dedupeCuts,
  explicitTimeCuts,
  formatTime,
  invertCuts,
  manualCut,
  nextTagName,
  sanitizeChanges,
  silenceCuts,
  unknownTagMentions,
} from "./edits";
import type { Change, Tag } from "../types";

const cut = (start: number, end: number, status: Change["status"] = "pending"): Change => ({
  id: `c${start}-${end}`,
  type: "cut",
  start,
  end,
  status,
  label: "test",
  rationale: "test",
});

describe("formatTime", () => {
  it("formats m:ss.tenth and clamps garbage", () => {
    expect(formatTime(65.42)).toBe("1:05.4");
    expect(formatTime(-3)).toBe("0:00.0");
    expect(formatTime(NaN)).toBe("0:00.0");
  });
});

describe("invertCuts", () => {
  it("merges overlapping cuts and keeps the gaps + tail", () => {
    const keep = invertCuts(100, [
      cut(10, 20, "accepted"),
      cut(18, 30, "accepted"),
      cut(40, 50, "accepted"),
    ]);
    expect(keep).toEqual([
      { start: 0, end: 10 },
      { start: 30, end: 40 },
      { start: 50, end: 100 },
    ]);
  });

  it("ignores rejected cuts", () => {
    const keep = invertCuts(10, [cut(2, 8, "rejected")]);
    expect(keep).toEqual([{ start: 0, end: 10 }]);
  });

  it("can include pending cuts for preview", () => {
    const keep = invertCuts(20, [cut(5, 8, "pending")], true);
    expect(keep).toEqual([
      { start: 0, end: 5 },
      { start: 8, end: 20 },
    ]);
  });
});

describe("silenceCuts", () => {
  it("pads plain silences and labels the trim", () => {
    const [c] = silenceCuts([{ start: 10, end: 15 }]);
    expect(c.start).toBeCloseTo(10.18);
    expect(c.end).toBeCloseTo(14.82);
    expect(c.label).toContain("4.6s");
    expect(c.rationale).not.toContain("picture");
  });

  it("splits around a mid-pause visual change", () => {
    const cuts = silenceCuts([{ start: 10, end: 15, visual: [12] }]);
    expect(cuts.length).toBe(2);
    expect(cuts[0].end).toBeCloseTo(11.8);
    expect(cuts[1].start).toBeCloseTo(12.2);
    expect(cuts[1].rationale).toContain("the picture changes");
  });

  it("keeps pauses shorter than 0.1s", () => {
    const [c] = silenceCuts([{ start: 5, end: 5.08 }]);
    expect(c).toBeTruthy();
    expect(c.end - c.start).toBeGreaterThan(0.02);
    expect(c.origin).toBe("silence");
  });

  it("auto-accepts silence trims", () => {
    expect(silenceCuts([{ start: 10, end: 15 }])[0].status).toBe("accepted");
  });
});

describe("manualCut", () => {
  it("creates an accepted manual cut with a readable label", () => {
    const c = manualCut(62, 70);
    expect(c.status).toBe("accepted");
    expect(c.origin).toBe("manual");
    expect(c.label).toContain("1:02.0");
    expect(c.label).toContain("1:10.0");
  });
});

describe("dedupeCuts", () => {
  it("filters new cuts that overlap queued ones but passes other types through", () => {
    const existing = [cut(10, 20)];
    const fresh = [cut(15, 25), cut(30, 35), { ...cut(12, 14), type: "pan" as const }];
    const kept = dedupeCuts(fresh, existing);
    expect(kept.map((c) => c.start)).toEqual([30, 12]);
  });
});

describe("sanitizeChanges", () => {
  it("drops degenerate cuts (seen in the wild as cut@1156.2-1156.2)", () => {
    const out = sanitizeChanges([cut(1156.2, 1156.2)], 1156.2);
    expect(out).toHaveLength(0);
  });

  it("keeps short non-cut edits but fixes inverted edges", () => {
    const flash: Change = { ...cut(5, 4), type: "flash" };
    const out = sanitizeChanges([flash], 100);
    expect(out).toHaveLength(1);
    expect(out[0].end).toBeGreaterThan(out[0].start);
  });

  it("clamps times into the video", () => {
    const out = sanitizeChanges([cut(-5, 999)], 60);
    expect(out[0].start).toBe(0);
    expect(out[0].end).toBe(60);
  });
});

describe("explicitTimeCuts", () => {
  it("cuts everything after an mm:ss time", () => {
    const [c] = explicitTimeCuts("trim everything after 17:07", 1100);
    expect(c.start).toBeCloseTo(17 * 60 + 7);
    expect(c.end).toBe(1100);
    expect(c.label).toContain("after 17:07");
  });

  it("cuts a named range between two times", () => {
    const [c] = explicitTimeCuts("cut 1:02 to 1:10", 300);
    expect(c.start).toBeCloseTo(62);
    expect(c.end).toBeCloseTo(70);
  });

  it("handles 'drop the first N seconds' with bare seconds", () => {
    const [c] = explicitTimeCuts("drop the first 30 seconds", 300);
    expect(c.start).toBe(0);
    expect(c.end).toBeCloseTo(30);
  });

  it("resolves timeline tags case-insensitively", () => {
    const tags: Tag[] = [
      { id: "t1", name: "A45", t: 45 },
      { id: "t2", name: "B60", t: 60 },
    ];
    const [c] = explicitTimeCuts("trim between @a45 and @b60", 120, tags);
    expect(c.start).toBeCloseTo(45);
    expect(c.end).toBeCloseTo(60);
  });

  it("ignores prompts without cut verbs or outside-duration times", () => {
    expect(explicitTimeCuts("add a title please", 300)).toEqual([]);
    expect(explicitTimeCuts("trim everything after 99:99", 60)).toEqual([]);
    expect(explicitTimeCuts("trim something", 0.1)).toEqual([]);
  });
});

describe("tags", () => {
  it("auto-names tags A45, B… skipping used letters", () => {
    expect(nextTagName([], 45)).toBe("A45");
    expect(nextTagName([{ id: "t", name: "A12", t: 12 }], 60)).toBe("B60");
  });

  it("lists unknown @mentions deduped and lowercased", () => {
    expect(unknownTagMentions("cut @b34 to @B34 then @x9", [{ id: "t", name: "A45", t: 1 }])).toEqual([
      "b34",
      "x9",
    ]);
  });
});
