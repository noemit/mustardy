import { describe, expect, it } from "vitest";
import { fillerCuts, phraseTimeCuts, transcriptToSrt, wordRangeCut } from "./transcribe";
import type { Transcript, TranscriptWord } from "../types";

const words = (list: Array<[number, number, string]>): TranscriptWord[] =>
  list.map(([t, end, text]) => ({ t, end, text }));

const empty: Transcript = { text: "", words: [], source: "none" };

describe("wordRangeCut", () => {
  const ws = words([
    [10, 10.5, "hello"],
    [11, 11.6, "big"],
    [12.2, 13, "world"],
  ]);

  it("cuts an inclusive range with a small pad", () => {
    const c = wordRangeCut(ws, 0, 1);
    expect(c?.start).toBeCloseTo(9.97);
    expect(c?.end).toBeCloseTo(11.63);
    expect(c?.label).toContain("hello");
    expect(c?.label).toContain("big");
  });

  it("is order-independent and null-safe outside the list", () => {
    expect(wordRangeCut(ws, 1, 0)?.start).toBeCloseTo(9.97);
    expect(wordRangeCut(ws, 5, 9)).toBeNull();
  });
});

describe("fillerCuts", () => {
  it("matches um/uh/like but not lookalikes", () => {
    const cuts = fillerCuts(
      words([
        [1, 1.3, "um,"],
        [2, 2.4, "umbrella"],
        [3, 3.2, "like"],
      ])
    );
    expect(cuts.map((c) => c.text)).toEqual(["um,", "like"]);
  });

  it("catches 'you know' when whisper emits it as one word", () => {
    const cuts = fillerCuts(
      words([
        [4, 4.5, "you"],
        [4.6, 5.2, "know,"],
        [6, 6.8, "you know"],
      ])
    );
    expect(cuts.map((c) => c.text)).toEqual(["you know"]);
  });
});

describe("phraseTimeCuts", () => {
  const t: Transcript = {
    text: "hi there thanks for watching thanks for watching again bye",
    words: words([
      [0, 0.4, "hi"],
      [0.5, 1.0, "there"],
      [1.1, 2.0, "thanks"],
      [2.1, 2.4, "for"],
      [2.5, 3.4, "watching"],
      [10.0, 10.9, "thanks"],
      [11.0, 11.3, "for"],
      [11.4, 12.5, "watching"],
      [13, 13.6, "again"],
      [14, 14.5, "bye"],
    ]),
    source: "whisper",
  };

  it("cuts everything after the first occurrence", () => {
    const r = phraseTimeCuts("trim everything after I say thanks for watching", t, 100);
    expect(r.phrase).toBe("thanks for watching");
    expect(r.cuts[0].start).toBeCloseTo(3.4);
    expect(r.cuts[0].end).toBe(100);
  });

  it("honors 'the second time' and trailing occurrence wording", () => {
    const r = phraseTimeCuts(
      "cut everything after I say thanks for watching the second time",
      t,
      100
    );
    expect(r.cuts[0].start).toBeCloseTo(12.5);
  });

  it("cuts before a phrase when asked", () => {
    const r = phraseTimeCuts("trim everything before I say bye", t, 100);
    expect(r.cuts[0].start).toBe(0);
    expect(r.cuts[0].end).toBeCloseTo(14 - 0.05);
  });

  it("reports a missing phrase instead of guessing", () => {
    const r = phraseTimeCuts("trim after I say lorem ipsum dolor", t, 100);
    expect(r.cuts).toHaveLength(0);
    expect(r.phrase).toBe("lorem ipsum dolor");
    expect(phraseTimeCuts("add a title", empty, 50).cuts).toHaveLength(0);
  });
});

describe("transcriptToSrt", () => {
  it("groups words into cues and stamps SRT timecodes", () => {
    const srt = transcriptToSrt({
      text: "",
      source: "whisper",
      words: words([
        [1.25, 1.5, "a"],
        [1.6, 1.9, "b"],
      ]),
    });
    expect(srt).toContain("00:00:01,250 --> ");
    expect(srt.trim().split("\n\n")[0]).toBe("1\n00:00:01,250 --> 00:00:01,900\na b");
  });

  it("falls back to one cue from raw text", () => {
    expect(transcriptToSrt({ ...empty, text: "just words" })).toContain("just words");
    expect(transcriptToSrt(empty)).toBe("");
  });
});
