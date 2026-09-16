# Publishing Workflow — "lets publish"

This is the canonical doc for packaging a video for upload. The `publish-video` skill (installed in every agent) reads and follows this file. Edit here — it lives with your content and syncs back to the skill.

**The golden rule: never guess, always confirm.** There are exactly **4 user check-ins** — one for each creative decision. Do not skip or batch them.

| # | Check-in | Question to user |
|---|----------|------------------|
| 1 | 🎬 Video | "Which recording are we publishing?" |
| 2 | 📌 Title | "Which title do we lock?" |
| 3 | 💬 Subtitle | "Which thumbnail text (subtitle) do we lock?" |
| 4 | 🖼 Thumbnail | "Which thumbnail concept do we lock?" |

## The workflow

1. **Find candidate recordings.** List the newest `.mov`/`.mp4` in `/workspace/agent/youtube/` (root and `videos/`, by mtime) with filename, date, duration (ffprobe), size.

2. **✋ CHECK-IN 1 — confirm the video.** Ask which file is being published. Do NOT extract frames or assume the newest file is right — raw recordings, trimmed exports, and stray clips all look alike from the filesystem. Proceed only after explicit confirmation.

3. **Review the confirmed file — frames AND full transcript.** Extract frames with ffmpeg (`fps=1/45`, ~1280w, into `/tmp/opencode/...`) and read a spread. **Also transcribe the full audio** (extract wav, run STT) and **read the whole transcript before proposing any titles.** Frames show the screen; the talk is often a different story. Identify what's demoed, tools/models shown, the narrative arc, and meta hooks.

4. **✋ CHECK-IN 2 — lock the title.** Propose 5–7 titles **only after the transcript is read**, using the channel's proven styles (see `analysis/` findings): known-product anchors for builds, cost hooks, fresh-model first impressions, direct roasts, meta loops. Include the planned title from `plan/100-day-plan.html` if it matches a slot. Wait for the user's pick.

5. **✋ CHECK-IN 3 — lock the subtitle (thumbnail text).** Propose 3–5 short options (2–6 words, readable at tiny size, complements — never repeats — the locked title). Wait for the user's pick.

6. **✋ CHECK-IN 4 — lock the thumbnail.** Build `youtube/videos/<slug>/<slug>-thumbnail.html`: 4–6 concept cards at exactly 1280×720 (lettered A–E+, one marked RECOMMENDED) using the locked title + subtitle, with full-res frame stills saved to `<slug>/assets/` as backgrounds. Render-verify with the Playwright install in `youtube/assets/intro/` before showing. User screenshots the locked card on the Mac (Cmd+Shift+4).

7. **Build the package** in `youtube/videos/<slug>/`: `<slug>-youtube-description.md` following the exemplar (`videos/grok46-coloringbook/`) — header block (source file, length, tools shown, locked title, locked thumb text), suggested titles (locked first), copy-paste description in Em's voice (narrative + links + outline + timestamps), chapters block (starts `0:00`, ≥10s gaps), tags, thumbnail notes (locked concept), pinned comment idea ending in an engagement question. Links: https://noemititarenco.com/about · https://x.com/NoemiTitarenco. Never invent links.

8. **Plan check.** Mention the prediction on record if the video matches a planned slot in `plan/100-day-plan.html`.

9. **After upload** (user does it on the Mac), run the `published-the-video` skill: log status/title/video ID into the plan, icebox any displaced topic.

## House style

- Voice: honest, builder-first, first person, concrete over hype. No guru-speak.
- Titles can change any time before upload; the locked value is whatever the user last confirmed.
- Timestamps are estimated from frame review — remind the user to nudge them in the upload preview.

## Description rules (learned from Em's edits, Aug 16 2026)

1. **Never repeat the title as the first line of the description.** The first ~100 chars show before "…more" — lead with the hook and, when the thing built is live, the link to the live result (e.g. cuberry.com).
2. **Cross-link related previous videos.** Pull videoIds from `plan/100-day-plan.html` and link them (with `&t=` timestamps when pointing at a specific moment). References like "the coloring book we made" should be clickable.
3. **Talk to the viewer, not about the video.** Include a "who this is for" line ("If you want to dip your feet into vibecoding something practical…"). Keep the middle short — one honest line about mistakes/questions beats a play-by-play; timestamps carry the outline.
4. **Any number not verifiable from the footage gets a `<CONFIRM: …>` placeholder** (final session cost, durations, counts). Never state a precise figure read off a mid-session frame — the cuberry draft said ~$0.09, the real total was ~$0.34.
5. **No "What you'll see" bullet list** — it duplicates the chapters. Keep chapter labels factual; don't speculate ("(next project?)") about cold opens or content you can't identify.
6. **Never add hashtags** to the description. Tags go in the Tags block only, comma-separated, no `#`.

