# Mustardy

Mustardy is a video editor that trims the pauses for you. It's a Tauri
desktop app that runs fully offline. FFmpeg does the work, so nothing
leaves your machine.

## How it works

Open a video and Mustardy runs two scans, then applies the trims right away.

It measures silence from the audio itself using RMS windows. Only stretches
that dip well below the floor get cut. Each cut keeps some air at the edges,
so quiet speech and soft word endings stay. When the fixed floor finds
nothing, the scan adapts to the file's own noise floor (see
`src-tauri/core/src/ffmpeg.rs`).

Each quiet stretch is also checked visually. Small frames across the gap are
pixel-diffed. If the picture changes mid-pause, the trim splits around the
transition. The static parts go, the visual change stays.

Sliders re-threshold the scan live. You can change how quiet counts as a
pause and the minimum pause length without re-reading the file.

There's no accept/reject step. Zoom the timeline with +/−. Drag across it to
add a manual cut, drag a cut to move or resize it, and press Delete (or the
cut tooltip) to remove one.

## Projects

Save writes a `.mustardy.json` next to the video, or wherever you pick. It
stores silences, tags, and queued edits. Open accepts that file and reloads
without re-scanning.

## Timeline tags

Press `T` or the `tag` button to mark the playhead. Tags auto-name like
`A45`, `B34` (a letter plus the whole seconds). Click a tag to seek.
Right-click to remove.

## Export

Export bakes the kept segments with FFmpeg. You can add podcast-style audio
normalization.

Mustardy runs on macOS (Apple Silicon) and Linux.

## CLI

The `mustardy` binary runs headless when you give it a command, so agents and
scripts can trim silences without opening the app:

```bash
mustardy trim talk.mp4                       # -> talk-trimmed.mp4 (30 dB / 0.9 s, normalized)
mustardy trim talk.mp4 -o cut.mp4 --drop 24 --min 0.5 --no-normalize
mustardy trim talk.mp4 --dry-run --json      # report the cuts, write nothing
```

`--drop` is how far below talking a stretch must fall to count as a pause
(8–35 dB). `--min` is the shortest pause to remove, in seconds. Both default to
the same values as the editor and can be overridden per run. Audio
normalization is on by default too; pass `--no-normalize` to leave the audio
untouched. `--json` prints the result as JSON for tooling; progress goes to
stderr so stdout stays clean. `--dry-run` reports without writing, and
`--no-visual` skips the picture-change scan for speed.

On Linux the packaged app already puts `mustardy` on your PATH. On macOS the
app lives in a bundle, so run `mustardy install-cli` once from the binary under
`Mustardy.app/Contents/MacOS/` to symlink it into `/usr/local/bin`.

For CI or a server with no desktop toolchain, `cargo build -p mustardy-core
--bin mustardy-cli` builds the same commands without Tauri or GTK. Run
`mustardy --help` for the full list.

## Develop

You'll need Node and a Rust toolchain. Install Rust with
`curl https://sh.rustup.rs -sSf | sh`. On Debian or Ubuntu also run
`sudo apt install build-essential libwebkit2gtk-4.1-dev`.

```bash
npm install
npm run prep        # stages ffmpeg/ffprobe sidecars into src-tauri/binaries
npm run dev         # vite + tauri
```

`npm run dev:ui` still gives you the plain-web UI without the native
sidecars.

## Logs

Everything the core does is appended to `mustardy.log` in the repo root.
Lines are timestamped, and stderr keeps a copy too. The file is truncated past
2 MB. Override the path with `MUSTARDY_LOG=/some/file.log`.

## Package

```bash
npm run pack:mac    # or pack:linux
```

macOS builds are signed with the Developer ID in
`src-tauri/tauri.conf.json`. Set `APPLE_ID`, `APPLE_PASSWORD`
(app-specific), and `APPLE_TEAM_ID` to also notarize.

## Layout

- `src/` — React UI (video stage, timeline)
- `src-tauri/` — Tauri shell (`src/main.rs`, commands and events only)
- `src-tauri/core/` — `mustardy-core`: ffmpeg probe, audio-envelope scan,
  visual-change check, export, and the headless CLI (`cli.rs` / `silence.rs`).
  No Tauri deps, so it builds headlessly: `cargo build -p mustardy-core`
- `scripts/prep.mjs` — sidecar staging

## Knobs

- `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE` — binary overrides
- `MUSTARDY_LOG` — log file path
