# Mustardy — notes (Aug 2026)

Working notes from the stretch that turned this from “small model tries to edit video” into “deterministic editor with optional transcript.” For the next person (or future us).

## What actually works

`trim silences` does **not** use AI. Load a video → audio RMS scan (~3s) + pixel-diff of tiny 64×36 frames across each pause (~10s). That’s it. Ready to trim in ~15s.

Whisper is **optional**. It unlocks:

- the script / review pane
- “trim after I say thanks for watching”
- Gling-style “select words → Delete → cut”

On this Mac, tiny.en took ~2 min for a 19-min file and ~3 min for a 21-min file. Do not ship “3 minutes” as a universal number. Show a range, then ETA from the first chunk’s realtime factor, and remember last RTF in settings.

## Models (local)

| Role | Model | When it runs |
|---|---|---|
| Ears | whisper tiny.en (default) or small.en | Only if the user transcribes |
| Brain | SmolLM2-1.7B GGUF | Only if a chat request matches nothing deterministic |
| Eyes | SmolVLM-256M GGUF | Only to snap a *brain*-proposed cut, not silence trims |

HF:

- https://huggingface.co/ggerganov/whisper.cpp (`ggml-tiny.en.bin`, `ggml-small.en.bin`)
- https://huggingface.co/HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF
- https://huggingface.co/ggml-org/SmolVLM-256M-Instruct-GGUF

Pixel-diff ≠ Eyes. Diff catches “the picture changed” (slide, hard cut). Eyes would catch “this pause is interesting” (a gesture). We don’t do the latter on silence trims.

## Chat should stay dumb on purpose

The word `trim` used to hijack every request into “queue all silences.” Don’t put generic verbs in that regex. Named times (`17:07`), `@a45` tags, and “after I say X” are parsed in the app — never ask SmolLM2 to copy timestamps.

Planner order: deterministic cuts → keyword `localPlan` → brain last.

## Word-level editing

tiny.en word times are often 100–400ms off. Fine for finding a phrase. Bad for cutting a single word without clipping the next one. **small.en** (Settings) is the right default if people will edit from the script. Slower, ~466MB, downloads on first use. Re-transcribe after switching.

Select words in the transcript, Delete or “cut words.” Timeline hover (and playhead-in-range) also has ✓/✕ so you can review while watching. No transcript → old timestamp cards.

## Ears SIGKILL

`mustardy-ears` sometimes dies in <15ms with signal 9 and empty stderr. Same binary sometimes works. Two likely causes: macOS Jetsam (OOM — full PCM + DTW on a 20-min file) or Gatekeeper killing the staged sidecar. Rebuild locally:

```
cargo build -p mustardy-ears-cli --release
MUSTARDY_EARS="$PWD/src-tauri/target/release/mustardy-ears" npm run dev
```

If it keeps dying: chunk long audio. Don’t load 20 minutes of f32 + DTW in one process.

## Export

Used to re-encode every keep-segment (90 cuts → ~91 x264 jobs → minutes). Cuts-only now one ffmpeg pass: trim + concat + encode once. Overlays/pans/fx still use the old per-segment path. Export emits `export-progress` events (segment encodes → join → faststart → normalize) and logs every 5% so long exports aren't a silent one-liner.

## Preview replay

Preview is two `<video>` elements ping-ponging so a cut never seeks the audible player. The failure mode is standby not being decoded exactly on the cut frame; the old 150 ms "close enough" check made joins smear. Keep edges are now snapped up to the source fps (same frame grid the CFR export lands on), standby readiness requires `HAVE_CURRENT_DATA` within ¾ frame, preroll decodes at 1× so high-speed previews don't overshoot, and a missed preroll hard-seeks after crossing the cut edge instead of playing through the whole gap. Preview audio is native element audio again — the Web Audio compressor path drifted at 1.5×/2×, so Normalize is export-only. HTML media still isn't sample-accurate; if preview needs to be perfect, the next step is WebCodecs/MP4 demuxed frames, not more `<video>` tuning.

## Projects

Save → `.mustardy.json` (video path, silences, transcript, tags, edits). Open accepts that or a video. Reload skips the scan. Transcript export is `.txt` or `.srt`.

## If we add cloud later

Only ears would get faster. Groq/Deepgram/OpenAI would turn a 3-min local transcribe into ~20–60s including upload, and word times would be better than tiny.en. Silence + visual stay local. Not worth it for the edit/preview loop.

## Don’t do next without a reason

- Don’t warm Eyes/Brain at startup (RAM fights whisper).
- Don’t caption 5 frames on open.
- Don’t auto-transcribe on open.
- Don’t let the 1.7B brain invent cut times.

## Worth doing later

- Chunked / streaming whisper + live “4:12 of 20:00” + ETA from first chunk.
- Accept-all silence trims as one decision instead of 90 chips (timeline hover helps; the rail is still noisy).
- Eyes-on-pauses only if pixel-diff misses real “interesting” motion.
- Cut-only export is one pass; still re-encodes. Stream-copy is faster but snaps to keyframes — probably wrong for 0.5s silence cuts.
