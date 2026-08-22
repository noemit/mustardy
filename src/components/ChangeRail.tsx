import { useEffect, useMemo, useRef, useState, type MouseEvent, type RefObject } from "react";
import type { Change, Transcript, TranscriptWord } from "../types";
import { formatTime } from "../lib/edits";

type Props = {
  changes: Change[];
  transcript: Transcript;
  current: number;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onSeek: (t: number) => void;
  onAccept: (id: string) => void;
  onReject: (id: string) => void;
  onAcceptAll: () => void;
  onRejectAll: () => void;
  onTranscribe: () => void;
  transcribing: boolean;
  onCutWords: (from: number, to: number) => void;
};

type Node =
  | { kind: "word"; word: TranscriptWord; index: number; cut?: Change }
  | { kind: "cut"; change: Change };

export function ChangeRail({
  changes,
  transcript,
  current,
  selectedId,
  onSelect,
  onSeek,
  onAccept,
  onReject,
  onAcceptAll,
  onRejectAll,
  onTranscribe,
  transcribing,
  onCutWords,
}: Props) {
  const pending = changes.filter((c) => c.status === "pending").length;
  const words = transcript.words;
  const hasScript = words.length > 0;
  const others = changes.filter((c) => c.type !== "cut");
  const nodes = useMemo(() => (hasScript ? buildNodes(words, changes) : []), [hasScript, words, changes]);
  const liveRef = useRef<HTMLButtonElement | null>(null);
  const drag = useRef(false);
  const [sel, setSel] = useState<{ a: number; b: number } | null>(null);
  const lo = sel ? Math.min(sel.a, sel.b) : -1;
  const hi = sel ? Math.max(sel.a, sel.b) : -1;

  useEffect(() => {
    liveRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [current]);

  useEffect(() => {
    const up = () => {
      drag.current = false;
    };
    const key = (e: KeyboardEvent) => {
      if (!sel) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      if (e.key === "Backspace" || e.key === "Delete") {
        e.preventDefault();
        onCutWords(sel.a, sel.b);
        setSel(null);
      }
      if (e.key === "Escape") setSel(null);
    };
    window.addEventListener("mouseup", up);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", key);
    };
  }, [sel, onCutWords]);

  return (
    <section className="review">
      <div className="panel-h">
        <strong>{hasScript ? "Transcript" : "Changes"}</strong>
        <span>
          {pending} pending
          {sel && (
            <>
              {" · "}
              <button className="tiny" onClick={() => { onCutWords(sel.a, sel.b); setSel(null); }}>
                cut words
              </button>
            </>
          )}
          {changes.length > 0 && (
            <>
              {" · "}
              <button className="tiny" onClick={onAcceptAll}>
                accept all
              </button>{" "}
              <button className="tiny" onClick={onRejectAll}>
                reject all
              </button>
            </>
          )}
        </span>
      </div>
      {hasScript ? (
        <div className="tx" tabIndex={0}>
          <p className="tx-hint">Select words, then Delete or “cut words” to drop them from the video.</p>
          {nodes.map((n, i) =>
            n.kind === "word" ? (
              <WordSpan
                key={`w-${i}-${n.word.t}`}
                word={n.word}
                index={n.index}
                picked={n.index >= lo && n.index <= hi}
                cut={n.cut}
                live={current >= n.word.t && current < Math.max(n.word.end, n.word.t + 0.12)}
                liveRef={liveRef}
                onSeek={onSeek}
                onDown={(idx, e) => {
                  drag.current = true;
                  if (e.shiftKey && sel) setSel({ a: sel.a, b: idx });
                  else setSel({ a: idx, b: idx });
                }}
                onEnter={(idx) => {
                  if (drag.current) setSel((s) => (s ? { ...s, b: idx } : s));
                }}
              />
            ) : (
              <CutChip
                key={n.change.id}
                change={n.change}
                words={words}
                selected={selectedId === n.change.id}
                onSelect={onSelect}
                onSeek={onSeek}
                onAccept={onAccept}
                onReject={onReject}
              />
            )
          )}
          {others.length > 0 && (
            <div className="tx-others">
              {others.map((c) => (
                <button
                  key={c.id}
                  className={`tx-fx ${c.status} ${selectedId === c.id ? "selected" : ""}`}
                  onClick={() => {
                    onSelect(c.id);
                    onSeek(c.start);
                  }}
                >
                  {c.type} · {formatTime(c.start)}
                  {c.status === "pending" && (
                    <>
                      <span
                        className="tx-ok"
                        onClick={(e) => {
                          e.stopPropagation();
                          onAccept(c.id);
                        }}
                      >
                        ✓
                      </span>
                      <span
                        className="tx-no"
                        onClick={(e) => {
                          e.stopPropagation();
                          onReject(c.id);
                        }}
                      >
                        ✕
                      </span>
                    </>
                  )}
                </button>
              ))}
            </div>
          )}
        </div>
      ) : (
        <>
          <div className="tx-offer">
            <p>
              Script view needs a transcript. Optional — skip it if you’re only trimming silences.
              Local tiny.en is a few minutes on a 20-minute video; small.en (Settings) is slower
              but better if you’ll cut individual words.
            </p>
            <button className="solid" disabled={transcribing} onClick={onTranscribe}>
              {transcribing ? "Transcribing…" : "Transcribe"}
            </button>
          </div>
          <Cards
            changes={changes}
            selectedId={selectedId}
            onSelect={onSelect}
            onAccept={onAccept}
            onReject={onReject}
          />
        </>
      )}
    </section>
  );
}

function WordSpan({
  word,
  index,
  picked,
  cut,
  live,
  liveRef,
  onSeek,
  onDown,
  onEnter,
}: {
  word: TranscriptWord;
  index: number;
  picked: boolean;
  cut?: Change;
  live: boolean;
  liveRef: RefObject<HTMLButtonElement | null>;
  onSeek: (t: number) => void;
  onDown: (index: number, e: MouseEvent) => void;
  onEnter: (index: number) => void;
}) {
  return (
    <button
      ref={live ? liveRef : undefined}
      type="button"
      className={`tx-word${cut ? ` in-cut ${cut.status}` : ""}${live ? " live" : ""}${picked ? " pick" : ""}`}
      title={formatTime(word.t)}
      onMouseDown={(e) => {
        if (e.button !== 0) return;
        onDown(index, e);
      }}
      onMouseEnter={() => onEnter(index)}
      onClick={() => onSeek(word.t)}
    >
      {word.text}
    </button>
  );
}

function CutChip({
  change,
  words,
  selected,
  onSelect,
  onSeek,
  onAccept,
  onReject,
}: {
  change: Change;
  words: TranscriptWord[];
  selected: boolean;
  onSelect: (id: string) => void;
  onSeek: (t: number) => void;
  onAccept: (id: string) => void;
  onReject: (id: string) => void;
}) {
  const before = lastWordBefore(words, change.start);
  const after = firstWordAfter(words, change.end);
  const dur = Math.max(0, change.end - change.start);
  const ctx = [
    before ? `after “${before.text}”` : "start",
    after ? `before “${after.text}”` : "end",
  ].join(" · ");
  return (
    <span
      className={`tx-cut ${change.status}${selected ? " selected" : ""}`}
      title={`${ctx} · ${formatTime(change.start)}–${formatTime(change.end)}`}
      onClick={() => {
        onSelect(change.id);
        onSeek(change.start);
      }}
    >
      <span className="tx-cut-dur">{dur.toFixed(1)}s</span>
      {change.status === "pending" && (
        <>
          <button
            type="button"
            className="tx-ok"
            aria-label="Accept cut"
            onClick={(e) => {
              e.stopPropagation();
              onAccept(change.id);
            }}
          >
            ✓
          </button>
          <button
            type="button"
            className="tx-no"
            aria-label="Reject cut"
            onClick={(e) => {
              e.stopPropagation();
              onReject(change.id);
            }}
          >
            ✕
          </button>
        </>
      )}
    </span>
  );
}

function Cards({
  changes,
  selectedId,
  onSelect,
  onAccept,
  onReject,
}: {
  changes: Change[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onAccept: (id: string) => void;
  onReject: (id: string) => void;
}) {
  return (
    <div className="cards">
      {changes.length === 0 && (
        <div className="card">
          <p>Queued edits show up here. A transcript turns this into the script.</p>
        </div>
      )}
      {changes.map((c) => (
        <article
          key={c.id}
          className={`card ${c.status} ${selectedId === c.id ? "selected" : ""}`}
          onClick={() => onSelect(c.id)}
        >
          <div className="card-top">
            <span className={`tag ${c.type}`}>{c.type}</span>
            <span className="card-time">
              {formatTime(c.start)}–{formatTime(c.end)}
            </span>
          </div>
          <h3>{c.label}</h3>
          {c.text && <p>“{c.text}”</p>}
          {c.rationale && <p>{c.rationale}</p>}
          {c.status === "pending" && (
            <div className="actions">
              <button className="accept" onClick={() => onAccept(c.id)}>
                Accept
              </button>
              <button className="reject" onClick={() => onReject(c.id)}>
                Reject
              </button>
            </div>
          )}
          {c.status !== "pending" && <p className="card-status">{c.status}</p>}
        </article>
      ))}
    </div>
  );
}

function buildNodes(words: TranscriptWord[], changes: Change[]): Node[] {
  const cuts = changes.filter((c) => c.type === "cut").sort((a, b) => a.start - b.start);
  const placed = new Set<string>();
  const nodes: Node[] = [];

  const inGap = (from: number, to: number) =>
    cuts.filter((c) => c.start < to && c.end > from && !placed.has(c.id));

  if (words[0]) {
    for (const c of inGap(-0.01, words[0].t)) {
      nodes.push({ kind: "cut", change: c });
      placed.add(c.id);
    }
  }
  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    const covering = cuts.find((c) => w.t < c.end && w.end > c.start);
    if (covering && !placed.has(covering.id)) {
      nodes.push({ kind: "cut", change: covering });
      placed.add(covering.id);
    }
    nodes.push({ kind: "word", word: w, index: i, cut: covering });
    const nextT = i + 1 < words.length ? words[i + 1].t : Number.POSITIVE_INFINITY;
    for (const c of inGap(w.end, nextT)) {
      nodes.push({ kind: "cut", change: c });
      placed.add(c.id);
    }
  }
  for (const c of cuts) {
    if (!placed.has(c.id)) nodes.push({ kind: "cut", change: c });
  }
  return nodes;
}

function lastWordBefore(words: TranscriptWord[], t: number) {
  let best: TranscriptWord | undefined;
  for (const w of words) {
    if (w.end <= t + 0.02) best = w;
    else break;
  }
  return best;
}

function firstWordAfter(words: TranscriptWord[], t: number) {
  return words.find((w) => w.t >= t - 0.02);
}
