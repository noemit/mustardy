import { chat } from "./bridge";
import {
  SYSTEM_PROMPT,
  dedupeCuts,
  explicitTimeCuts,
  normalizeDraft,
  parseAgentJson,
  sanitizeChanges,
  silenceCuts,
  unknownTagMentions,
} from "./edits";
import { planWithGemma } from "./gemma";
import { describePlan, describeScene, localPlan } from "./planner";
import { formatTranscript, phraseTimeCuts } from "./transcribe";
import type { Caption } from "./vision";
import type { Change, Settings, SilenceRange, Tag, Transcript, VideoInfo } from "../types";

export type PlanRequest = {
  prompt: string;
  video: VideoInfo;
  /** Detected silences from the audio scan (may carry visual-change splits). */
  silences: SilenceRange[];
  captions: Caption[];
  transcript: Transcript;
  changes: Change[];
  tags: Tag[];
  settings: Settings;
  /** Lazily transcribes when the request needs words (called at most once). */
  ensureTranscript: () => Promise<Transcript>;
};

/**
 * The full planning pipeline behind a chat request: deterministic cuts first
 * (named times, @tags, transcript phrases, named-silence trims), then the
 * keyword planner, then the local brain — with remote providers swapped in
 * per settings and every failure falling back to the rule-based editor.
 * Pure-ish: no state writes, no logging; callers own the UI.
 */
export async function planEdits(req: PlanRequest): Promise<{ changes: Change[]; reply: string }> {
  const { prompt: text, video, silences, captions, transcript, changes, tags, settings } = req;

  const context = [
    `Video: ${video.name}, duration ${video.duration.toFixed(2)}s, ${video.width}x${video.height}.`,
    silences.length
      ? `Detected silences: ${JSON.stringify(silences.map((s) => [Number(s.start.toFixed(2)), Number(s.end.toFixed(2))]))}`
      : "No silence ranges detected (or FFmpeg not available in this session).",
    changes.length
      ? `Existing changes: ${JSON.stringify(changes.map((c) => ({ type: c.type, start: c.start, end: c.end, status: c.status, label: c.label })))}`
      : "No edits yet.",
  ].join("\n");

  let reply = "";
  let incoming: Change[] = [];
  const seen = captions;

  // Deterministic cuts first — user-named times ("trim everything after
  // 17:07", "cut between @a45 and @b34") and transcript phrases ("trim
  // after I say thanks") never go through a model. Silence trims are
  // mechanical too: built straight from the audio scan, but only when the
  // user actually names silences (a bare "trim …" is not a silence request).
  const needsWords =
    /\b(after|before)\s+(?:i|we)\s+say\b|\bfiller\b|\bum+\b|\bstutter\b|\btranscrib/i.test(text);
  const liveTranscript = needsWords ? await req.ensureTranscript() : transcript;
  const explicitCuts = explicitTimeCuts(text, video.duration, tags);
  const unknownTags = unknownTagMentions(text, tags);
  const phraseResult = phraseTimeCuts(text, liveTranscript, video.duration);
  const wantsSilence = /silence|dead air|tight|pause|quiet|gap/.test(text.toLowerCase());
  const silencePreCuts = dedupeCuts(wantsSilence ? silenceCuts(silences) : [], changes);
  const preCuts = [...explicitCuts, ...phraseResult.cuts, ...silencePreCuts];
  const plannerSilences = preCuts.length ? [] : silences; // planners add their own otherwise
  const hushNote =
    wantsSilence && silences.length
      ? "\nSilence trims are already queued as cuts — do NOT emit cuts for silences."
      : "";

  try {
    if (settings.provider === "kimi" || settings.provider === "ollama") {
      const result = await chat({
        provider: settings.provider,
        settings,
        system: SYSTEM_PROMPT,
        images: [],
        messages: [
          {
            role: "user",
            content: `${context}${hushNote}\n\nTranscript: ${formatTranscript(liveTranscript)}\n\nUser: ${text}`,
          },
        ],
      });
      const parsed = parseAgentJson(result.text);
      reply = parsed.message;
      incoming = normalizeDraft(parsed.draft, video.duration);
    } else {
      // Built-in vocabulary first — most edits come from the transcript,
      // the audio scan, and what the user literally asked. The local
      // brain only spins up when nothing matched (loads on demand).
      incoming = localPlan(text, video, plannerSilences, seen, liveTranscript);
      if (!incoming.length && !preCuts.length) {
        const planned = await planWithGemma({
          prompt: text,
          video,
          silences,
          captions: seen,
          transcript: formatTranscript(liveTranscript),
          existing: changes,
          silencesHandled: wantsSilence && silences.length > 0,
        });
        incoming = planned.changes;
      }
      reply = describePlan(incoming, describeScene(seen));
    }
  } catch (err) {
    incoming = localPlan(text, video, plannerSilences, seen, liveTranscript);
    reply = `${describePlan(incoming, describeScene(seen))}\n\n(${err instanceof Error ? err.message : "model unavailable"} — used the built-in editor.)`;
  }

  if (!incoming.length && !preCuts.length) {
    const fallback = localPlan(text, video, plannerSilences, seen, liveTranscript);
    if (fallback.length) {
      incoming = fallback;
      if (!reply) reply = describePlan(fallback);
    }
  }

  // Drop planner cuts that redo a range we already queued deterministically.
  if (preCuts.length) {
    incoming = incoming.filter(
      (c) => c.type !== "cut" || !preCuts.some((p) => c.start < p.end && c.end > p.start)
    );
  }
  const plannerChanges = incoming;
  incoming = sanitizeChanges([...preCuts, ...plannerChanges], video.duration).sort(
    (a, b) => a.start - b.start
  );

  // Report deterministic cuts in the app's own words, and keep whatever
  // the planner said about its own changes instead of burying it.
  if (preCuts.length || wantsSilence || phraseResult.phrase || unknownTags.length) {
    const notes: string[] = [];
    for (const c of [...explicitCuts, ...phraseResult.cuts]) notes.push(`Queued: ${c.label}.`);
    if (unknownTags.length) {
      notes.push(
        `No tag ${unknownTags.map((n) => `@${n}`).join(", ")} — press T to tag the playhead first.`
      );
    }
    if (phraseResult.phrase && !phraseResult.cuts.length) {
      notes.push(
        liveTranscript.words.length
          ? `I couldn't find “${phraseResult.phrase}” in the transcript.`
          : `Can't look for “${phraseResult.phrase}” — no transcript.`
      );
    }
    if (silencePreCuts.length) {
      notes.push(
        `Queued ${silencePreCuts.length} silence trim${silencePreCuts.length === 1 ? "" : "s"} from the audio scan.`
      );
    } else if (wantsSilence) {
      notes.push(
        silences.length ? "Those silences are already queued." : "No quiet stretches on file — nothing to trim."
      );
    }
    const rest = plannerChanges.length
      ? reply || describePlan(plannerChanges, describeScene(seen))
      : "";
    reply = [notes.join(" "), rest].filter(Boolean).join("\n\n");
  }

  return { changes: incoming, reply };
}
