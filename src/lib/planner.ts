import type { Change, Transcript, VideoInfo } from "../types";
import { defaultPans, silenceCuts, titleOverlay, uid } from "./edits";
import { fillerCuts } from "./transcribe";
import type { Caption } from "./vision";

export function localPlan(
  prompt: string,
  video: VideoInfo,
  silences: Array<{ start: number; end: number }>,
  captions: Caption[] = [],
  transcript?: Transcript
) {
  const q = prompt.toLowerCase();
  const changes: Change[] = [];
  const wantsSilence = /silence|dead air|tight|pause|quiet|gap/.test(q);
  const wantsFillers = /um+|uh+|filler|stutter|remove where i say|cut the um/.test(q);
  // Word boundaries matter here — /pan/ used to hijack "panic shake" into
  // queuing camera pans (the exact regression class THOUGHTS.md warns about).
  const wantsPan = /\bpans?\b|\bpanning\b|ken burns|interesting|dynamic/.test(q);
  const wantsAll = /make it|edit (this|it)|do your thing|clean (it )?up|full pass|youtube/.test(q);
  const wantsTitle = /title|overlay|caption|text|thumbnail|hook|youtube/.test(q);
  const wantsSmash = /text ?card|smash|cut to text/.test(q);
  const visual = captions.find((c) => matchesAsk(q, c.text)) || captions[0];
  const at = visual ? clamp(visual.t, 0, Math.max(0, video.duration - 0.4)) : Math.min(1.2, video.duration * 0.2);

  if (wantsFillers && transcript?.words.length) {
    changes.push(
      ...fillerCuts(transcript.words).map((r) => ({
        id: uid("cut"),
        type: "cut" as const,
        start: r.start,
        end: r.end,
        status: "pending" as const,
        label: `Cut “${r.text}”`,
        rationale: "Filler from the transcript.",
      }))
    );
  }
  if (wantsSilence || wantsAll) changes.push(...silenceCuts(silences));
  // A smash-to-text request IS the title card — don't also drop a generic
  // opening overlay on top of it.
  if ((wantsTitle && !wantsSmash) || wantsAll) {
    const quoted = prompt.match(/["“](.+?)["”]/);
    const text = quoted?.[1] || hookFromCaption(visual?.text || "") || guessTitle(video.name) || "WATCH THIS";
    const start = visual && !quoted ? Math.max(0, visual.t - 0.15) : 0.2;
    changes.push(titleOverlay(text.toUpperCase(), start, start + 3.1));
  }
  if (wantsPan || wantsAll) {
    const pans = defaultPans(video.duration);
    if (visual) {
      pans.unshift({
        id: uid("pan"),
        type: "pan",
        start: Math.max(0, visual.t - 0.4),
        end: Math.min(video.duration, visual.t + 3.6),
        status: "pending",
        label: "Push in on what I see",
        rationale: visual.text,
        pan: { kind: "zoom-in" },
      });
    }
    changes.push(...pans);
  }

  if (/punch|snap zoom|emphasis/.test(q)) {
    changes.push(fx("punch", at, at + 0.4, "Zoom punch", "Hit the beat."));
  }
  if (/slow|slo-?mo|slow.?mo/.test(q)) {
    changes.push(fx("slow", at, Math.min(video.duration, at + 2.2), "Slow-mo", "Let it breathe.", 0.5));
  }
  if (/fast|speed ?up|rush|timelapse/.test(q)) {
    changes.push(fx("fast", at, Math.min(video.duration, at + 2.4), "Speed-up", "Skip the mush.", 1.8));
  }
  if (/shake|panic|earthquake|nervous|chaos/.test(q)) {
    changes.push(fx("shake", at, at + 0.85, "Panic shake", "Handheld spiral."));
  }
  if (/freeze|hold|wait for it|record scratch/.test(q)) {
    changes.push(fx("freeze", at, at + 0.9, "Freeze", "Wait for it."));
  }
  if (/flash|whoosh|white/.test(q)) {
    changes.push(fx("flash", at, at + 0.22, "Flash", "Cut punctuation."));
  }
  if (/text ?card|smash|cut to text/.test(q)) {
    const quoted = prompt.match(/["“](.+?)["”]/);
    changes.push({
      ...fx("textcard", at, at + 1.15, "Smash to text", "Hard cut to type."),
      text: (quoted?.[1] || hookFromCaption(visual?.text || "") || "NOPE").toUpperCase(),
    });
  }
  if (/tilt|dutch|uneasy|wonky/.test(q)) {
    changes.push(fx("tilt", at, Math.min(video.duration, at + 2.2), "Dutch tilt", "Something's off."));
  }
  if (/impact|boom|hit|meme/.test(q)) {
    changes.push({
      ...fx("impact", at, at + 0.7, "Impact", "Meme hammer."),
      text: (prompt.match(/["“](.+?)["”]/)?.[1] || "BOOM").toUpperCase(),
    });
  }
  if (/bounce|boing|playful/.test(q)) {
    changes.push(fx("bounce", at, at + 0.7, "Bounce", "Silly pop."));
  }
  if (/glitch|corrupt|digital/.test(q)) {
    changes.push(fx("glitch", at, at + 0.45, "Glitch", "Signal's dying."));
  }
  if (/spotlight|vignette|noir/.test(q)) {
    changes.push(fx("spotlight", at, Math.min(video.duration, at + 3), "Spotlight", "Hole of light."));
  }
  if (/zoom/.test(q) && !wantsPan && !/punch/.test(q)) {
    changes.push(fx("punch", at, at + 0.45, "Zoom punch", "Lean in."));
  }

  return changes;
}

function fx(
  type: Change["type"],
  start: number,
  end: number,
  label: string,
  rationale: string,
  rate?: number
): Change {
  return {
    id: uid(type.slice(0, 3)),
    type,
    start,
    end,
    status: "pending",
    label,
    rationale,
    rate,
  };
}

function clamp(n: number, min: number, max: number) {
  return Math.min(max, Math.max(min, n));
}

export function guessTitle(name: string) {
  const stem = name.replace(/\.[a-z0-9]+$/i, "").replace(/[_-]+/g, " ").trim();
  if (!stem || /^(video|untitled|img|dsc|mvi|clip)/i.test(stem)) return "";
  return stem.slice(0, 42).toUpperCase();
}

export function hookFromCaption(text: string) {
  if (!text) return "";
  return text
    .replace(/^(a |an |the )/i, "")
    .replace(/\.$/, "")
    .slice(0, 42)
    .toUpperCase();
}

export function describeScene(captions: Caption[]) {
  if (!captions.length) return "";
  return captions.map((c) => `${c.t.toFixed(1)}s: ${c.text}`).join(" · ");
}

export function describePlan(changes: Change[], scene = "") {
  if (!changes.length) {
    return scene
      ? `I can see: ${scene}. Try “panic shake”, “smash to text”, or “make it youtube”.`
      : "Nothing queued. Try “trim silences”, “panic shake”, or “make it youtube”.";
  }
  const counts = new Map<string, number>();
  for (const c of changes) counts.set(c.type, (counts.get(c.type) || 0) + 1);
  const bits = [...counts.entries()].map(([k, n]) => `${n} ${k}${n === 1 ? "" : "s"}`);
  const lead = `Queued ${bits.join(", ")}. Accept or reject each one.`;
  return scene ? `${lead}\n\nI see: ${scene}` : lead;
}

function matchesAsk(prompt: string, caption: string) {
  const words = prompt
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((w) => w.length > 3 && !STOP.has(w));
  const hay = caption.toLowerCase();
  return words.some((w) => hay.includes(w));
}

const STOP = new Set([
  "this",
  "that",
  "with",
  "from",
  "have",
  "make",
  "just",
  "when",
  "what",
  "title",
  "overlay",
  "please",
  "video",
  "zoom",
  "slow",
  "shake",
]);
