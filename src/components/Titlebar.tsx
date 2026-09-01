import { native, platform } from "../lib/bridge";

type Props = {
  theme: "light" | "dark";
  onToggleTheme: () => void;
  onOpen: () => void;
  onSave: () => void;
  onExport: () => void;
  onQuit: () => void;
  canSave: boolean;
  canExport: boolean;
};

export function Titlebar({
  theme,
  onToggleTheme,
  onOpen,
  onSave,
  onExport,
  onQuit,
  canSave,
  canExport,
}: Props) {
  return (
    <header className={`titlebar ${native && platform === "darwin" ? "" : "web"}`} data-tauri-drag-region>
      <div className="bar-right">
        <button className="ghost" onClick={onOpen}>
          Open
        </button>
        <button className="ghost" disabled={!canSave} onClick={onSave} title="Save project (⌘S)">
          Save
        </button>
        <button
          className="ghost icon"
          onClick={onToggleTheme}
          aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
          title={theme === "dark" ? "Light theme" : "Dark theme"}
        >
          {theme === "dark" ? <IconSun /> : <IconMoon />}
        </button>
        <button className="solid" disabled={!canExport} onClick={onExport}>
          Export
        </button>
        <button className="ghost" onClick={onQuit} title="Quit (⌘Q)">
          Quit
        </button>
      </div>
      <div className="brand">
        <span className="word">mustardy</span>
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
