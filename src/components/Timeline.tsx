import { useMemo, useRef, useState, type MouseEvent } from "react";
import type { Change, Tag, VideoInfo } from "../types";
import { formatTime } from "../lib/edits";

type Props = {
  video: VideoInfo | null;
  current: number;
  changes: Change[];
  tags: Tag[];
  selectedId: string | null;
  onSeek: (t: number) => void;
  onSelect: (id: string) => void;
  onRemoveTag: (id: string) => void;
  onAccept: (id: string) => void;
  onReject: (id: string) => void;
};

export function Timeline({
  video,
  current,
  changes,
  tags,
  selectedId,
  onSeek,
  onSelect,
  onRemoveTag,
  onAccept,
  onReject,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const duration = video?.duration || 0;
  const [hoverId, setHoverId] = useState<string | null>(null);

  const ticks = useMemo(() => {
    if (!duration) return ["0:00", "—"];
    return [0, duration * 0.25, duration * 0.5, duration * 0.75, duration].map(formatTime);
  }, [duration]);

  function timeFromEvent(e: MouseEvent) {
    const box = ref.current?.getBoundingClientRect();
    if (!box || !duration) return 0;
    const x = Math.min(1, Math.max(0, (e.clientX - box.left) / box.width));
    return x * duration;
  }

  function changeAt(t: number) {
    const hits = changes.filter((c) => t >= c.start && t < c.end);
    return (
      hits.find((c) => c.id === selectedId) ||
      hits.find((c) => c.status === "pending") ||
      hits[0] ||
      null
    );
  }

  const playheadChange = changeAt(current);
  const tip = (hoverId && changes.find((c) => c.id === hoverId)) || playheadChange;

  return (
    <div className="timeline">
      {tip && duration > 0 && (
        <div
          className={`tl-tip ${tip.status}`}
          style={{ left: `${(((tip.start + tip.end) / 2) / duration) * 100}%` }}
        >
          <span className="tl-tip-label">
            {tip.type} · {formatTime(tip.start)}–{formatTime(tip.end)}
          </span>
          {tip.status === "pending" && (
            <>
              <button
                type="button"
                className="tx-ok"
                aria-label="Accept"
                onClick={(e) => {
                  e.stopPropagation();
                  onAccept(tip.id);
                }}
              >
                ✓
              </button>
              <button
                type="button"
                className="tx-no"
                aria-label="Reject"
                onClick={(e) => {
                  e.stopPropagation();
                  onReject(tip.id);
                }}
              >
                ✕
              </button>
            </>
          )}
          {tip.status !== "pending" && <span className="tl-tip-status">{tip.status}</span>}
        </div>
      )}
      <div
        className="ruler"
        ref={ref}
        onMouseMove={(e) => {
          const hit = changeAt(timeFromEvent(e));
          setHoverId(hit?.id ?? null);
        }}
        onMouseLeave={() => setHoverId(null)}
        onClick={(e) => {
          if ((e.target as HTMLElement).dataset.change) return;
          onSeek(timeFromEvent(e));
        }}
      >
        <div className="track">
          {changes.map((c) => {
            if (!duration) return null;
            const left = `${(c.start / duration) * 100}%`;
            const width = `${(Math.max(0.08, c.end - c.start) / duration) * 100}%`;
            return (
              <button
                key={c.id}
                data-change="1"
                className={`seg ${c.type} ${c.status} ${selectedId === c.id ? "selected" : ""}`}
                style={{ left, width }}
                title={`${c.label}  ${formatTime(c.start)}–${formatTime(c.end)}`}
                onClick={(e) => {
                  e.stopPropagation();
                  onSelect(c.id);
                  onSeek(c.start);
                }}
              />
            );
          })}
          {tags.map((tag) => {
            if (!duration) return null;
            return (
              <button
                key={tag.id}
                className="tag-pin"
                style={{ left: `${(tag.t / duration) * 100}%` }}
                title={`@${tag.name.toLowerCase()} — ${formatTime(tag.t)} · right-click to remove`}
                onClick={(e) => {
                  e.stopPropagation();
                  onSeek(tag.t);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onRemoveTag(tag.id);
                }}
              >
                {tag.name}
              </button>
            );
          })}
          {duration > 0 && (
            <div className="head" style={{ left: `${(current / duration) * 100}%` }} />
          )}
        </div>
      </div>
      <div className="ticks">
        {ticks.map((t) => (
          <span key={t}>{t}</span>
        ))}
      </div>
    </div>
  );
}
