import { useState } from "react";
import type { Settings } from "../types";

type Props = {
  settings: Settings;
  models: string[];
  kimiSaved: boolean;
  onClose: () => void;
  onSave: (next: Settings) => void;
};

export function SettingsModal({ settings, models, kimiSaved, onClose, onSave }: Props) {
  const [draft, setDraft] = useState(settings);

  function set<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDraft((d) => ({ ...d, [key]: value }));
  }

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>How Mustardy thinks</h2>
        <div className="field">
          <label>Engine</label>
          <select
            value={draft.provider}
            onChange={(e) => set("provider", e.target.value as Settings["provider"])}
          >
            <option value="local">Bundled models</option>
            <option value="kimi">Kimi</option>
            <option value="ollama">Ollama</option>
          </select>
        </div>
        {draft.provider === "local" && (
          <p className="field-note">Eyes, Whisper, and SmolLM2 ship inside the app. Nothing else is used.</p>
        )}
        {draft.provider === "ollama" && (
          <>
            <div className="field">
              <label>Ollama URL</label>
              <input value={draft.ollamaUrl} onChange={(e) => set("ollamaUrl", e.target.value)} />
            </div>
            <div className="field">
              <label>Ollama model</label>
              <input
                list="qwen-models"
                value={draft.qwenModel}
                onChange={(e) => set("qwenModel", e.target.value)}
              />
              <datalist id="qwen-models">
                {models.map((m) => (
                  <option key={m} value={m} />
                ))}
              </datalist>
            </div>
          </>
        )}
        {draft.provider === "kimi" && (
          <>
            <div className="field">
              <label>Kimi API key</label>
              <input
                type="password"
                value={draft.kimiKey}
                placeholder={kimiSaved ? "Saved on this Mac — type to replace" : "Moonshot / Kimi key"}
                onChange={(e) => set("kimiKey", e.target.value)}
              />
              <p style={{ color: "var(--muted)", fontSize: 12, margin: 0 }}>
                {kimiSaved
                  ? "Stored by the desktop app, not in the browser — typing a new key replaces it."
                  : "Stored by the desktop app, never in the webview."}
              </p>
            </div>
            <div className="field">
              <label>Kimi base URL</label>
              <input value={draft.kimiBaseUrl} onChange={(e) => set("kimiBaseUrl", e.target.value)} />
            </div>
          </>
        )}
        <div className="field">
          <label>Silence floor</label>
          <input value={draft.silenceNoise} onChange={(e) => set("silenceNoise", e.target.value)} />
        </div>
        <div className="field">
          <label>Silence min length (s)</label>
          <input
            type="number"
            min={0.2}
            step={0.05}
            value={draft.silenceMin}
            onChange={(e) => set("silenceMin", Math.max(0.2, Number(e.target.value) || 0.6))}
          />
        </div>
        <p className="field-note">
          A pause counts only where audio dips ~10 dB below the floor for at least the min length,
          and cuts keep ~0.4 s of air at each edge — quiet speech and soft word endings stay.
        </p>
        <div className="field">
          <label>Whisper (transcript)</label>
          <select
            value={draft.whisperModel}
            onChange={(e) => set("whisperModel", e.target.value as Settings["whisperModel"])}
          >
            <option value="tiny.en">tiny.en — faster, looser word times</option>
            <option value="small.en">small.en — slower, better for cutting words (~466 MB)</option>
          </select>
        </div>
        <p className="field-note">
          small.en downloads on first use. Re-transcribe after switching. For cutting individual
          words from the script, small.en is worth the wait.
        </p>
        <div className="row">
          <button className="ghost" onClick={onClose}>
            Cancel
          </button>
          <button
            className="solid"
            onClick={() => {
              onSave(draft);
              onClose();
            }}
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
