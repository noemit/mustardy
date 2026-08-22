import { native, platform } from "../lib/bridge";
import appIcon from "../assets/app-icon.png";
import type { BrainStatus } from "../lib/gemma";
import type { EyesStatus } from "../lib/vision";
import type { Provider } from "../types";

type Props = {
  provider: Provider;
  eyes: EyesStatus;
  brain: BrainStatus;
  theme: "light" | "dark";
  onToggleTheme: () => void;
  onOpen: () => void;
  onSave: () => void;
  onTranscript: () => void;
  onSettings: () => void;
  onExport: () => void;
  onPackage: () => void;
  onQuit: () => void;
  canSave: boolean;
  canTranscript: boolean;
  hasTranscript: boolean;
  canExport: boolean;
  canPackage: boolean;
};

export function Titlebar({
  provider,
  eyes,
  brain,
  theme,
  onToggleTheme,
  onOpen,
  onSave,
  onTranscript,
  onSettings,
  onExport,
  onPackage,
  onQuit,
  canSave,
  canTranscript,
  hasTranscript,
  canExport,
  canPackage,
}: Props) {
  const ready = provider === "local" && eyes.state === "ready" && brain.state === "ready";
  const label =
    provider === "kimi" ? "Kimi" : provider === "ollama" ? "Ollama" : ready ? "Local models" : "Local";
  const dot = provider !== "local" ? "" : ready ? "on" : eyes.state === "error" || brain.state === "error" ? "off" : "";
  return (
    <header className={`titlebar ${native && platform === "darwin" ? "" : "web"}`} data-tauri-drag-region>
      <div className="brand">
        <img src={appIcon} alt="" />
        <span className="word">
          mustard<em>y</em>
        </span>
      </div>
      <div className="bar-right">
        <span className={`chip ${ready ? "on" : ""}`}>
          <span className={`status-dot ${dot}`} /> {label}
        </span>
        <button className="ghost" onClick={onOpen}>
          Open
        </button>
        <button className="ghost" disabled={!canSave} onClick={onSave} title="Save project (⌘S)">
          Save
        </button>
        <button
          className="ghost"
          disabled={!canTranscript}
          onClick={onTranscript}
          title={
            hasTranscript
              ? "Export transcript as .txt or .srt"
              : "Transcribe with Whisper (~3 min locally on a 20-min video), then export"
          }
        >
          {hasTranscript ? "Export .txt" : "Transcribe"}
        </button>
        <button
          className="ghost icon"
          onClick={onToggleTheme}
          aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
          title={theme === "dark" ? "Light theme" : "Dark theme"}
        >
          {theme === "dark" ? <IconSun /> : <IconMoon />}
        </button>
        <button className="ghost" onClick={onSettings}>
          Settings
        </button>
        <button className="ghost" disabled={!canPackage} onClick={onPackage} title="Suggest titles and thumbnails from the trimmed cut">
          Package
        </button>
        <button className="solid" disabled={!canExport} onClick={onExport}>
          Export
        </button>
        <button className="ghost" onClick={onQuit} title="Quit (⌘Q)">
          Quit
        </button>
      </div>
    </header>
  );
}

function IconMoon() {
  return (
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z" />
    </svg>
  );
}

function IconSun() {
  return (
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </svg>
  );
}
