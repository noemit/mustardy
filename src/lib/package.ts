import type { Change, Transcript } from "../types";
import { invertCuts } from "./edits";

export type ThumbLayout = "center" | "bottom" | "left" | "vs" | "stamp" | "clean";

export type PackIdea = {
  id: string;
  /** The YouTube title — never rendered on the thumbnail itself. */
  title: string;
  /** Thumbnail text: 2–6 words, readable at 160px, complements the title. */
  subtitle: string;
  font: string;
  layout: ThumbLayout;
  frame: number;
  /** Short labels for the VS split (falls back to subtitle when absent). */
  left?: string;
  right?: string;
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

/**
 * Title + thumbnail suggestions modeled on the channel's publishing skill
 * (docs/youtube-PUBLISHING.md): titles in proven styles (known-product
 * anchors, cost hooks, first impressions, direct roasts, meta loops), and
 * thumbnail text that is 2–6 words, readable at feed size, and complements
 * — never repeats — the title. Deterministic: junk sentences (greetings,
 * CTAs, housekeeping) are dropped before anything becomes a hook.
 */
export function suggestPackage(text: string, videoName: string): PackIdea[] {
  const bits = sentences(text);
  const stem = cleanStem(videoName);
  const prod = findProduct(text, videoName);
  const money = findMoney(text);
  const question = bits.find((s) => s.endsWith("?")) || null;

  const scored = bits
    .filter((s) => !s.endsWith("?"))
    .map((s) => ({ s, score: hookScore(s, prod) }))
    .sort((a, b) => b.score - a.score);
  // hook / mid / last must be three different sentences.
  const picked: string[] = [];
  const pick = (idx: number) => {
    for (let i = Math.max(0, Math.min(idx, scored.length - 1)); i < scored.length; i++) {
      if (!picked.includes(scored[i].s)) {
        picked.push(scored[i].s);
        return scored[i].s;
      }
    }
    return "";
  };
  const hook = pick(0) || stem || "I built this";
  const mid = pick(2) || hook;
  const last = pick(scored.length - 1) || mid;

  const thing = prod || stem || "this";
  const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  const vsShort = (prod || stem || "NEW").toUpperCase().slice(0, 14);

  // title / thumbnail-text pairs. Thumb copy is 2–6 words, ALL CAPS, and
  // checked against the title — if half its words already appear there it
  // is swapped for a generic stamp (the skill: complement, never repeat).
  const ideas: Array<{ title: string; sub: string; layout: ThumbLayout; left?: string; right?: string }> = [
    { title: cap(short(hook, 58)), sub: "IT ACTUALLY WORKS", layout: "center" },
    { title: `I tried ${short(thing, 32)} so you don't have to`, sub: "HONEST TAKE", layout: "left" },
    money
      ? { title: `I didn't expect this to cost ${money}`, sub: "WORTH IT?", layout: "stamp" }
      : { title: `${cap(short(thing, 34))} just changed how I work`, sub: "GAME CHANGER", layout: "stamp" },
    { title: `${cap(short(thing, 40))}: first impressions`, sub: "FIRST LOOK", layout: "bottom" },
    {
      title: `${cap(short(thing, 26))} vs the usual way`,
      sub: "",
      layout: "vs",
      left: "OLD WAY",
      right: vsShort === "OLD WAY" ? "NEW WAY" : vsShort,
    },
    { title: "I edited this with the tool I'm building", sub: "SO META", layout: "center" },
    { title: cap(short(mid, 54)), sub: "THE GOOD PART", layout: "stamp" },
    question
      ? { title: cap(short(question.replace(/\?+$/, ""), 54)), sub: "THE ANSWER", layout: "center" }
      : { title: `${cap(short(thing, 36))} is not what I expected`, sub: "BRUTALLY HONEST", layout: "left" },
    { title: cap(short(last, 56)), sub: "THE ENDING", layout: "bottom" },
    { title: `${cap(short(thing, 30))} in one sitting`, sub: "", layout: "clean" },
  ];

  // Titles must be unique — a duplicate reads as a bug in the list.
  const seen = new Set<string>();
  const out = ideas.map((it, i) => {
    let title = it.title.trim();
    while (seen.has(title.toLowerCase())) title = `${title} (${i + 1})`;
    seen.add(title.toLowerCase());
    const sub =
      it.layout === "vs"
        ? ""
        : repeats(title, it.sub)
          ? FALLBACK_STAMPS[i % FALLBACK_STAMPS.length]
          : it.sub;
    return {
      id: `idea-${i + 1}`,
      title,
      subtitle: sub,
      font: PACK_FONTS[i % PACK_FONTS.length].css,
      layout: it.layout,
      left: it.left,
      right: it.right,
      frame: i % 10,
    };
  });
  return out;
}

/** Truncate so titles never end mid-word or mid-thought: prefer breaking
 *  at a comma or connective ("and", "but", …) over a hard character cut. */
function short(s: string, n = 42) {
  const t = s.trim();
  if (t.length <= n) return t.replace(/[.,;:!?]+$/, "");
  const slice = t.slice(0, n);
  const clause = slice.match(/^(.*)(?:,|\s+and\s|\s+but\s|\s+so\s|\s+which\s|\s+that\s|\s+it\s)[^,]*$/);
  const cut = clause && clause[1].length >= n * 0.5 ? clause[1] : slice.replace(/[\s.,:;!?-]+\S*$/, "");
  return cut.replace(/[.,;:!?\s]+$/, "");
}

const FALLBACK_STAMPS = ["WATCH THIS", "DON'T SKIP", "NO WAY", "WAIT FOR IT", "REAL TALK"];

/** Split into candidate sentences and drop the ones that can never be a
 *  hook: greetings, sign-offs, CTAs, and "today I will show you" setups.
 *  Leading discourse markers ("So, …") are stripped first — "So is it
 *  actually worth it?" is a hook; the "So" is not. */
function sentences(text: string): string[] {
  return text
    .replace(/\s+/g, " ")
    .trim()
    .split(/(?<=[.!?])\s+/)
    .map((s) => s.trim().replace(/^(so|okay|ok|alright|right|well|and|but|now)[,\s]+/i, ""))
    .filter((s) => {
      if (s.length < 14 || s.length > 160) return false;
      return !JUNK.test(s);
    });
}

const JUNK = new RegExp(
  [
    "^(hi|hello|hey|yo|okay|ok|so|alright|right)\\b",
    "welcome (back|to)",
    "going to (show|tell) you",
    "let'?s (get|jump) (started|into)",
    "(like|subscribe|hit the bell|comment) (and|below|if)\\b",
    "see you (in the next|later|soon)",
    "^(um+|uh+|hmm)\\b",
  ].join("|"),
  "i"
);

/** Filename stems like "2026 08 19 17 44 48" are timestamps, not topics. */
function cleanStem(name: string): string {
  const stem = name.replace(/\.[a-z0-9]+$/i, "").replace(/[_-]+/g, " ").trim();
  if (!stem || /^(video|untitled|img|dsc|mvi|clip|screen|recording)/i.test(stem)) return "";
  if (/^[\d\s:.-]+$/.test(stem)) return "";
  return stem;
}

const KNOWN_TOOLS = [
  "openchamber", "deepseek", "chatgpt", "openai", "claude", "gemini", "grok",
  "cursor", "copilot", "windsurf", "ollama", "llama", "qwen", "kimi", "sora",
  "runway", "midjourney", "whisper", "ffmpeg", "docker", "vercel", "tauri",
  "vite", "rust", "react", "python", "mustardy", "cuberry",
];

/** The product anchor: a known tool named in the transcript/filename, else
 *  a TitleCase phrase that repeats in the transcript (likely the subject). */
function findProduct(text: string, videoName: string): string {
  const hay = `${videoName} ${text}`.toLowerCase();
  const hits = KNOWN_TOOLS.filter((t) => new RegExp(`\\b${t}\\b`, "i").test(hay));
  if (hits.length) {
    // Prefer the most-mentioned tool — that is what the video is about.
    hits.sort(
      (a, b) =>
        (hay.match(new RegExp(`\\b${b}\\b`, "g"))?.length || 0) -
        (hay.match(new RegExp(`\\b${a}\\b`, "g"))?.length || 0)
    );
    const t = hits[0];
    return t.charAt(0).toUpperCase() + t.slice(1);
  }
  const names = new Map<string, number>();
  for (const m of text.matchAll(/\b([A-Z][a-z0-9]+(?:\s+[A-Z][a-z0-9]+){0,2})\b/g)) {
    const n = m[1];
    if (!/^(I|We|The|This|That|It|So|And|But|You|My)$/.test(n)) names.set(n, (names.get(n) || 0) + 1);
  }
  const best = [...names.entries()].filter(([, n]) => n > 1).sort((a, b) => b[1] - a[1])[0];
  return best ? best[0] : "";
}

function findMoney(text: string): string {
  const m =
    text.match(/\$\s?\d[\d.,]*\s?[kKmM]?\b/) ||
    text.match(/\b\d[\d.,]*\s?(dollars|cents|usd)\b/i) ||
    text.match(/\bfree\b/i);
  return m ? m[0].trim() : "";
}

/** Hooks earn points for numbers, money, the product anchor, and strong
 *  first-person verbs; long meandering sentences lose them. */
function hookScore(s: string, prod: string): number {
  let score = 0;
  if (/\d/.test(s)) score += 2;
  if (/\$|free|cost|paid|cheap/.test(s.toLowerCase())) score += 2;
  if (prod && s.toLowerCase().includes(prod.toLowerCase())) score += 2;
  if (/^(i|we) (built|made|tried|shipped|tested|broke|fixed|asked)/i.test(s)) score += 3;
  if (/\b(never|actually|finally|worst|best|insane|weird)\b/i.test(s)) score += 1;
  if (s.length >= 25 && s.length <= 90) score += 2;
  if (s.length > 120) score -= 2;
  return score;
}

/** True when more than half the thumbnail words already appear in the
 *  title verbatim — the skill's "complement, never repeat" rule. */
function repeats(title: string, sub: string): boolean {
  if (!sub) return false;
  const t = new Set(title.toLowerCase().split(/[^a-z0-9$]+/));
  const words = sub.toLowerCase().split(/[^a-z0-9$]+/).filter(Boolean);
  const dupes = words.filter((w) => t.has(w)).length;
  return dupes > words.length / 2;
}
