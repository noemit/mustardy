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

const installCommand = "/Applications/Mustardy.app/Contents/MacOS/mustardy install-cli";
const agentPrompt = `Use the Mustardy CLI to trim pauses from "/path/to/my-video.mp4". Check mustardy --help first. Use the defaults; don't tune the settings. Preview the cuts with --dry-run --json, then export with --json. Keep my original and don't overwrite an existing output file (choose a new path with -o if needed). Tell me where the trimmed video is and how many seconds were removed.`;

const parameters = [
  ["<input>", "Required", "Path to your video. Quote paths that contain spaces."],
  ["-o, --output <path>", "<input>-trimmed.mp4", "Saves next to the source unless you choose another path."],
  ["--drop <dB>", `${DEFAULT_SILENCE_DROP} dB`, "How far below talking counts as a pause. Range: 8–35 dB."],
  ["--min <seconds>", `${DEFAULT_SILENCE_MIN} s`, "Shortest pause to remove. Minimum: 0.02 seconds."],
  ["--normalize / --no-normalize", "On", "Podcast-style audio normalization. Use --no-normalize to skip it."],
  ["--normalize-amount <0..1>", "0.7", "Normalization strength, from 0 to 1."],
  ["--no-visual", "Visual scan on", "Skips the picture-change scan. Faster, but less precise."],
  ["--dry-run", "Off", "Reports planned cuts without writing a video."],
  ["--json", "Off", "Machine-readable results on stdout. Progress goes to stderr."],
];

export function LandingHero({ onOpenEditor, onDropFile }: Props) {
  const [copied, setCopied] = useState<number | null>(null);
  const [copyError, setCopyError] = useState(false);

  async function copyCommand(command: string, index: number) {
    try {
      await navigator.clipboard.writeText(command);
      setCopyError(false);
      setCopied(index);
      window.setTimeout(() => setCopied((current) => (current === index ? null : current)), 1600);
    } catch {
      setCopyError(true);
    }
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
            <a className="agent-link" href="#agent-guide">Set up your agent ↓</a>

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
              <p className="terminal-local">
                Nothing is uploaded. FFmpeg runs on your machine.{" "}
                <a className="agent-link" href="#agent-guide">Set up your agent ↓</a>
              </p>
            </div>
          </section>
        </div>

        <section className="agent-guide" id="agent-guide" aria-labelledby="agent-guide-title">
          <h2 id="agent-guide-title">Let your agent do the trimming</h2>
          <ol className="agent-steps">
            <li>
              <strong>Install the app.</strong> The CLI ships inside it, so there's nothing extra to
              download.
            </li>
            <li>
              <strong>Put the command on your PATH.</strong> On macOS run <code>mustardy
              install-cli</code> once and it links the binary into <code>/usr/local/bin</code>. On
              Linux the packaged app already puts <code>mustardy</code> on your PATH.
            </li>
            <li>
              <strong>Check it runs.</strong> Ask your agent to run <code>mustardy --help</code>.
            </li>
          </ol>

          <div className="prompt-block">
            <div className="prompt-head">
              <span>Prompt for your agent</span>
              <button
                className="command-copy"
                onClick={() => void copyCommand(agentPrompt, -2)}
                aria-label="Copy the sample agent prompt"
              >
                {copied === -2 ? "Copied" : "Copy"}
              </button>
            </div>
            <pre className="prompt-sample"><code>{agentPrompt}</code></pre>
            <p className="prompt-note">
              Swap in your own file path. Your agent needs terminal access on the same machine as
              the video.
              {copyError ? " If the copy button does nothing, select the text and copy it yourself." : ""}
            </p>
          </div>

          <h3 className="params-title">Command parameters</h3>
          <p className="params-intro">
            The defaults are tuned for spoken video. Most trims need nothing beyond the input path.
          </p>
          <div className="params-table-wrap">
            <table className="params-table">
              <thead>
                <tr>
                  <th scope="col">Parameter</th>
                  <th scope="col">Default</th>
                  <th scope="col">What it does</th>
                </tr>
              </thead>
              <tbody>
                {parameters.map(([name, def, note]) => (
                  <tr key={name}>
                    <td><code>{name}</code></td>
                    <td>{def}</td>
                    <td>{note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="agent-note">
            On macOS, if <code>mustardy</code> isn't on the PATH yet, the full path is{" "}
            <code>{installCommand}</code>.
          </p>
        </section>
      </div>
    </main>
  );
}
