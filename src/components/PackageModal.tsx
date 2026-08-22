import { useState } from "react";
import type { PackIdea } from "../lib/package";
import { formatTime } from "../lib/edits";

type Props = {
  ideas: PackIdea[];
  frames: Array<{ t: number; dataUrl: string }>;
  duration: number;
  onClose: () => void;
};

export function PackageModal({ ideas, frames, duration, onClose }: Props) {
  const [pick, setPick] = useState(0);
  const idea = ideas[pick] || ideas[0];
  const frame = frames[idea?.frame % Math.max(1, frames.length)] || frames[0];

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal pack-modal" onClick={(e) => e.stopPropagation()}>
        <div className="pack-head">
          <div>
            <h2>Titles & thumbnails</h2>
            <p>Ten options from the trimmed cut. Check the tiny previews — if you can’t read it there, neither can a viewer.</p>
          </div>
          <button className="ghost" onClick={onClose}>
            Close
          </button>
        </div>
        <div className="pack-grid">
          <div className="pack-list">
            {ideas.map((it, i) => (
              <button
                key={it.id}
                className={`pack-row ${i === pick ? "on" : ""}`}
                onClick={() => setPick(i)}
              >
                <strong>{i + 1}.</strong> {it.title}
                <span>{it.subtitle}</span>
              </button>
            ))}
          </div>
          {idea && (
            <div className="pack-preview">
              <ThumbCard idea={idea} src={frame?.dataUrl} />
              <div className="feed">
                <FeedItem idea={idea} src={frame?.dataUrl} size={320} views="12K views" />
                <FeedItem idea={idea} src={frame?.dataUrl} size={168} views="12K views" />
                <FeedItem idea={idea} src={frame?.dataUrl} size={120} views="12K" />
              </div>
              <p className="pack-meta">
                {formatTime(frame?.t || 0)} still · {idea.font.replace(/"/g, "").split(",")[0]} ·{" "}
                {Math.round(duration / 60)} min video
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function ThumbCard({ idea, src }: { idea: PackIdea; src?: string }) {
  return (
    <div className={`thumb-card lay-${idea.layout}`} style={{ fontFamily: idea.font }}>
      {src && <img src={src} alt="" />}
      <div className="thumb-dim" />
      {idea.layout === "vs" ? (
        <>
          <div className="thumb-vs-l">{idea.subtitle}</div>
          <div className="thumb-vs-r">{idea.title}</div>
        </>
      ) : (
        <>
          <div className="thumb-sub">{idea.subtitle}</div>
          <div className="thumb-title">{idea.title}</div>
        </>
      )}
    </div>
  );
}

function FeedItem({
  idea,
  src,
  size,
  views,
}: {
  idea: PackIdea;
  src?: string;
  size: number;
  views: string;
}) {
  const h = Math.round((size * 9) / 16);
  return (
    <div className="feed-item" style={{ width: size }}>
      <div className="feed-thumb" style={{ width: size, height: h }}>
        <ThumbCard idea={idea} src={src} />
        <span className="feed-dur">12:04</span>
      </div>
      <div className="feed-meta">
        <span className="feed-av" />
        <div>
          <div className="feed-title">{idea.title}</div>
          <div className="feed-sub">Em Vibecoding · {views}</div>
        </div>
      </div>
    </div>
  );
}
