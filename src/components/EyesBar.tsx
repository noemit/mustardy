import type { EyesStatus } from "../lib/vision";

export function EyesBar({ status }: { status: EyesStatus }) {
  if (status.state === "ready" || status.state === "idle") return null;
  return (
    <div className={`eyesbar ${status.state}`}>
      <div className="eyesbar-track">
        <div className="eyesbar-fill" style={{ width: `${status.progress}%` }} />
      </div>
      <span>{status.label}</span>
    </div>
  );
}
