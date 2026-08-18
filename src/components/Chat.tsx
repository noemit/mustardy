import { useEffect, useRef, useState } from "react";
import type { ChatMessage } from "../types";

const HINTS = [
  "make it youtube",
  "panic shake then smash to text “NOPE”",
  "slow-mo the reaction",
  "trim silences",
];

type Props = {
  messages: ChatMessage[];
  busy: boolean;
  activity: string | null;
  disabled: boolean;
  onSend: (text: string) => void;
};

export function Chat({ messages, busy, activity, disabled, onSend }: Props) {
  const [text, setText] = useState("");
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    end.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, busy, activity]);

  function submit() {
    const next = text.trim();
    if (!next || busy || disabled) return;
    setText("");
    onSend(next);
  }

  return (
    <section className="chat">
      <div className="panel-h">
        <strong>Chat</strong>
        {busy ? <span className="busy">thinking</span> : <span />}
      </div>
      <div className="msgs">
        {messages.map((m) =>
          m.kind === "divider" ? (
            <div key={m.id} className="msg-divider" role="separator">
              <span>{m.text}</span>
            </div>
          ) : (
            <div key={m.id} className={`msg ${m.role}`}>
            <span className="msg-role">{m.role === "user" ? "you" : "mustardy"}</span>
            <span className="msg-text">
              {m.icon === "check" && (
                <svg
                  className="msg-check"
                  viewBox="0 0 24 24"
                  width="13"
                  height="13"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="3"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              )}
              {m.text}
            </span>
            </div>
          )
        )}
        {activity && (
          <div className="msg assistant activity">
            <span className="msg-role">mustardy</span>
            <span className="msg-text">
              <span className="spinner" aria-hidden="true" />
              {activity}
            </span>
          </div>
        )}
        <div ref={end} />
      </div>
      <div className="suggest">
        {HINTS.map((h) => (
          <button key={h} disabled={disabled || busy} onClick={() => onSend(h)}>
            {h}
          </button>
        ))}
      </div>
      <form
        className="composer"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <textarea
          value={text}
          disabled={disabled}
          placeholder={disabled ? "Open a video first" : "Ask for an edit"}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
        />
        <button className="solid" disabled={disabled || busy || !text.trim()} type="submit">
          Send
        </button>
      </form>
    </section>
  );
}
