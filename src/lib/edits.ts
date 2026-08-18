import type { Change, OverlayStyle, PanKind, SilenceRange, Tag } from "../types";
import { isChangeType } from "./tools";

export function uid(prefix = "c") {
  return `${prefix}-${Math.random().toString(36).slice(2, 9)}`;
}

export function formatTime(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) seconds = 0;
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const f = Math.floor((seconds % 1) * 10);
  return `${m}:${String(s).padStart(2, "0")}.${f}`;
}

export function invertCuts(duration: number, cuts: Change[]) {
  const sorted = [...cuts]
    .filter((c) => c.type === "cut" && c.status === "accepted")
    .map((c) => ({ start: Math.max(0, c.start), end: Math.min(duration, c.end) }))
    .filter((c) => c.end > c.start)
    .sort((a, b) => a.start - b.start);

  const merged: Array<{ start: number; end: number }> = [];
  for (const c of sorted) {
    const last = merged[merged.length - 1];
    if (last && c.start <= last.end + 0.02) last.end = Math.max(last.end, c.end);
    else merged.push({ ...c });
  }

  const keep: Array<{ start: number; end: number }> = [];
  let cursor = 0;
  for (const c of merged) {
    if (c.start > cursor + 0.01) keep.push({ start: cursor, end: c.start });
    cursor = Math.max(cursor, c.end);
  }
  if (cursor < duration - 0.01) keep.push({ start: cursor, end: duration });
  return keep;
}

export function sourceToEdited(sourceTime: number, duration: number, changes: Change[]) {
  const cuts = changes.filter((c) => c.type === "cut" && c.status === "accepted");
  let t = sourceTime;
  for (const c of cuts) {
    if (sourceTime >= c.end) t -= c.end - c.start;
    else if (sourceTime >= c.start && sourceTime < c.end) t -= sourceTime - c.start;
  }
  void duration;
  return Math.max(0, t);
}

export function editedToSource(editedTime: number, duration: number, changes: Change[]) {
  const keep = invertCuts(duration, changes);
  let remaining = editedTime;
  for (const seg of keep) {
    const len = seg.end - seg.start;
    if (remaining <= len) return seg.start + remaining;
    remaining -= len;
  }
  return duration;
}

export function editedDuration(duration: number, changes: Change[]) {
  return invertCuts(duration, changes).reduce((n, s) => n + (s.end - s.start), 0);
}

export function skippableCuts(changes: Change[], previewPending: boolean) {
  return changes.filter(
    (c) =>
      c.type === "cut" &&
      (c.status === "accepted" || (previewPending && c.status === "pending"))
  );
}

export function isInAcceptedCut(t: number, changes: Change[], previewPending = false) {
  return skippableCuts(changes, previewPending).some((c) => t >= c.start && t < c.end);
}

export function activeAt(t: number, changes: Change[], type?: Change["type"]) {
  return changes.filter(
    (c) =>
      c.status !== "rejected" &&
      t >= c.start &&
      t < c.end &&
      (!type || c.type === type)
  );
}

/** Build silence trims. When a pause hides a visual change (scene cut, slide
 * switch), the trim splits around it: the static sides go, the transition
 * stays — cutting through it is what made silence trims feel jumpy. */
export function silenceCuts(
  ranges: SilenceRange[],
  pad = 0.18,
  transitionMargin = 0.2
): Change[] {
  const out: Change[] = [];
  for (const r of ranges) {
    const splits = (r.visual || [])
      .filter((t) => t > r.start + pad && t < r.end - pad)
      .sort((a, b) => a - b);
    const bounds = [r.start, ...splits, r.end];
    for (let i = 0; i + 1 < bounds.length; i++) {
      const start = i === 0 ? bounds[i] + pad : bounds[i] + transitionMargin;
      const end = i + 1 === bounds.length - 1 ? bounds[i + 1] - pad : bounds[i + 1] - transitionMargin;
      const dur = end - start;
      if (dur < 0.4) continue;
      out.push({
        id: uid("cut"),
        type: "cut" as const,
        start,
        end,
        status: "pending" as const,
        label: splits.length
          ? `Trim ${dur.toFixed(1)}s silence — kept the visual change at ${splits.map(formatTime).join(", ")}`
          : `Trim ${dur.toFixed(1)}s silence`,
        rationale: splits.length
          ? "Dead air, but the picture changes mid-pause — that moment stays."
          : "Dead air — keep a short breath on either side.",
      });
    }
  }
  return out;
}

/** Drop cuts that overlap an already-queued (non-rejected) cut, so asking
 * twice doesn't stack duplicates. Only cut-vs-cut; other types pass through. */
export function dedupeCuts(cuts: Change[], existing: Change[]): Change[] {
  const queued = existing.filter((c) => c.type === "cut" && c.status !== "rejected");
  if (!queued.length) return cuts;
  return cuts.filter(
    (c) => c.type !== "cut" || !queued.some((q) => c.start < q.end && c.end > q.start)
  );
}

/** Parse a user-named time range into cut changes — deterministic, no model
 * involved. Handles "trim everything after 17:07", "cut 1:02 to 1:10",
 * "drop the first 30 seconds", "cut from 5:00 to the end", and tag mentions:
 * "trim between @a45 and @b34". Times are mm:ss, bare seconds ("90s"),
 * or timeline tags. */
export function explicitTimeCuts(prompt: string, duration: number, tags: Tag[] = []): Change[] {
  const q = prompt.toLowerCase();
  if (!/trim|cut|remove|delete|drop|shorten|chop/.test(q)) return [];
  if (!(duration > 0.2)) return [];

  const times: Array<{ t: number; at: number }> = [];
  for (const m of q.matchAll(/(\d{1,3}):([0-5]?\d)(?:[.,](\d))?/g)) {
    times.push({
      t: Number(m[1]) * 60 + Number(m[2]) + (m[3] ? Number(`0.${m[3]}`) : 0),
      at: m.index ?? 0,
    });
  }
  for (const m of q.matchAll(/(\d+(?:\.\d+)?)\s*(?:s|sec|secs|second|seconds)\b/g)) {
    times.push({ t: Number(m[1]), at: m.index ?? 0 });
  }
  for (const m of q.matchAll(/@([a-z]{1,3}\d{1,5})\b/g)) {
    const tag = tags.find((tg) => tg.name.toLowerCase() === m[1]);
    if (tag) times.push({ t: tag.t, at: m.index ?? 0 });
  }
  if (!times.length) return [];
  times.sort((a, b) => a.at - b.at);

  let start: number | null = null;
  let end: number | null = null;
  if (times.length >= 2) {
    start = Math.min(times[0].t, times[1].t);
    end = Math.max(times[0].t, times[1].t);
  } else if (/\bafter\b|\bpast\b|\bbeyond\b|onwards?|\bfrom\b|to (the )?end\b/.test(q)) {
    start = times[0].t;
    end = duration;
  } else if (/\bbefore\b|\buntil\b|up to|\bfirst\b/.test(q)) {
    start = 0;
    end = times[0].t;
  }
  if (start == null || end == null) return [];
  if (start >= duration - 0.1 || end <= 0.05) return []; // named time is outside the video

  start = Math.max(0, start);
  end = Math.min(end, duration);
  if (end - start < 0.1) return [];

  const label =
    start <= 0.05
      ? `Cut the first ${formatTime(end)}`
      : end >= duration - 0.05
        ? `Cut everything after ${formatTime(start)}`
        : `Cut ${formatTime(start)}–${formatTime(end)}`;
  return [
    {
      id: uid("cut"),
      type: "cut",
      start,
      end,
      status: "pending",
      label,
      rationale: "You named the time — queued directly, no model guesswork.",
    },
  ];
}

export function titleOverlay(
  text: string,
  start = 0.2,
  end = 3.4,
  style: OverlayStyle = "youtube"
): Change {
  return {
    id: uid("ov"),
    type: "overlay",
    start,
    end,
    status: "pending",
    label: "Title card",
    rationale: "YouTube-style hook in the first seconds.",
    text,
    style,
  };
}

export function defaultPans(duration: number): Change[] {
  const spans: Array<{ start: number; end: number; kind: PanKind; label: string }> = [];
  if (duration > 3) {
    spans.push({
      start: 0.1,
      end: Math.min(4.2, duration * 0.22),
      kind: "zoom-in",
      label: "Open with a slow push-in",
    });
  }
  if (duration > 12) {
    const mid = duration * 0.48;
    spans.push({
      start: mid,
      end: Math.min(duration - 0.4, mid + 5),
      kind: "pan-right",
      label: "Drift across the mid section",
    });
  }
  if (duration > 20) {
    spans.push({
      start: Math.max(0, duration - 5.5),
      end: duration - 0.15,
      kind: "zoom-out",
      label: "Ease out at the end",
    });
  }
  return spans.map((s) => ({
    id: uid("pan"),
    type: "pan" as const,
    start: s.start,
    end: s.end,
    status: "pending" as const,
    label: s.label,
    rationale: "Keeps a locked-off shot from going flat.",
    pan: { kind: s.kind },
  }));
}

export function parseAgentJson(raw: string): { message: string; draft: Array<Partial<Change>> } {
  const trimmed = raw.trim();
  const fence = trimmed.match(/```(?:json)?\s*([\s\S]*?)```/);
  const body = fence ? fence[1] : trimmed;
  const start = body.indexOf("{");
  const end = body.lastIndexOf("}");
  if (start < 0 || end <= start) return { message: trimmed, draft: [] };
  try {
    const parsed = JSON.parse(body.slice(start, end + 1));
    return {
      message: String(parsed.message || parsed.reply || "").trim() || "Here are the edits.",
      draft: Array.isArray(parsed.changes) ? parsed.changes : [],
    };
  } catch {
    return { message: trimmed, draft: [] };
  }
}

export function normalizeDraft(
  draft: Array<Partial<Change>>,
  duration: number
): Change[] {
  const out: Change[] = [];
  for (const d of draft) {
    const type = d.type;
    if (!isChangeType(type)) continue;
    const start = clampNum(d.start, 0, Math.max(0, duration - 0.05));
    const end = clampNum(d.end, start + 0.08, duration);
    out.push({
      id: uid(type.slice(0, 3)),
      type,
      start,
      end,
      status: "pending",
      label: String(d.label || defaultLabel(type, d)),
      rationale: String(d.rationale || ""),
      text: d.text ? String(d.text).slice(0, 80) : undefined,
      style: (d.style as OverlayStyle) || "youtube",
      pan: d.pan?.kind ? { kind: d.pan.kind } : type === "pan" ? { kind: "zoom-in" } : undefined,
      rate: typeof d.rate === "number" ? d.rate : type === "slow" ? 0.5 : type === "fast" ? 1.8 : undefined,
    });
  }
  return out;
}

function defaultLabel(type: Change["type"], d: Partial<Change>) {
  if (type === "cut") return "Cut";
  if (type === "overlay" || type === "textcard" || type === "impact") return d.text || type;
  if (type === "pan") return "Camera move";
  return type;
}

function clampNum(n: unknown, min: number, max: number) {
  const v = Number(n);
  if (!Number.isFinite(v)) return min;
  return Math.min(max, Math.max(min, v));
}

// ---------- timeline tags ----------

function letterFor(n: number) {
  // 0 → A, 25 → Z, 26 → AA (spreadsheet letters)
  let s = "";
  n += 1;
  while (n > 0) {
    const r = (n - 1) % 26;
    s = String.fromCharCode(65 + r) + s;
    n = Math.floor((n - 1) / 26);
  }
  return s;
}

/** Auto tag name: first unused letter + whole seconds, e.g. A45, B34. */
export function nextTagName(tags: Tag[], t: number) {
  const used = new Set(tags.map((x) => x.name.replace(/\d+$/, "")));
  let i = 0;
  while (used.has(letterFor(i))) i++;
  return `${letterFor(i)}${Math.max(0, Math.floor(t))}`;
}

/** @mentions in the prompt that don't match any existing tag (lowercased,
 * deduped) — so the UI can say so instead of silently ignoring them. */
export function unknownTagMentions(prompt: string, tags: Tag[]): string[] {
  const known = new Set(tags.map((t) => t.name.toLowerCase()));
  const out: string[] = [];
  for (const m of prompt.matchAll(/@([a-z]{1,3}\d{1,5})\b/gi)) {
    const n = m[1].toLowerCase();
    if (!known.has(n) && !out.includes(n)) out.push(n);
  }
  return out;
}

export const SYSTEM_PROMPT = `You are Mustardy, a punchy video editor.
Return ONLY JSON: {"message":"short reply","changes":[...]}
Each change: type, start, end, label, rationale, optional text, style, pan.kind, rate.
Types: cut, overlay, pan, punch, slow, fast, shake, freeze, flash, textcard, tilt, impact, bounce, glitch, spotlight.
Rules: source seconds; 3–8 strong edits; cut only listed silences unless asked; text/textcard/impact = short ALL CAPS.`;
