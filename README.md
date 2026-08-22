# Mustardy

Agentic video editor. Video on top, chat underneath.

**Tauri desktop app with native inference.** No browser WASM ceiling, no Ollama required.
Models load on demand — nothing is downloaded or kept in memory until an edit
actually needs it:

- **Ears**: whisper.cpp tiny.en (optional small.en) in a sidecar
  (`mustardy-ears`), with DTW word-level timestamps. Optional — skip it for
  silence trims; run it for the script view and word-level cuts
- **Brain**: SmolLM2-1.7B (GGUF) picks edit tools via llama.cpp — it only chooses
  tools, never talks back; the UI writes the message, and generation retries
  until the JSON parses with at least one change. Only spins up when a request
  doesn't match the built-in vocabulary
- **Eyes**: SmolVLM-256M (GGUF) captions frames via llama.cpp's mtmd API — only
  around proposed cut points, to snap them to the exact moment. No up-front
  frame captioning
- **FFmpeg**: probe / silence detection / export, via bundled CLI sidecars.
  Silence is measured from the audio itself (100 ms RMS windows): only
  stretches dipping ~10 dB below the floor count, and cuts keep ~0.4 s of air
  at each edge, so quiet speech and soft word endings aren't trimmed. When
  the fixed floor finds nothing, the scan adapts to the file's own noise
  floor. Each quiet stretch is also checked visually — tiny frames across it
  are pixel-diffed (no model), and where the picture changes mid-pause the
  trim splits around the transition: static sides go, the visual change stays

Deterministic requests never touch a model: "trim silences", "trim everything
after 17:07", "trim after I say thanks for watching", "cut filler words".

**Projects:** Save writes a `.mustardy.json` next to (or wherever you pick)
the video — silences, transcript, tags, and queued edits. Open accepts that
file and reloads without re-scanning. Transcript can be exported as `.txt` or
`.srt` (whispers only when you ask).

Opening a video now only runs the audio silence scan + a cheap visual-change
check. Whisper waits until a request needs words.

**Timeline tags**: press `T` (or the `tag` button) to mark the playhead. Tags
auto-name `A45`, `B34`, … (letter + whole seconds) and chat resolves them:
"trim the video between @a45 and @b34". Click a tag to seek; right-click to
remove.

Runs on macOS (Metal-accelerated) and Linux. Kimi/Ollama remain optional
providers; API keys are stored by the desktop app (`app-data/.kimi-key`,
user-only permissions) and never persisted in webview localStorage.

## Develop

Prerequisites: Node, plus a Rust toolchain (`curl https://sh.rustup.rs -sSf | sh`)
and `cmake` (`brew install cmake`; on Debian/Ubuntu also
`sudo apt install build-essential libwebkit2gtk-4.1-dev`).

```bash
npm install
npm run prep        # downloads models into ./models, stages sidecars
npm run dev         # vite + tauri
```

`npm run dev:ui` still gives you the plain-web UI (no local engines there).

## Logs

Everything the engines do is appended to `mustardy.log` in the repo root
(timestamped; stderr keeps a copy too; truncated past 2 MB). Override the
path with `MUSTARDY_LOG=/some/file.log`. Set `MUSTARDY_DEBUG=1` to also log
the brain's raw generations.

## Package

```bash
npm run pack:mac    # or pack:linux
```

Model files (~1.5 GB) download the first time an engine is actually needed
(progress shown in the bar under the video). They live in
`~/Library/Application Support/studio.mustardy.editor/models` when packaged,
or `./models` in dev.

## Layout

- `src/` — React UI (video stage, timeline, chat, change rail)
- `src-tauri/` — Tauri shell (`src/main.rs`, commands + events only)
- `src-tauri/core/` — `mustardy-core`: ffmpeg port, model registry/downloader,
  eyes/brain engines. No Tauri deps; build and test it headlessly:
  `cargo build -p mustardy-core --release && ./src-tauri/target/release/smoke all demo.mp4`
- `src-tauri/ears-cli/` — whisper sidecar (separate process because llama.cpp
  and whisper.cpp vendor colliding ggml symbols)
- `scripts/prep.mjs` — model + sidecar staging

## Knobs

- `MUSTARDY_BRAIN=smollm2-360m-instruct-q8_0.gguf` — lighter brain (also:
  absolute path to any GGUF chat model)
- `MUSTARDY_MODELS=/path/to/models` — model dir override
- `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE` / `MUSTARDY_EARS` — binary overrides
- `MUSTARDY_DEBUG=1` — log raw brain generations to stderr
