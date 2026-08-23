import { describe, expect, it } from "vitest";
import { suggestPackage } from "./package";

const TRANSCRIPT = [
  "Hi everybody, welcome back to the channel.",
  "Today I'm going to show you something cool.",
  "I built a coloring book site with DeepSeek and it only cost me $0.34.",
  "The wild part is it deployed to Vercel on the first try.",
  "So is it actually worth it?",
  "If you liked this, subscribe and leave a comment below.",
  "See you in the next one.",
].join(" ");

describe("suggestPackage", () => {
  const ideas = suggestPackage(TRANSCRIPT, "2026 08 19 17 44 48.mp4");

  it("returns ten unique-titled ideas", () => {
    expect(ideas).toHaveLength(10);
    expect(new Set(ideas.map((i) => i.title.toLowerCase())).size).toBe(10);
  });

  it("never uses greetings, setups or CTAs as titles", () => {
    for (const i of ideas) {
      expect(i.title).not.toMatch(/hi everybody|welcome back|going to show you|subscribe|comment below|see you/i);
    }
  });

  it("finds the product anchor and the cost hook", () => {
    const titles = ideas.map((i) => i.title).join("\n");
    expect(titles).toMatch(/DeepSeek/);
    expect(titles).toMatch(/\$0\.34/);
  });

  it("keeps thumbnail text to 2–6 words that don't repeat the title", () => {
    for (const i of ideas) {
      if (i.layout === "clean") {
        expect(i.subtitle).toBe("");
        continue;
      }
      if (i.layout === "vs") {
        expect(i.left!.split(/\s+/).length).toBeLessThanOrEqual(3);
        expect(i.right!.split(/\s+/).length).toBeLessThanOrEqual(3);
        continue;
      }
      const words = i.subtitle.split(/\s+/).filter(Boolean);
      expect(words.length).toBeGreaterThanOrEqual(1);
      expect(words.length).toBeLessThanOrEqual(6);
      const titleWords = new Set(i.title.toLowerCase().split(/[^a-z0-9$]+/));
      const dupes = words.filter((w) => titleWords.has(w.toLowerCase())).length;
      expect(dupes).toBeLessThanOrEqual(words.length / 2);
    }
  });

  it("includes a minimalist no-text card", () => {
    expect(ideas.some((i) => i.layout === "clean")).toBe(true);
  });

  it("ignores timestamp-only filenames", () => {
    const titles = ideas.map((i) => i.title).join("\n");
    expect(titles).not.toMatch(/2026 08 19/);
  });

  it("falls back gracefully on empty transcripts", () => {
    const out = suggestPackage("", "clip.mp4");
    expect(out).toHaveLength(10);
    for (const i of out) expect(i.title.length).toBeGreaterThan(0);
  });
});
