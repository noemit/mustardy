export type Provider = "local" | "kimi" | "ollama";

export type ChangeType =
  | "cut"
  | "overlay"
  | "pan"
  | "punch"
  | "slow"
  | "fast"
  | "shake"
  | "freeze"
  | "flash"
  | "textcard"
  | "tilt"
  | "impact"
  | "bounce"
  | "glitch"
  | "spotlight";

export type ChangeStatus = "pending" | "accepted" | "rejected";
export type OverlayStyle = "youtube" | "minimal" | "bold-caption";
export type PanKind = "zoom-in" | "zoom-out" | "pan-left" | "pan-right";

export type Change = {
  id: string;
  type: ChangeType;
  start: number;
  end: number;
  status: ChangeStatus;
  label: string;
  rationale: string;
  origin?: "silence" | "manual";
  text?: string;
  style?: OverlayStyle;
  pan?: { kind: PanKind };
  rate?: number;
};

export type VideoInfo = {
  path: string;
  url: string;
  name: string;
  duration: number;
  width: number;
  height: number;
  fps: number;
  hasAudio: boolean;
};

export type TranscriptWord = {
  t: number;
  end: number;
  text: string;
};

/** A user-dropped marker on the timeline. Name is automatic: letter +
 * whole seconds, e.g. "A45". Chat resolves @a45 (case-insensitive). */
export type Tag = {
  id: string;
  name: string;
  t: number;
};

/** A quiet stretch from the audio scan. `visual` holds the times where the
 * picture changes mid-pause (pixel-diff scan) — trims split around them. */
export type SilenceRange = {
  start: number;
  end: number;
  visual?: number[];
};

export type Transcript = {
  text: string;
  words: TranscriptWord[];
  source: "whisper" | "none";
};

export type ChatMessage = {
  id: string;
  role: "user" | "assistant" | "system";
  text: string;
  changeIds?: string[];
  icon?: "check";
  kind?: "divider";
};

export type Settings = {
  provider: Provider;
  ollamaUrl: string;
  qwenModel: string;
  kimiKey: string;
  kimiBaseUrl: string;
  kimiModel: string;
  silenceNoise: string;
  silenceMin: number;
  silenceDrop: number;
  normalizeAudio: boolean;
  normalizeAmount: number;
  whisperModel: "tiny.en" | "small.en";
  theme: "light" | "dark";
};

export type ProjectFile = {
  version: 1;
  video: Omit<VideoInfo, "url">;
  silences: SilenceRange[];
  transcript: Transcript;
  changes: Change[];
  tags: Tag[];
};

export type AgentResponse = {
  message: string;
  changes: Array<Partial<Change> & { type: ChangeType; start: number; end: number }>;
};
