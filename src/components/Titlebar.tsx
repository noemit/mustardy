import { native, platform } from "../lib/bridge";
import type { BrainStatus } from "../lib/gemma";
import type { EyesStatus } from "../lib/vision";
import type { Provider } from "../types";

type Props = {
  provider: Provider;
  eyes: EyesStatus;
  brain: BrainStatus;
  onOpen: () => void;
  onSave: () => void;
  onTranscript: () => void;
  onSettings: () => void;
  onExport: () => void;
  onQuit: () => void;
  canSave: boolean;
  canTranscript: boolean;
  hasTranscript: boolean;
  canExport: boolean;
};

export function Titlebar({
  provider,
  eyes,
  brain,
  onOpen,
  onSave,
  onTranscript,
  onSettings,
  onExport,
  onQuit,
  canSave,
  canTranscript,
  hasTranscript,
  canExport,
}: Props) {
  const ready = provider === "local" && eyes.state === "ready" && brain.state === "ready";
  const label =
    provider === "kimi" ? "Kimi" : provider === "ollama" ? "Ollama" : ready ? "Local models" : "Local";
  const dot = provider !== "local" ? "" : ready ? "on" : eyes.state === "error" || brain.state === "error" ? "off" : "";
  return (
    <header className={`titlebar ${native && platform === "darwin" ? "" : "web"}`} data-tauri-drag-region>
      <div className="brand">
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
        <button className="ghost" onClick={onSettings}>
          Settings
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
