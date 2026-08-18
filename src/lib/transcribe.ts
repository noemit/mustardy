import type { Change, Transcript, TranscriptWord, VideoInfo } from "../types";
import { formatTime, uid } from "./edits";
import { native, transcribeNative } from "./bridge";

const FILLER = /^(um+|uh+|er+|uh-huh|ah+|like|you know)$/i;

export async function transcribeWhisper(video: VideoInfo, model?: string): Promise<Transcript> {
  if (!native || !video.path.startsWith("/")) return { text: "", words: [], source: "none" };
  const result = await transcribeNative(video.path, model);
  const words: TranscriptWord[] = result.words
    .map((w) => ({ t: Number(w.t || 0), end: Number(w.end || w.t || 0), text: String(w.text || "").trim() }))
    .filter((w) => w.text);
  return {
    text: String(result.text || words.map((w) => w.text).join(" ")).trim(),
    words,
    source: "whisper",
  };
}

/** Cut covering words[from]…words[to] (inclusive). */
export function wordRangeCut(words: TranscriptWord[], from: number, to: number): Change | null {
  const a = Math.max(0, Math.min(from, to));
  const b = Math.min(words.length - 1, Math.max(from, to));
  if (!words[a] || !words[b]) return null;
  const start = Math.max(0, words[a].t - 0.03);
  const end = Math.max(start + 0.08, words[b].end + 0.03);
  const label =
    a === b
      ? `Cut “${words[a].text}”`
      : `Cut “${words[a].text}”…“${words[b].text}”`;
  return {
    id: uid("cut"),
    type: "cut",
    start,
    end,
    status: "pending",
    label,
    rationale: "Removed from the transcript.",
  };
}

export function fillerCuts(words: TranscriptWord[]) {
  return words
    .filter((w) => FILLER.test(w.text.replace(/[.,!?]/g, "")))
    .filter((w) => w.end - w.t >= 0.08 || w.end === w.t)
    .map((w) => ({
      start: Math.max(0, w.t - 0.04),
      end: Math.max(w.t + 0.12, w.end + 0.04),
      text: w.text,
    }));
}

export function transcriptToTxt(t: Transcript) {
  if (t.words.length) {
    return t.words
      .map((w) => `${formatTime(w.t)}  ${w.text}`)
      .join("\n");
  }
  return t.text;
}

export function transcriptToSrt(t: Transcript) {
  if (!t.words.length) {
    if (!t.text) return "";
    return `1\n00:00:00,000 --> 00:00:02,000\n${t.text}\n`;
  }
  const cues: Array<{ start: number; end: number; text: string }> = [];
  let buf: typeof t.words = [];
  for (const w of t.words) {
    buf.push(w);
    const span = w.end - buf[0].t;
    if (buf.length >= 10 || span >= 4) {
      cues.push({
        start: buf[0].t,
        end: Math.max(buf[buf.length - 1].end, buf[0].t + 0.4),
        text: buf.map((x) => x.text).join(" "),
      });
      buf = [];
    }
  }
  if (buf.length) {
    cues.push({
      start: buf[0].t,
      end: Math.max(buf[buf.length - 1].end, buf[0].t + 0.4),
      text: buf.map((x) => x.text).join(" "),
    });
  }
  return cues
    .map((c, i) => `${i + 1}\n${srtStamp(c.start)} --> ${srtStamp(c.end)}\n${c.text}\n`)
    .join("\n");
}

function srtStamp(t: number) {
  const ms = Math.max(0, Math.round(t * 1000));
  const h = Math.floor(ms / 3_600_000);
  const m = Math.floor((ms % 3_600_000) / 60_000);
  const s = Math.floor((ms % 60_000) / 1000);
  const f = ms % 1000;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")},${String(f).padStart(3, "0")}`;
}

export function formatTranscript(t: Transcript, limit = 1600) {
  if (!t.text) return "";
  if (t.text.length <= limit) return t.text;
  return `${t.text.slice(0, limit)}…`;
}

/** Deterministic "trim after/before I say X (the first time)" — locate the
 * phrase in the whisper words and cut the tail/head there. No model, no
 * guessing. Returns the parsed phrase even when nothing matches so the UI
 * can say "couldn't find it" instead of staying silent. */
export function phraseTimeCuts(
  prompt: string,
  transcript: Transcript,
  duration: number
): { cuts: Change[]; phrase: string | null } {
  const q = prompt.toLowerCase();
  if (!/trim|cut|remove|delete|drop|shorten|chop/.test(q)) return { cuts: [], phrase: null };
  const m = q.match(
    /\b(after|before)\s+(?:the\s+(first|second|last)\s+time\s+)?(?:i|we)\s+say\s+["“'‘]?(.+?)["”'’]?\s*$/
  );
  if (!m) return { cuts: [], phrase: null };
  const after = m[1] === "after";
  let occurrence = m[2] || "first";
  let phrase = m[3].trim();
  // "…thanks for watching the first time" — occurrence may trail the phrase.
  const trailing = phrase.match(/\s+(?:the\s+)?(first|second|last)\s+time$/);
  if (trailing) {
    occurrence = trailing[1];
    phrase = phrase.slice(0, trailing.index).trim();
  }
  const needle = phrase.split(/[^a-z0-9']+/).filter(Boolean);
  if (!needle.length) return { cuts: [], phrase: null };

  const hay = transcript.words.map((w) => w.text.toLowerCase().replace(/[^a-z0-9']/g, ""));
  const hits: number[] = [];
  for (let i = 0; i + needle.length <= hay.length; i++) {
    if (needle.every((w, j) => hay[i + j] === w)) hits.push(i);
  }
  if (!hits.length) return { cuts: [], phrase };
  const hit =
    occurrence === "last" ? hits[hits.length - 1] : occurrence === "second" ? (hits[1] ?? hits[0]) : hits[0];
  const first = transcript.words[hit];
  const lastWord = transcript.words[hit + needle.length - 1];
  const start = after ? Math.max(lastWord.end, lastWord.t) : 0;
  const end = after ? duration : Math.max(0, first.t - 0.05);
  if (end - start < 0.1) return { cuts: [], phrase };

  return {
    cuts: [
      {
        id: uid("cut"),
        type: "cut",
        start,
        end,
        status: "pending",
        label: after ? `Cut everything after “${phrase}”` : `Cut everything before “${phrase}”`,
        rationale: `Phrase ${after ? "ends" : "starts"} at ${formatTime(after ? start : end)} — found in the transcript.`,
      },
    ],
    phrase,
  };
}
