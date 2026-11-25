# Mustardy

A fast, offline CLI that trims dead air out of video and audio files using FFmpeg.

- **Audio-envelope pause detection** (20 ms RMS windows, streamed in O(1) RAM) with automatic noise-floor adaptation and diagnostics.
- **Parallel visual-change protection** — pixel-diffs small frames across every quiet stretch so scene cuts, slide switches, and dissolves are kept intact while static pauses are removed.
- **Fast single-pass / chunked-parallel FFmpeg export** — trims, micro-fades audio joins to eliminate clicks/pops, normalizes speech (`-16 LUFS`), and writes `+faststart` without redundant passes. Supports hardware encoders (`--hw`).
- **Zero-reencode NLE timelines** — export `.fcpxml` (Final Cut Pro / DaVinci Resolve / Premiere) or `.edl` (CMX 3600) in under a second.
- **Audio-only & batch ready** — trims `.wav`, `.mp3`, `.m4a`, `.flac`, and multi-file batches (`--out-dir`, `--in-place`, `--skip-existing`).
- **Agent & script native** — composable `scan` → edit → `apply` plan JSON, `--json` output (including structured JSON errors), and `--progress jsonl`.

## Install

Requires `ffmpeg` and `ffprobe` on `PATH` (or overridden via `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE`).

### From source (Cargo)

```bash
cargo install --git https://github.com/noemit/mustardy
```

### Homebrew (head or tap)

```bash
brew install --HEAD ./Formula/mustardy.rb
```

### Prebuilt releases

Download the tarball for macOS (`aarch64`/`x86_64`) or Linux (`x86_64`/`aarch64`) from [GitHub Releases](https://github.com/noemit/mustardy/releases).

Verify your setup:

```bash
mustardy doctor
```

## Quickstart

```bash
# Default trim: 30 dB drop, 0.9 s min pause, visual-change check, podcast normalization
mustardy trim talk.mp4

# Snappy YouTube pacing using hardware encoding
mustardy trim talk.mp4 --preset tight --hw

# Podcast audio-only trim (preserves .wav / .mp3 / .m4a / .flac)
mustardy trim episode.wav --preset podcast

# Zero-reencode timeline export for DaVinci Resolve / Final Cut / Premiere
mustardy trim talk.mp4 -o talk.fcpxml
mustardy trim talk.mp4 -o talk.edl

# Batch trim a folder of clips
mustardy trim clips/*.mp4 --out-dir trimmed/ --skip-existing

# Combine silence trimming with explicit manual cuts or SRT filler words
mustardy trim talk.mp4 --cut 0:00-0:12 --cut 14:20-end
mustardy trim talk.mp4 --srt talk.srt --cut-words "um,uh,cut that"
```

## Two-Step Workflow (`scan` → `apply`)

Avoid scanning a long recording twice, or let a script/agent edit the cut list before rendering:

```bash
# 1. Scan once and save a reusable plan JSON
mustardy scan talk.mp4 -o plan.json --json

# 2. Inspect or edit plan.json, then render without re-scanning
mustardy apply plan.json -o talk-trimmed.mp4
mustardy apply plan.json -o talk.fcpxml    # or export directly to an NLE timeline
```

## Presets & Pacing Options

| Preset | `--drop` | `--min` | `--pad-before` / `--pad-after` | Normalize |
|---|---|---|---|---|
| `natural` (default) | `30 dB` | `0.90 s` | `0.18 s` / `0.18 s` | on (`0.7`) |
| `tight` | `24 dB` | `0.45 s` | `0.10 s` / `0.08 s` | on (`0.7`) |
| `podcast` | `28 dB` | `0.70 s` | `0.18 s` / `0.14 s` | on (`0.8`) |

Key flags on `mustardy trim`:

- `-o, --output <path>` — output file (default `<input>-trimmed.<ext>`)
- `--format <media|fcpxml|edl>` — output format (also auto-detected from `-o` extension)
- `--drop <dB>` — how far below talking counts as a pause (`8–35`, default `30`)
- `--min <seconds>` — shortest pause to remove (default `0.9`)
- `--pad <seconds>` / `--pad-before` / `--pad-after` — breathing room around speech
- `--keep-edges` — keep edge padding at `0:00` and EOF instead of trimming cleanly
- `--no-visual` — skip visual-change scan
- `--no-normalize` / `--normalize-amount <0..1>` — control `-16 LUFS` podcast normalization
- `--hw` / `--codec <name>` / `--crf <0..51>` / `--encode-preset <name>` — encoder tuning
- `--crossfade <ms>` — audio micro-fade at cut joins (default `8`, `0` to disable)
- `--fast-copy` — keyframe-snapped stream copy for instant drafts
- `--out-dir <dir>` / `--in-place` / `--backup` / `--skip-existing` — batch workflows
- `--dry-run` / `--json` / `--progress <auto|bar|plain|jsonl|none>` / `-q` / `-v`

## Config File

Save your microphone/room defaults in `./.mustardy.toml` or `~/.config/mustardy/config.toml`:

```toml
preset = "tight"
hw = true
crf = 19
crossfade_ms = 8.0
```

CLI flags always override the config file.

## Shell Completions

```bash
mustardy completions zsh  > ~/.zfunc/_mustardy
mustardy completions bash > ~/.local/share/bash-completion/completions/mustardy
mustardy completions fish > ~/.config/fish/completions/mustardy.fish
```

## Agent & Scripting Contract

- `--json` writes structured JSON (schema v1, including `video`, `audio` diagnostics, `cuts`, and `keeps`) to **stdout**; all progress stays on **stderr**.
- Errors with `--json` emit `{"ok": false, "code": "...", "error": "..."}` to stdout.
- `--progress jsonl` streams NDJSON progress events (`{"event":"progress","percent":...}`) on stderr.
- Exit codes: `0` success, `1` runtime/FFmpeg error, `2` usage/invalid arguments, `3` unsupported input (e.g. no audio track), `130` interrupted (`Ctrl-C`).

## Environment Variables

- `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE` — explicit binary paths
- `MUSTARDY_LOG` — optional path for append-only debug log file (off by default)

## Development

```bash
cargo test
cargo clippy --all-targets
cargo build --release
```
