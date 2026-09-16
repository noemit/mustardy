import { useState } from "react";
import { DEFAULT_SILENCE_DROP, DEFAULT_SILENCE_MIN } from "../lib/silence";

type Props = {
  onOpenEditor: () => void;
  onDropFile: (file: File) => void;
};

const commands = [
  "mustardy trim talk.mp4",
  "mustardy trim talk.mp4 --json",
];

export function LandingHero({ onOpenEditor, onDropFile }: Props) {
  const [copied, setCopied] = useState<number | null>(null);

  async function copyCommand(command: string, index: number) {
    await navigator.clipboard.writeText(command);
    setCopied(index);
    window.setTimeout(() => setCopied((current) => (current === index ? null : current)), 1600);
  }

  return (
    <main
      className="landing"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        const file = event.dataTransfer.files[0];
        if (file) onDropFile(file);
      }}
    >
      <div className="landing-inner">
        <div className="landing-lockup">
          <div className="landing-wordmark">mustardy</div>
          <h1 id="landing-title">An app for you. A command for your agent.</h1>
        </div>

        <div className="landing-grid">
          <section className="landing-human" aria-labelledby="landing-title">
            <p className="landing-lede">
              Trim pauses from spoken video offline, without sending the file anywhere.
            </p>
            <button className="solid landing-cta" onClick={onOpenEditor}>
              Open the editor
            </button>
            <p className="landing-drop-note">Or drop a video anywhere.</p>

            <ul className="proofs" aria-label="How Mustardy works">
              <li>Offline</li>
              <li>No upload</li>
              <li>FFmpeg</li>
              <li>{DEFAULT_SILENCE_DROP} dB / {DEFAULT_SILENCE_MIN} s defaults</li>
            </ul>
          </section>

          <section className="terminal-card" aria-label="Mustardy command line examples">
            <header className="terminal-head">
              <span className="terminal-dots" aria-hidden="true"><i /><i /><i /></span>
              <span>terminal</span>
            </header>
            <div className="terminal-body">
              <p className="terminal-intro">Give an agent one command and get a trimmed file back.</p>
              {commands.map((command, index) => (
                <div className="command-row" key={command}>
                  <code><span aria-hidden="true">$ </span>{command}</code>
                  <button
                    className="command-copy"
                    onClick={() => void copyCommand(command, index)}
                    aria-label={`Copy ${command}`}
                  >
                    {copied === index ? "Copied" : "Copy"}
                  </button>
                </div>
              ))}
              <pre className="json-sample" aria-label="Sample JSON result"><code>{`{
  "output": "talk-trimmed.mp4",
  "removed": 18.4,
  "dropDb": ${DEFAULT_SILENCE_DROP},
  "minPause": ${DEFAULT_SILENCE_MIN}
}`}</code></pre>
              <p className="terminal-local">Nothing is uploaded. FFmpeg runs on your machine.</p>
            </div>
          </section>
        </div>
      </div>
    </main>
  );
}
