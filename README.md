# Mustardy

Video editor that trims the pauses for you. Tauri desktop app, fully offline —
FFmpeg does the work, nothing leaves the machine.

Opening a video runs an audio silence scan plus a cheap visual-change check,
and applies the trims immediately:

- Silence is measured from the audio itself (RMS windows): only stretches
  dipping well below the floor count, and cuts keep air at each edge, so
  quiet speech and soft word endings aren't trimmed. When the fixed floor
  finds nothing, the scan adapts to the file's own noise floor
- Each quiet stretch is also checked visually — tiny frames across it are
  pixel-diffed, and where the picture changes mid-pause the trim splits
  around the transition: static sides go, the visual change stays
- Sliders re-threshold the scan live (how quiet counts as a pause, minimum
  pause length) without re-reading the file

There is no accept/reject step — zoom the timeline with +/−, drag across it
to add a manual cut, drag a cut to move or resize it, and press Delete (or
the cut tooltip) to remove one.

**Projects:** Save writes a `.mustardy.json` next to (or wherever you pick)
the video — silences, tags, and queued edits. Open accepts that file and
reloads without re-scanning.

**Timeline tags**: press `T` (or the `tag` button) to mark the playhead. Tags
auto-name `A45`, `B34`, … (letter + whole seconds). Click a tag to seek;
right-click to remove.

**Export** bakes the kept segments with FFmpeg, with optional podcast-style
audio normalization.

Runs on macOS (Apple Silicon) and Linux.

## Develop

Prerequisites: Node, plus a Rust toolchain (`curl https://sh.rustup.rs -sSf | sh`;
on Debian/Ubuntu also `sudo apt install build-essential libwebkit2gtk-4.1-dev`).

```bash
npm install
npm run prep        # stages ffmpeg/ffprobe sidecars into src-tauri/binaries
npm run dev         # vite + tauri
```

`npm run dev:ui` still gives you the plain-web UI (no native sidecars there).

## Logs

Everything the core does is appended to `mustardy.log` in the repo root
(timestamped; stderr keeps a copy too; truncated past 2 MB). Override the
path with `MUSTARDY_LOG=/some/file.log`.

## Package

```bash
npm run pack:mac    # or pack:linux
```

macOS builds are signed with the Developer ID in
`src-tauri/tauri.conf.json`; set `APPLE_ID`, `APPLE_PASSWORD` (app-specific),
and `APPLE_TEAM_ID` to also notarize.

## Layout

- `src/` — React UI (video stage, timeline)
- `src-tauri/` — Tauri shell (`src/main.rs`, commands + events only)
- `src-tauri/core/` — `mustardy-core`: ffmpeg probe / audio-envelope scan /
  visual-change check / export. No Tauri deps; builds headlessly:
  `cargo build -p mustardy-core`
- `scripts/prep.mjs` — sidecar staging

## Knobs

- `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE` — binary overrides
- `MUSTARDY_LOG` — log file path
