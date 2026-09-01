import { useEffect, useMemo, useRef, useState, type MouseEvent, type PointerEvent } from "react";
import type { Change, SilenceRange, VideoInfo } from "../types";
import { formatTime, uid } from "../lib/edits";
import { MIN_SILENCE_S, silenceTotal, wavePeaks, type AudioEnvelope } from "../lib/silence";

type Props = {
  video: VideoInfo | null;
  current: number;
  changes: Change[];
  silences: SilenceRange[];
  selectedId: string | null;
  envelope: AudioEnvelope | null;
  dropDb: number;
  minDur: number;
  onDrop: (db: number) => void;
  onMinDur: (s: number) => void;
  playing: boolean;
  normalize: boolean;
  normalizeAmount: number;
  onNormalize: (on: boolean) => void;
  onNormalizeAmount: (n: number) => void;
  keepDur: number;
  onSeek: (t: number) => void;
  onSelect: (id: string) => void;
  onManualCut: (id: string, start: number, end: number) => void;
  onAdjustManualCut: (id: string, start: number, end: number) => void;
  onDeleteChange: (id: string) => void;
};

type DragState = {
  id: string;
  kind: "create" | "move" | "start" | "end";
  pointerId: number;
  startClientX: number;
  anchor: number;
  offset: number;
  moved: boolean;
  captured: boolean;
};

const MIN_MANUAL_SPAN_S = 0.05;
const MAX_ZOOM = 16;

export function Timeline({
  video,
  current,
  changes,
  silences,
  selectedId,
  envelope,
  dropDb,
  minDur,
  onDrop,
  onMinDur,
  playing,
  normalize,
  normalizeAmount,
  onNormalize,
  onNormalizeAmount,
  keepDur,
  onSeek,
  onSelect,
  onManualCut,
  onAdjustManualCut,
  onDeleteChange,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const suppressClick = useRef(false);
  const duration = video?.duration || 0;
  const [hoverId, setHoverId] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [viewStart, setViewStart] = useState(0);

  const viewSpan = duration ? duration / zoom : 0;
  const maxViewStart = Math.max(0, duration - viewSpan);
  const viewStartClamped = Math.min(maxViewStart, Math.max(0, viewStart));
  const viewEnd = duration ? Math.min(duration, viewStartClamped + viewSpan) : 0;

  const peaks = useMemo(() => {
    if (!envelope) return [];
    const all = wavePeaks(envelope.dbs, 560);
    if (!duration || zoom <= 1) return all;
    const a = Math.max(0, Math.floor((viewStartClamped / duration) * all.length));
    const b = Math.min(all.length, Math.max(a + 1, Math.ceil((viewEnd / duration) * all.length)));
    return all.slice(a, b);
  }, [duration, envelope, viewEnd, viewStartClamped, zoom]);
  const quiet = useMemo(() => silenceTotal(silences), [silences]);

  const ticks = useMemo(() => {
    if (!duration || !viewSpan) return ["0:00", "—"];
    return [0, 0.25, 0.5, 0.75, 1].map((p) => formatTime(viewStartClamped + viewSpan * p));
  }, [duration, viewSpan, viewStartClamped]);

  useEffect(() => {
    setZoom(1);
    setViewStart(0);
    setHoverId(null);
  }, [video?.path, video?.duration]);

  useEffect(() => {
    if (!playing || !dragRef.current) return;
    dragRef.current = null;
    setDragging(false);
  }, [playing]);

  useEffect(() => {
    if (!playing || zoom <= 1 || !duration) return;
    if (current >= viewStartClamped && current <= viewEnd) return;
    const start = Math.min(maxViewStart, Math.max(0, current - viewSpan / 2));
    setViewStart(start);
  }, [current, duration, maxViewStart, playing, viewEnd, viewSpan, viewStartClamped, zoom]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Delete" && e.key !== "Backspace") return;
      if (playing || isTyping(e.target)) return;
      const change = changes.find((c) => c.id === selectedId);
      if (!change || change.type !== "cut") return;
      e.preventDefault();
      onDeleteChange(change.id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [changes, onDeleteChange, playing, selectedId]);

  function clampTime(t: number) {
    return Math.min(duration, Math.max(0, t));
  }

  function zoomTo(nextZoom: number, focus: number) {
    if (!duration) return;
    const z = Math.min(MAX_ZOOM, Math.max(1, nextZoom));
    const span = duration / z;
    const maxStart = Math.max(0, duration - span);
    const start = Math.min(maxStart, Math.max(0, clampTime(focus) - span / 2));
    setZoom(z);
    setViewStart(start);
  }

  function zoomBy(factor: number) {
    if (!duration) return;
    const focus =
      current >= viewStartClamped && current <= viewEnd
        ? current
        : viewStartClamped + viewSpan / 2;
    zoomTo(zoom * factor, focus);
  }

  function pct(t: number) {
    return viewSpan ? ((t - viewStartClamped) / viewSpan) * 100 : 0;
  }

  function widthPct(start: number, end: number) {
    return viewSpan ? (Math.max(0.02, end - start) / viewSpan) * 100 : 0;
  }

  function timeFromClientX(clientX: number) {
    const box = ref.current?.getBoundingClientRect();
    if (!box || !duration || !viewSpan) return 0;
    const x = Math.min(1, Math.max(0, (clientX - box.left) / box.width));
    return viewStartClamped + x * viewSpan;
  }

  function timeFromEvent(e: MouseEvent) {
    return timeFromClientX(e.clientX);
  }

  function changeAt(t: number) {
    const hits = changes.filter((c) => t >= c.start && t < c.end);
    return hits.find((c) => c.id === selectedId) || hits[0] || null;
  }

  function beginDrag(e: PointerEvent, drag: Omit<DragState, "moved" | "captured">) {
    if (playing || !duration || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    dragRef.current = { ...drag, moved: false, captured: false };
    setDragging(true);
  }

  function captureDrag() {
    const drag = dragRef.current;
    if (!drag || drag.captured) return;
    drag.captured = true;
    try {
      ref.current?.setPointerCapture(drag.pointerId);
    } catch {
      /* pointer already gone */
    }
  }

  function moveDrag(e: PointerEvent) {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== e.pointerId || !duration) return;
    if (!drag.moved && Math.abs(e.clientX - drag.startClientX) < 4) return;
    if (!drag.moved) {
      drag.moved = true;
      captureDrag();
      onSelect(drag.id);
    }

    const t = clampTime(timeFromClientX(e.clientX));
    if (drag.kind === "create") {
      onManualCut(drag.id, Math.min(drag.anchor, t), Math.max(drag.anchor, t));
      return;
    }

    const change = changes.find((c) => c.id === drag.id);
    if (!change) return;
    if (drag.kind === "move") {
      const len = change.end - change.start;
      const maxStart = Math.max(0, duration - len);
      const start = Math.min(maxStart, Math.max(0, t - drag.offset));
      onAdjustManualCut(drag.id, start, start + len);
    } else if (drag.kind === "start") {
      onAdjustManualCut(drag.id, Math.min(t, change.end - MIN_MANUAL_SPAN_S), change.end);
    } else {
      onAdjustManualCut(drag.id, change.start, Math.max(t, change.start + MIN_MANUAL_SPAN_S));
    }
  }

  function endDrag(e: PointerEvent, canceled = false) {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;
    if (!canceled) moveDrag(e);
    dragRef.current = null;
    setDragging(false);
    if (drag.captured) {
      try {
        ref.current?.releasePointerCapture(drag.pointerId);
      } catch {
        /* already released */
      }
    }
    if (!canceled && !drag.moved) {
      if (drag.kind === "create") onSeek(drag.anchor);
      else {
        const change = changes.find((c) => c.id === drag.id);
        if (change) {
          onSelect(change.id);
          onSeek(change.start);
        }
      }
    }
    suppressClick.current = true;
    window.setTimeout(() => {
      suppressClick.current = false;
    }, 0);
  }

  function clickSuppressed() {
    if (!suppressClick.current) return false;
    suppressClick.current = false;
    return true;
  }

  const playheadChange = changeAt(current);
  const tip = (hoverId && changes.find((c) => c.id === hoverId)) || playheadChange;
  const tipMid = tip ? (tip.start + tip.end) / 2 : 0;
  const tipLeft = Math.min(100, Math.max(0, pct(tipMid)));

  return (
    <div className="timeline">
      {tip && duration > 0 && (
        <div className={`tl-tip ${tip.status}`} style={{ left: `${tipLeft}%` }}>
          <span className="tl-tip-label">
            {tip.type} · {formatTime(tip.start)}–{formatTime(tip.end)}
          </span>
          {tip.type === "cut" && tip.id === selectedId && !playing && (
            <button
              type="button"
              className="tx-no tl-tip-delete"
              aria-label="Delete cut"
              onClick={(e) => {
                e.stopPropagation();
                onDeleteChange(tip.id);
              }}
            >
              delete
            </button>
          )}
        </div>
      )}
      <div
        className={`ruler ${playing ? "playing" : ""} ${dragging ? "dragging" : ""}`}
        ref={ref}
        onMouseMove={(e) => {
          if (dragRef.current) return;
          const hit = changeAt(timeFromEvent(e));
          setHoverId(hit?.id ?? null);
        }}
        onMouseLeave={() => setHoverId(null)}
        onPointerDown={(e) => {
          if ((e.target as HTMLElement).closest("[data-change]")) return;
          const anchor = clampTime(timeFromClientX(e.clientX));
          beginDrag(e, {
            id: uid("cut"),
            kind: "create",
            pointerId: e.pointerId,
            startClientX: e.clientX,
            anchor,
            offset: 0,
          });
        }}
        onPointerMove={moveDrag}
        onPointerUp={(e) => endDrag(e)}
        onPointerCancel={(e) => endDrag(e, true)}
        onClick={(e) => {
          if (clickSuppressed()) return;
          if ((e.target as HTMLElement).closest("[data-change]")) return;
          onSeek(timeFromEvent(e));
        }}
      >
        <div className="track">
          {peaks.length > 0 && (
            <div className="wave" aria-hidden="true">
              {peaks.map((db, i) => (
                <span
                  key={i}
                  style={{ height: `${Math.max(6, Math.min(100, ((db + 70) / 70) * 100))}%` }}
                />
              ))}
            </div>
          )}
          {silences.map((r, i) => {
            if (!duration) return null;
            return (
              <div
                key={`sil-${i}`}
                className="seg silence"
                style={{
                  left: `${pct(r.start)}%`,
                  width: `${widthPct(r.start, r.end)}%`,
                }}
              />
            );
          })}
          {changes.map((c) => {
            if (!duration) return null;
            const left = `${pct(c.start)}%`;
            const width = `${widthPct(c.start, c.end)}%`;
            const manualCut = c.type === "cut" && c.origin === "manual";
            const adjustable = manualCut && !playing;
            return (
              <div
                key={c.id}
                data-change="1"
                role="button"
                tabIndex={0}
                className={`seg ${c.type} ${c.status} ${manualCut ? "manual" : ""} ${
                  selectedId === c.id ? "selected" : ""
                }`}
                style={{ left, width }}
                title={`${c.label}  ${formatTime(c.start)}–${formatTime(c.end)}`}
                onPointerDown={(e) => {
                  if (!adjustable) return;
                  beginDrag(e, {
                    id: c.id,
                    kind: "move",
                    pointerId: e.pointerId,
                    startClientX: e.clientX,
                    anchor: c.start,
                    offset: timeFromClientX(e.clientX) - c.start,
                  });
                }}
                onClick={(e) => {
                  e.stopPropagation();
                  if (clickSuppressed()) return;
                  onSelect(c.id);
                  onSeek(c.start);
                }}
                onKeyDown={(e) => {
                  if (e.key !== "Enter" && e.key !== " ") return;
                  e.preventDefault();
                  onSelect(c.id);
                  onSeek(c.start);
                }}
              >
                {selectedId === c.id && adjustable && (
                  <>
                    <span
                      className="cut-handle start"
                      aria-hidden="true"
                      onPointerDown={(e) =>
                        beginDrag(e, {
                          id: c.id,
                          kind: "start",
                          pointerId: e.pointerId,
                          startClientX: e.clientX,
                          anchor: c.start,
                          offset: 0,
                        })
                      }
                    />
                    <span
                      className="cut-handle end"
                      aria-hidden="true"
                      onPointerDown={(e) =>
                        beginDrag(e, {
                          id: c.id,
                          kind: "end",
                          pointerId: e.pointerId,
                          startClientX: e.clientX,
                          anchor: c.end,
                          offset: 0,
                        })
                      }
                    />
                  </>
                )}
              </div>
            );
          })}
          {duration > 0 && <div className="head" style={{ left: `${pct(current)}%` }} />}
        </div>
      </div>
      <div className="ticks">
        {ticks.map((t) => (
          <span key={t}>{t}</span>
        ))}
      </div>
      {video && (
        <div className="timeline-tools">
          <span className="drag-hint">
            {playing
              ? "Pause to adjust cuts"
              : "Drag empty timeline to cut · click a cut, then Delete to remove it"}
          </span>
          <div className="zoom-tools" aria-label="Timeline zoom">
            <button type="button" className="ghost icon" disabled={zoom <= 1} onClick={() => zoomBy(0.5)}>
              −
            </button>
            <span>{zoom}×</span>
            <button
              type="button"
              className="ghost icon"
              disabled={zoom >= MAX_ZOOM}
              onClick={() => zoomBy(2)}
            >
              +
            </button>
            {zoom > 1 && (
              <button type="button" className="ghost" onClick={() => zoomTo(1, duration / 2)}>
                reset
              </button>
            )}
          </div>
        </div>
      )}
      {video && (
        <div className="silence-bar">
          <label className="silence-slider">
            <header>
              <span>Quieter by</span>
              <strong>{Math.round(dropDb)} dB</strong>
            </header>
            <input
              type="range"
              min={8}
              max={28}
              step={1}
              value={Math.min(28, Math.max(8, dropDb))}
              disabled={!envelope || playing}
              onChange={(e) => onDrop(Number(e.target.value))}
            />
          </label>
          <label className="silence-slider">
            <header>
              <span>Min pause</span>
              <strong>{minDur < 1 ? minDur.toFixed(2) : minDur.toFixed(1)} s</strong>
            </header>
            <input
              type="range"
              min={MIN_SILENCE_S}
              max={2}
              step={0.01}
              value={Math.min(2, Math.max(MIN_SILENCE_S, minDur))}
              disabled={!envelope || playing}
              onChange={(e) => onMinDur(Number(e.target.value))}
            />
          </label>
          <div className="silence-meta">
            <span>
              {silences.length} gap{silences.length === 1 ? "" : "s"}
              {quiet > 0 ? ` · ${formatTime(quiet)} quiet` : ""}
            </span>
            {duration > 0 && keepDur < duration - 0.05 && (
              <span className="cut-stat" title="Length after trims">
                {formatTime(keepDur)}
                <em> −{formatTime(duration - keepDur)}</em>
              </span>
            )}
          </div>
          <label className="silence-check">
            <input
              type="checkbox"
              checked={normalize}
              disabled={!video.hasAudio}
              onChange={(e) => onNormalize(e.target.checked)}
            />
            Normalize export
          </label>
          <label className="silence-slider">
            <header>
              <span>Evenness</span>
              <strong>{Math.round(normalizeAmount * 100)}%</strong>
            </header>
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={normalizeAmount}
              disabled={!normalize || !video.hasAudio}
              onChange={(e) => onNormalizeAmount(Number(e.target.value))}
            />
          </label>
        </div>
      )}
    </div>
  );
}

function isTyping(target: EventTarget | null) {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable;
}
