import type { ChangeType } from "../types";

export const TOOLS: Array<{ type: ChangeType; blurb: string }> = [
  { type: "cut", blurb: "Delete a range. Only use on known silences unless asked." },
  { type: "overlay", blurb: "YouTube caption over picture. Needs text." },
  { type: "pan", blurb: "Slow Ken Burns. pan.kind: zoom-in | zoom-out | pan-left | pan-right." },
  { type: "punch", blurb: "Snap zoom in and back. Comic emphasis, 0.25–0.6s." },
  { type: "slow", blurb: "Slow-mo. rate 0.4–0.7. Use on a beat or reaction, 0.8–3s." },
  { type: "fast", blurb: "Speed-up. rate 1.5–2.4. Skip fluff or rush a panic." },
  { type: "shake", blurb: "Handheld panic / earthquake. Funny stress. 0.4–1.4s." },
  { type: "freeze", blurb: "Hold the frame. Wait-for-it or record-scratch. 0.4–1.6s." },
  { type: "flash", blurb: "White flash. Cut punctuation. 0.12–0.35s." },
  { type: "textcard", blurb: "Smash-cut to a full-screen title. Needs short text." },
  { type: "tilt", blurb: "Dutch angle. Uneasy or goofy. 1–3s." },
  { type: "impact", blurb: "Flash + shake + giant word. Meme hit. Needs text." },
  { type: "bounce", blurb: "Playful scale bounce. 0.4–1s." },
  { type: "glitch", blurb: "RGB split / digital hiccup. 0.2–0.8s." },
  { type: "spotlight", blurb: "Dark vignette, subject in a hole of light. 1–4s." },
];

export const CHANGE_TYPES = TOOLS.map((t) => t.type);

export function isChangeType(v: unknown): v is ChangeType {
  return typeof v === "string" && (CHANGE_TYPES as string[]).includes(v);
}

export function toolListPrompt() {
  return TOOLS.map((t) => `- ${t.type}: ${t.blurb}`).join("\n");
}
