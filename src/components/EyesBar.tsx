import type { EyesStatus } from "../lib/vision";

/** Full-width status banner while models download or load. Big, calm,
 * honest: what is happening, how far along, and a real progress track. */
export function EyesBar({ status }: { status: EyesStatus }) {
  if (status.state === "ready" || status.state === "idle") return null;
  return (
    <div className={`eyesbar ${status.state}`} role="status">
      <span className="eyesbar-label">{status.label}</span>
      <div className="eyesbar-track">
        <div
          className="eyesbar-fill"
          style={{ width: `${Math.max(3, Math.min(100, status.progress))}%` }}
        />
      </div>
      <span className="eyesbar-pct">{Math.round(status.progress)}%</span>
    </div>
  );
}
