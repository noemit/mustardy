import { useEffect, useState, type RefObject } from "react";
import type { VideoInfo } from "../types";
import { formatTime } from "../lib/edits";
import mainImage from "../assets/main-image.png";

type Props = {
  video: VideoInfo | null;
  videoRef: RefObject<HTMLVideoElement | null>;
  standbyRef: RefObject<HTMLVideoElement | null>;
  current: number;
  playing: boolean;
  keepDur: number;
  loader?: string | null;
  note?: string | null;
  onToggle: () => void;
  onSeek: (t: number) => void;
  previewCuts: boolean;
  onTogglePreview: () => void;
  onOpen: () => void;
  onDropFile: (file: File) => void;
};

export function VideoStage({
  video,
  videoRef,
  standbyRef,
  current,
  playing,
  keepDur,
  loader,
  note,
  onToggle,
  onSeek,
  previewCuts,
  onTogglePreview,
  onOpen,
  onDropFile,
}: Props) {
  const [speed, setSpeed] = useState(1);

  useEffect(() => {
    for (const el of [videoRef.current, standbyRef.current]) {
      if (!el) continue;
      el.playbackRate = speed;
      (el as HTMLVideoElement & { preservesPitch?: boolean }).preservesPitch = true;
    }
  }, [speed, videoRef, standbyRef]);

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
  }, [current, onSeek, onToggle, video?.duration]);

  const wrapClass = ["player-wrap", video ? "has-video" : ""].filter(Boolean).join(" ");
  const saved = video ? Math.max(0, video.duration - keepDur) : 0;

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
              playsInline
              preload="auto"
              onClick={onToggle}
            />
            <video
              ref={standbyRef}
              className="standby"
              src={video.url}
              playsInline
              preload="auto"
              muted
            />
          </>
        ) : (
          <div className="empty">
            <img src={mainImage} alt="" className="empty-hero" />
            <h1>Drop in a video</h1>
            <p>Trim the pauses.</p>
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
          <span>
            {formatTime(current)} / {formatTime(video.duration)}
          </span>
          {saved > 0.05 && (
            <span className="cut-stat" title="Length after trims">
              {formatTime(keepDur)}
              <em> −{formatTime(saved)}</em>
            </span>
          )}
          {note && <span className="cut-stat">{note}</span>}
        </div>
      )}
    </section>
  );
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
