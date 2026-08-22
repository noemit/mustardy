---
name: publish-video
description: >
  Package the user's latest YouTube recording for upload. Use when the user says
  "lets publish", "let's publish", "publish the video", or /publish-video.
  The canonical workflow doc is /workspace/agent/youtube/PUBLISHING.md — read and
  follow it. Four user check-ins are mandatory: confirm the video, lock the title,
  lock the subtitle (thumbnail text), lock the thumbnail concept. After upload,
  use the published-the-video skill to log it into the 100-day plan.
  Triggers: /publish-video, lets publish, let's publish, publish this video.
---

# Publish Video ("lets publish")

**Read and follow `/workspace/agent/youtube/PUBLISHING.md` — that file is the
canonical, user-editable workflow.** The summary below must match it; if they
disagree, PUBLISHING.md wins.

## The 4 mandatory user check-ins (never skip or batch)

1. **🎬 Video** — list candidate recordings (filename, date, duration, size); ask which one. No frame extraction before confirmation.
2. **📌 Title** — review the confirmed file (ffmpeg frames + full transcript), then propose 5–7 titles in the channel's proven styles; user locks one.
3. **💬 Subtitle** — propose 3–5 thumbnail-text options (2–6 words, complements the title); user locks one.
4. **🖼 Thumbnail** — build `<slug>-thumbnail.html` (4–6 cards, 1280×720, one RECOMMENDED, frame stills as backgrounds, render-verify with Playwright in `youtube/assets/intro/`); user locks a concept.

Then build the description package in `youtube/videos/<slug>/` per the exemplar
(`videos/grok46-coloringbook/`): both `<slug>-youtube-description.md` **and**
`<slug>-youtube-description.html` (readable page with Copy buttons). Give the
user the HTML via `http://localhost:8080/youtube/videos/<slug>/<slug>-youtube-description.html`.
Check the plan for a matching slot/prediction, and after the user uploads, run
**published-the-video** to log it.

Channel context: `/workspace/agent/youtube/AGENTS.md`.
