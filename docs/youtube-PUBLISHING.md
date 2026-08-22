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

1. **Find candidate recordings.** List the newest `.mov`/`.mp4` in `/workspace/agent/youtube/` (root and `videos/`, by mtime) with filename, date, duration (ffprobe), size. **If the user already named a specific file, that is check-in 1** — one-line confirm (name, duration, size) and go straight to step 3. Do not list every recording and wait.

2. **✋ CHECK-IN 1 — confirm the video.** Only if they did not name a file. Ask which one. Do NOT extract frames or assume the newest file is right — raw recordings, trimmed exports, and stray clips all look alike from the filesystem.

3. **Review the confirmed file — frames AND full transcript.** Say one line first ("transcribing 8 min…") so the user is not staring at silence. Extract frames with ffmpeg (`fps=1/45`, ~1280w, into `/tmp/opencode/...`) and read a spread. **Also transcribe the full audio** (extract wav, `faster-whisper` `base` in this container) and **read the whole transcript before proposing any titles.** Frames show the screen; the talk is often a different story. Identify what's demoed, tools/models shown, the narrative arc, and meta hooks.

4. **✋ CHECK-IN 2 — lock the title.** Propose 5–7 titles **only after the transcript is read**, using the channel's proven styles (see `analysis/` findings): known-product anchors for builds, cost hooks, fresh-model first impressions, direct roasts, meta loops. Include the planned title from `plan/100-day-plan.html` if it matches a slot. Wait for the user's pick.

5. **✋ CHECK-IN 3 — lock the subtitle (thumbnail text).** Propose 3–5 short options (2–6 words, readable at tiny size, complements — never repeats — the locked title). Wait for the user's pick.

6. **✋ CHECK-IN 4 — lock the thumbnail.** Build `youtube/videos/<slug>/<slug>-thumbnail.html`: **20 concept cards** at exactly 1280×720 (lettered A–T, one marked RECOMMENDED) using the locked title + subtitle, with full-res frame stills in `<slug>/assets/`. **Every text block, portrait, stamp, and emoji is `.tweak` — drag to move, corner handle to resize** — so the user can nudge before Cmd+Shift+4. Mix layouts that actually perform on YouTube:
   - **Centered text more often than left-stacked.** Big 2–4 words, high contrast, readable at ~160px wide.
   - **Angle some text** (−8° to −15°) — the slightly crooked look reads as energy, not a slide deck.
   - **At least one VS / split card** (LOCAL vs CONTAINER, $59 vs $0, before/after). Famous pattern: two sides, red X vs green check, or a fat “VS” in the middle.
   - **At least one minimalist card: a single strong screencap, no text, no portrait.** Pick the frame that already tells the story (the UI, the 404, the live site).
    - **At least 1–2 cards use cutout portraits** from `youtube/portraits/` (`center.png` facing camera, `right.png` looking/pointing right, `shh.png` finger-to-lips). Copy the ones you use into `<slug>/assets/`. Match pose to the hook (`shh` = secrets/safety, `right` = text on the left).
    - **Brand logos.** If the user asks for a logo, or the video is a known-product first look (DeepSeek, Grok, OpenChamber, …), download it (Simple Icons / Wikimedia) into `<slug>/assets/` and put it on the **RECOMMENDED card, the VS card, and a few others**. Do not recommend a concept that ignores an explicit logo request. (DeepSeek Vision, Aug 21: user asked for the whale; H / BLIND vs VISION was locked and had no logo.)
   Render-verify with Playwright in `youtube/assets/intro/` before showing. User screenshots the locked card on the Mac (Cmd+Shift+4).

7. **Build the package** in `youtube/videos/<slug>/`:
   - `<slug>-youtube-description.md` following the exemplar (`videos/grok46-coloringbook/`) — header block (source file, length, tools shown, locked title, locked thumb text), suggested titles (locked first), copy-paste description in Em's voice (narrative + links + outline + timestamps), chapters block (starts `0:00`, ≥10s gaps), tags, thumbnail notes (locked concept), pinned comment idea ending in an engagement question. Links: https://noemititarenco.com/about · https://x.com/NoemiTitarenco. Never invent links.
    - **`<slug>-youtube-description.html`** — same content, easier to read and copy. Dark page, one card per block (description / chapters / tags / pinned comment), each with a **Copy** button. After writing it, give the user `http://localhost:8080/youtube/videos/<slug>/<slug>-youtube-description.html` (not just the .md).
    - **Move media out of the youtube root** into `videos/<slug>/`: the publish file, its matching raw `.mov`, mustardy sidecars (`.json`), and same-day false starts. Root should stay docs + folders only (`AGENTS.md`, `PUBLISHING.md`, `analysis/`, `plan/`, `portraits/`, `assets/`, `videos/`, `old videos/`).

8. **Plan check.** Mention the prediction on record if the video matches a planned slot in `plan/100-day-plan.html`. Scheduling for tomorrow is the same package — do not wait for "it's live" to finish steps 1–8.

9. **After upload** (user does it on the Mac), run the `published-the-video` skill: log status/title/video ID into the plan, icebox any displaced topic.

## House style

- Voice: honest, builder-first, first person, concrete over hype. No guru-speak.
- Titles can change any time before upload; the locked value is whatever the user last confirmed.
- Timestamps must come from a transcript of the **publish file** (the mustardy export), never the raw recording. Remind the user to nudge ± a few seconds in the upload preview.

## Description rules (learned from Em's edits, Aug 16 2026; roast cut Aug 21)

1. **Never repeat the title as the first line of the description.** The first ~100 chars show before "…more" — lead with the hook and, when the thing built is live, the link to the live result (e.g. cuberry.com).
2. **Cross-link related previous videos.** Pull videoIds from `plan/100-day-plan.html` and link them (with `&t=` timestamps when pointing at a specific moment). References like "the coloring book we made" should be clickable.
3. **Talk to the viewer, not about the video.** A CTA beats a "who this is for" lecture ("If you want a roast, send the product…"). Keep the middle to 1–2 sentences of what you did. Timestamps carry the outline.
4. **Any number not verifiable from the footage gets a `<CONFIRM: …>` placeholder** (final session cost, durations, counts). Never state a precise figure read off a mid-session frame — the cuberry draft said ~$0.09, the real total was ~$0.34.
5. **No "What you'll see" bullet list** — it duplicates the chapters. Keep chapter labels factual; don't speculate ("(next project?)") about cold opens or content you can't identify.
6. **Don't recap or spoil the video.** Setup only (who sent it, the one thing they asked), then what you did in the blandest terms, then links. No pink hoodie, no credit fail, no verdict, no "five prompts" recap — if it's a chapter, it doesn't belong in the prose. VoxTale roast (Aug 21) draft recapped the face gag, OpenRouter dying, and the 20-card skill. Em cut the body to: landing-page setup → "I signed in and ran yesterday's video through it, checking out the UX and output." → "I also show and compare to my skill that I use for my videos." → CTA + links.

