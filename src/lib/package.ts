import type { Change, Transcript } from "../types";
import { invertCuts } from "./edits";

export type ThumbLayout = "center" | "bottom" | "left" | "vs" | "stamp";

export type PackIdea = {
  id: string;
  title: string;
  subtitle: string;
  font: string;
  layout: ThumbLayout;
  frame: number;
};

export const PACK_FONTS = [
  { name: "Bebas Neue", css: '"Bebas Neue", Impact, sans-serif' },
  { name: "Anton", css: '"Anton", Impact, sans-serif' },
  { name: "Fraunces", css: '"Fraunces", Georgia, serif' },
  { name: "Space Grotesk", css: '"Space Grotesk", sans-serif' },
  { name: "Outfit", css: '"Outfit", sans-serif' },
];

export function keptTranscript(t: Transcript, duration: number, changes: Change[]) {
  const keep = invertCuts(duration, changes);
  if (!t.words.length) return t.text;
  if (!keep.length) return t.words.map((w) => w.text).join(" ");
  return t.words
    .filter((w) => keep.some((k) => w.t >= k.start && w.t < k.end))
    .map((w) => w.text)
    .join(" ");
}

export function sampleKeepTimes(duration: number, changes: Change[], n = 10) {
  const keep = invertCuts(duration, changes);
  const spans = keep.length ? keep : [{ start: 0, end: duration }];
  const times: number[] = [];
  const total = spans.reduce((s, k) => s + (k.end - k.start), 0) || duration;
  for (let i = 0; i < n; i++) {
    let t = ((i + 0.5) / n) * total;
    for (const k of spans) {
      const len = k.end - k.start;
      if (t <= len) {
        times.push(k.start + t);
        break;
      }
      t -= len;
    }
  }
  return times;
}

export function suggestPackage(text: string, videoName: string): PackIdea[] {
  const clean = text.replace(/\s+/g, " ").trim();
  const bits = clean
    .split(/[.!?]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 12 && s.length < 140);
  const name = videoName.replace(/\.[^.]+$/, "").replace(/[_-]+/g, " ");
  const hook = bits[0] || name || "I built this";
  const mid = bits[Math.floor(bits.length / 2)] || hook;
  const last = bits[bits.length - 1] || hook;
  const short = (s: string, n = 42) => (s.length > n ? s.slice(0, n).replace(/\s+\S*$/, "") : s);
  const yell = (s: string) =>
    short(s, 28)
      .replace(/^(i |we |the |a )/i, "")
      .toUpperCase();

  const titles = [
    { title: short(hook, 58), subtitle: "WATCH THIS" },
    { title: `I tried ${short(name, 32)}`, subtitle: "HONEST TAKE" },
    { title: short(mid, 54), subtitle: "DON'T SKIP" },
    { title: `Don't use this until you see`, subtitle: yell(hook) },
    { title: `${short(name, 24)} vs the usual way`, subtitle: "VS" },
    { title: short(last, 56), subtitle: "THE ENDING" },
    { title: `Building in public: ${short(name, 28)}`, subtitle: "COME WITH ME" },
    { title: short(`Why ${hook}`, 56), subtitle: "THE REAL REASON" },
    { title: `${short(name, 20)} in one sitting`, subtitle: "FULL BUILD" },
    { title: short(hook, 50), subtitle: yell(mid || last) },
  ];

  const layouts: ThumbLayout[] = ["center", "bottom", "left", "vs", "stamp", "center", "bottom", "left", "stamp", "vs"];
  return titles.map((t, i) => ({
    id: `idea-${i + 1}`,
    title: t.title,
    subtitle: t.subtitle,
    font: PACK_FONTS[i % PACK_FONTS.length].css,
    layout: layouts[i],
    frame: i % 10,
  }));
}
