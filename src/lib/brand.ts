import { PACK_FONTS } from "./package";

/** The aesthetic presets — how the thumbnail text behaves. Colors and font
 *  come from the brand; the mood decides size, tilt, weight and dimming. */
export const BRAND_MOODS = [
  { id: "loud", label: "Loud", blurb: "Huge tilted type, heavy shadow. Maximum energy." },
  { id: "crisp", label: "Crisp", blurb: "Straight, tight tracking, clean contrast. Tech-review." },
  { id: "minimal", label: "Minimal", blurb: "Small quiet type — the frame does the talking." },
  { id: "clear", label: "Clear", blurb: "Boxed accent labels. Readable at any size, always." },
  { id: "playful", label: "Playful", blurb: "Bouncy tilt, sticker vibe, softer shadow." },
  { id: "noir", label: "Noir", blurb: "Moody dim, restrained type, cinematic." },
] as const;

export type BrandMood = (typeof BRAND_MOODS)[number]["id"];

export type BrandStyle = {
  /** Highlight color: stamps, VS winner side, kicker accents. */
  accent: string;
  /** Main text color on the thumbnail. */
  ink: string;
  /** CSS font stack (one of PACK_FONTS). */
  font: string;
  mood: BrandMood;
};

export const DEFAULT_BRAND: BrandStyle = {
  accent: "#ffe566",
  ink: "#ffffff",
  font: PACK_FONTS[0].css,
  mood: "loud",
};

const KEY = "mustardy.brand";

/** Brand style is per-machine: cached in localStorage of the webview. */
export function loadBrand(): BrandStyle {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULT_BRAND;
    return normalizeBrand(JSON.parse(raw));
  } catch {
    return DEFAULT_BRAND;
  }
}

export function saveBrand(b: BrandStyle) {
  try {
    localStorage.setItem(KEY, JSON.stringify(b));
  } catch {
    // private-mode webview — brand just won't persist
  }
}

/** Accept anything vaguely brand-shaped; fall back per-field. */
export function normalizeBrand(x: unknown): BrandStyle {
  const o = (x && typeof x === "object" ? x : {}) as Record<string, unknown>;
  const color = (v: unknown, d: string) =>
    typeof v === "string" && /^#[0-9a-f]{3,8}$/i.test(v) ? v : d;
  const moods = new Set<string>(BRAND_MOODS.map((m) => m.id));
  return {
    accent: color(o.accent, DEFAULT_BRAND.accent),
    ink: color(o.ink, DEFAULT_BRAND.ink),
    font:
      typeof o.font === "string" && PACK_FONTS.some((f) => f.css === o.font)
        ? (o.font as string)
        : DEFAULT_BRAND.font,
    mood: typeof o.mood === "string" && moods.has(o.mood) ? (o.mood as BrandMood) : DEFAULT_BRAND.mood,
  };
}

/** Download the brand as a JSON file (works in the webview and plain web). */
export function exportBrand(b: BrandStyle) {
  const blob = new Blob([JSON.stringify(b, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = "mustardy-brand.json";
  a.click();
  URL.revokeObjectURL(url);
}

export async function importBrand(file: File): Promise<BrandStyle> {
  return normalizeBrand(JSON.parse(await file.text()));
}
