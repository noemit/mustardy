import { useEffect, useMemo, useState, type CSSProperties, type RefObject } from "react";
import type { Change, VideoInfo } from "../types";
import { activeAt, formatTime } from "../lib/edits";
import appIcon from "../assets/app-icon.png";

type Props = {
  video: VideoInfo | null;
  videoRef: RefObject<HTMLVideoElement | null>;
  current: number;
  playing: boolean;
  changes: Change[];
  /** Long-running work over the picture (scan / transcribe / package). */
  loader?: string | null;
  onToggle: () => void;
  onSeek: (t: number) => void;
  onTag: () => void;
  previewCuts: boolean;
  onTogglePreview: () => void;
  onOpen: () => void;
  onDropFile: (file: File) => void;
};

export function VideoStage({
  video,
  videoRef,
  current,
  playing,
  changes,
  loader,
  onToggle,
  onSeek,
  onTag,
  previewCuts,
  onTogglePreview,
  onOpen,
  onDropFile,
}: Props) {
  const [speed, setSpeed] = useState(1);
  const live = useMemo(() => {
    const pick = (type: Change["type"]) =>
      activeAt(current, changes, type).find((c) => c.status !== "rejected");
    return {
      overlay: pick("overlay"),
      pan: pick("pan"),
      punch: pick("punch"),
      slow: pick("slow"),
      fast: pick("fast"),
      shake: pick("shake"),
      freeze: pick("freeze"),
      flash: pick("flash"),
      textcard: pick("textcard"),
      tilt: pick("tilt"),
      impact: pick("impact"),
      bounce: pick("bounce"),
      glitch: pick("glitch"),
      spotlight: pick("spotlight"),
    };
  }, [changes, current]);

  useEffect(() => {
    const el = videoRef.current;
    if (!el) return;
    if (live.freeze) el.playbackRate = 0.08;
    else if (live.slow) el.playbackRate = live.slow.rate || 0.5;
    else if (live.fast) el.playbackRate = live.fast.rate || 1.8;
    else el.playbackRate = speed;
  }, [live.fast, live.freeze, live.slow, speed, videoRef]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target)) return;
      if (e.code === "Space") {
        e.preventDefault();
        onToggle();
      }
      if (e.code === "Home") {
        e.preventDefault();
        onSeek(0);
      }
      if (e.code === "ArrowLeft") {
        e.preventDefault();
        onSeek(Math.max(0, current - 10));
      }
      if (e.code === "ArrowRight") {
        e.preventDefault();
        onSeek(Math.min(video?.duration || 0, current + 10));
      }
      if (e.code === "KeyT") {
        e.preventDefault();
        onTag();
      }
      if (e.code === "Period" && e.shiftKey) {
        e.preventDefault();
        setSpeed((s) => (s === 1 ? 1.5 : s === 1.5 ? 2 : 1));
      }
      if (e.code === "Comma" && e.shiftKey) {
        e.preventDefault();
        setSpeed((s) => (s === 2 ? 1.5 : s === 1.5 ? 1 : 2));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [current, onSeek, onTag, onToggle, video?.duration]);

  const wrapClass = [
    "player-wrap",
    video ? "has-video" : "",
    live.shake || live.impact ? "fx-shake" : "",
    live.glitch ? "fx-glitch" : "",
    live.spotlight ? "fx-spot" : "",
    live.freeze ? "fx-freeze" : "",
    live.bounce ? "fx-bounce" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const card = live.textcard || live.impact;

  return (
    <section
      className="stage"
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => {
        e.preventDefault();
        const file = e.dataTransfer.files[0];
        if (file) onDropFile(file);
      }}
    >
      <div className={wrapClass}>
        {video ? (
          <>
            <video
              ref={videoRef}
              src={video.url}
              style={videoStyle(live, current)}
              onClick={onToggle}
            />
            {(live.flash || live.impact) && <div className="fx-flash" />}
            {live.overlay?.text && !card && (
              <div className="overlay-layer">
                <div className={`yt-caption ${live.overlay.style || "youtube"}`}>{live.overlay.text}</div>
              </div>
            )}
            {card?.text && (
              <div className={`textcard ${card.type === "impact" ? "impact" : ""}`}>
                <span>{card.text}</span>
              </div>
            )}
          </>
        ) : (
          <div className="empty">
            <img src={appIcon} alt="" className="empty-icon" />
            <h1>Drop in a video</h1>
            <p>Chat proposes the edits — silence trims, punch-ins, titles. You accept or reject each one.</p>
            <div className="drop">
              <button className="solid" onClick={onOpen}>
                Open a video
              </button>
              <p className="drop-or">or drag a file here</p>
            </div>
          </div>
        )}
        {video && loader && (
          <div className="stage-loader">
            <div className="stage-loader-card">
              <div className="loader-ring" />
              <span className="loader-label">{loader}</span>
            </div>
          </div>
        )}
      </div>
      {video && (
        <div className="transport">
          <button className="ghost icon" onClick={() => onSeek(0)} aria-label="Go to start" title="Go to start (Home)">
            <IconSkipBack />
          </button>
          <button className="ghost icon" onClick={() => onSeek(Math.max(0, current - 10))} aria-label="Back 10 seconds" title="Back 10 seconds (←)">
            <IconReplay10 />
          </button>
          <button className="play" onClick={onToggle} aria-label={playing ? "Pause" : "Play"}>
            {playing ? "❚❚" : "▶"}
          </button>
          <button className="ghost icon" onClick={() => onSeek(Math.min(video.duration, current + 10))} aria-label="Forward 10 seconds" title="Forward 10 seconds (→)">
            <IconForward10 />
          </button>
          <button className={`ghost ${previewCuts ? "on" : ""}`} onClick={onTogglePreview}>
            {previewCuts ? "preview on" : "preview off"}
          </button>
          <button
            className={`ghost ${speed !== 1 ? "on" : ""}`}
            onClick={() => setSpeed((s) => (s === 1 ? 1.5 : s === 1.5 ? 2 : 1))}
            title="Preview speed (Shift+. faster, Shift+, slower)"
          >
            {speed === 1 ? "1×" : speed === 1.5 ? "1.5×" : "2×"}
          </button>
          <button className="ghost" onClick={onTag} title="Tag the playhead (T) — then say e.g. “trim between @a45 and @b34”">
            tag
          </button>
          <span>
            {formatTime(current)} / {formatTime(video.duration)}
          </span>
          <span>
            {video.width}×{video.height}
          </span>
        </div>
      )}
    </section>
  );
}

type LiveFx = {
  pan?: Change;
  punch?: Change;
  tilt?: Change;
};

function videoStyle(live: LiveFx, t: number): CSSProperties {
  const bits: string[] = [];
  if (live.pan?.pan) {
    const p = prog(live.pan, t);
    const kind = live.pan.pan.kind;
    if (kind === "zoom-in") bits.push(`scale(${1 + p * 0.14})`);
    else if (kind === "zoom-out") bits.push(`scale(${1.14 - p * 0.14})`);
    else if (kind === "pan-left") bits.push(`scale(1.12) translateX(${8 - p * 16}%)`);
    else bits.push(`scale(1.12) translateX(${-8 + p * 16}%)`);
  }
  if (live.punch) {
    const p = prog(live.punch, t);
    const pulse = Math.sin(p * Math.PI);
    bits.push(`scale(${1 + pulse * 0.22})`);
  }
  if (live.tilt) bits.push("rotate(-8deg) scale(1.12)");
  return {
    transformOrigin: "50% 42%",
    transform: bits.length ? bits.join(" ") : undefined,
    transition: live.punch ? "none" : "transform 80ms linear",
  };
}

function prog(change: Change, t: number) {
  return Math.min(1, Math.max(0, (t - change.start) / Math.max(0.01, change.end - change.start)));
}

function isTyping(target: EventTarget | null) {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable;
}

const iconProps = {
  viewBox: "0 0 24 24",
  width: 14,
  height: 14,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 2,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const;

function IconSkipBack() {
  return (
    <svg {...iconProps}>
      <polygon points="19 20 9 12 19 4 19 20" fill="currentColor" stroke="none" />
      <line x1="5" y1="19" x2="5" y2="5" />
    </svg>
  );
}

function IconReplay10() {
  return (
    <svg {...iconProps}>
      <polyline points="1 4 1 10 7 10" />
      <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
      <text x="12" y="15.5" textAnchor="middle" fontSize="8" fill="currentColor" stroke="none" fontFamily="inherit">
        10
      </text>
    </svg>
  );
}

function IconForward10() {
  return (
    <svg {...iconProps}>
      <polyline points="23 4 23 10 17 10" />
      <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
      <text x="12" y="15.5" textAnchor="middle" fontSize="8" fill="currentColor" stroke="none" fontFamily="inherit">
        10
      </text>
    </svg>
  );
}
