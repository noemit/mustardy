import { useEffect, useRef, useState, type CSSProperties } from "react";
import { PACK_FONTS, type PackIdea } from "../lib/package";
import {
  BRAND_MOODS,
  DEFAULT_BRAND,
  exportBrand,
  importBrand,
  loadBrand,
  saveBrand,
  type BrandStyle,
} from "../lib/brand";
import { formatTime } from "../lib/edits";

type Props = {
  ideas: PackIdea[];
  frames: Array<{ t: number; dataUrl: string }>;
  duration: number;
  onClose: () => void;
};

export function PackageModal({ ideas, frames, duration, onClose }: Props) {
  const [pick, setPick] = useState(0);
  const [brand, setBrand] = useState<BrandStyle>(() => loadBrand());
  const idea = ideas[pick] || ideas[0];
  const frame = frames[idea?.frame % Math.max(1, frames.length)] || frames[0];

  // Every tweak persists immediately — the brand is cached on this machine.
  const update = (patch: Partial<BrandStyle>) => {
    setBrand((b) => {
      const next = { ...b, ...patch };
      saveBrand(next);
      return next;
    });
  };

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
        <BrandCard brand={brand} onChange={update} />
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
              <ThumbCard idea={idea} src={frame?.dataUrl} brand={brand} />
              <div className="feed">
                <FeedItem idea={idea} src={frame?.dataUrl} size={320} views="12K views" duration={duration} brand={brand} />
                <FeedItem idea={idea} src={frame?.dataUrl} size={168} views="12K views" duration={duration} brand={brand} />
                <FeedItem idea={idea} src={frame?.dataUrl} size={120} views="12K" duration={duration} brand={brand} />
              </div>
              <p className="pack-meta">
                {formatTime(frame?.t || 0)} still · {brand.font.replace(/"/g, "").split(",")[0]} ·{" "}
                {Math.round(duration / 60)} min video
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** Colors, font and mood that apply to every thumbnail. Cached on this
 *  machine (localStorage), exportable/importable as a small JSON file. */
export function BrandCard({ brand, onChange }: { brand: BrandStyle; onChange: (p: Partial<BrandStyle>) => void }) {
  const fileRef = useRef<HTMLInputElement>(null);
  return (
    <div className="brand-card">
      <div className="brand-row">
        <span className="brand-label">Brand style</span>
        <label className="brand-field">
          Accent
          <input type="color" value={brand.accent} onChange={(e) => onChange({ accent: e.target.value })} />
        </label>
        <label className="brand-field">
          Text
          <input type="color" value={brand.ink} onChange={(e) => onChange({ ink: e.target.value })} />
        </label>
        <label className="brand-field">
          Font
          <select value={brand.font} onChange={(e) => onChange({ font: e.target.value })}>
            {PACK_FONTS.map((f) => (
              <option key={f.name} value={f.css}>
                {f.name}
              </option>
            ))}
          </select>
        </label>
        <span className="brand-spacer" />
        <button className="ghost" onClick={() => exportBrand(brand)} title="Download as mustardy-brand.json">
          Export
        </button>
        <button className="ghost" onClick={() => fileRef.current?.click()} title="Load a mustardy-brand.json">
          Import
        </button>
        <button className="ghost" onClick={() => onChange(DEFAULT_BRAND)} title="Back to the default look">
          Reset
        </button>
        <input
          ref={fileRef}
          type="file"
          accept="application/json"
          hidden
          onChange={async (e) => {
            const f = e.target.files?.[0];
            if (f) onChange(await importBrand(f));
            e.target.value = "";
          }}
        />
      </div>
      <div className="brand-moods">
        {BRAND_MOODS.map((m) => (
          <button
            key={m.id}
            className={`brand-mood ${brand.mood === m.id ? "on" : ""}`}
            onClick={() => onChange({ mood: m.id })}
            title={m.blurb}
          >
            {m.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * Thumbnails are drawn on a fixed 1280×720 canvas (px font sizes) and then
 * transform-scaled to whatever width the card actually renders at — so the
 * big preview and the 120px feed item are the exact same image, just
 * smaller. Fonts used to size off the viewport (vw/px), which is why the
 * tiny previews kept giant text and overflowed.
 */
const THUMB_W = 1280;
const THUMB_H = 720;

export function ThumbCard({ idea, src, brand }: { idea: PackIdea; src?: string; brand?: BrandStyle }) {
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(el.clientWidth));
    ro.observe(el);
    setW(el.clientWidth);
    return () => ro.disconnect();
  }, []);
  const scale = w > 0 ? w / THUMB_W : 0;
  const b = brand || DEFAULT_BRAND;

  return (
    <div
      ref={ref}
      className={`thumb-card lay-${idea.layout} mood-${b.mood}`}
      style={
        {
          fontFamily: b.font,
          "--brand-accent": b.accent,
          "--brand-ink": b.ink,
        } as CSSProperties
      }
    >
      <div className="thumb-inner" style={{ width: THUMB_W, height: THUMB_H, transform: `scale(${scale})` }}>
        {src && <img src={src} alt="" />}
        <div className="thumb-dim" />
        {idea.layout === "vs" ? (
          <>
            <div className="thumb-vs-l">{idea.left || idea.subtitle}</div>
            <div className="thumb-vs-r">{idea.right || idea.subtitle}</div>
          </>
        ) : idea.layout === "clean" ? null : (
          <div className="thumb-sub">{idea.subtitle}</div>
        )}
      </div>
    </div>
  );
}

function FeedItem({
  idea,
  src,
  size,
  views,
  duration,
  brand,
}: {
  idea: PackIdea;
  src?: string;
  size: number;
  views: string;
  duration: number;
  brand?: BrandStyle;
}) {
  const h = Math.round((size * 9) / 16);
  return (
    <div className="feed-item" style={{ width: size }}>
      <div className="feed-thumb" style={{ width: size, height: h }}>
        <ThumbCard idea={idea} src={src} brand={brand} />
        <span className="feed-dur">{formatTime(duration)}</span>
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
