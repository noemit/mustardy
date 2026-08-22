---
name: published-the-video
description: >
  Update the YouTube 100-day content plan after the user publishes a video.
  Use when the user says they published, uploaded, or shipped a video, or runs
  /published-the-video. Marks the matching day in youtube/plan/100-day-plan.html
  as published (video ID, final title), keeps its prediction for later
  comparison, moves displaced/unused topics into "Ideas on ice", and reports
  prediction vs actual when fresh analytics exist.
  Triggers: /published-the-video, published the video, video is live, uploaded a video.
---

# Published the Video

Updates the content plan at `/workspace/agent/youtube/plan/100-day-plan.html`.
All plan data lives in that file inside `<script id="plan-data" type="application/json">` as a JSON object: `{ "startDate", "videos": [...], "icebox": [...] }`. The page renders from this JSON via JS — **edit only the JSON block**, never the rendered markup.

## Workflow

1. **Identify the video.** Ask for (or get from context): final title, YouTube video ID/URL, publish date. If the user gives a URL, extract the 11-char video ID. If the user says the video "is in the youtube folder", look for the newest recording (e.g. `*.mov`/`*.mp4`) — but still confirm title and publish status before logging.

2. **Propose titles (if not yet finalized).** If the user hasn't picked a final title — or asks for proposals — review the raw recording first: extract frames with ffmpeg (`fps=1/60` is enough) and read them to identify the demoed tool/app/topic. Then propose 3–5 titles in the channel's proven styles (see `/workspace/agent/youtube/AGENTS.md`): vibecode builds anchor on a **known product** ("Vibecode a Spotify clone..."), cost hooks ("...for $0.38"), first-impressions ride model releases, roasts are direct. Include the planned title from the calendar as one option if the video matches a planned slot. Let the user pick or edit before logging — titles can change during this workflow.

2. **Match to a planned entry** in `videos` (fuzzy match on title/topic):
   - Set `"status": "published"`, add `"videoId"`, and if the final title differs from the plan, replace `"title"` with the actual published title.
   - **Keep the `"prediction"` object untouched** — it is compared against actual performance later.
   - Remove `"userIdea": true` is NOT needed; keep it as provenance.

3. **If nothing on the plan matches** (unplanned video):
   - Insert/replace the entry for the actual publish date: mark it published with videoId and actual title, set `"prediction": null`, and if another planned topic occupied that slot, **move the displaced topic to `icebox`** with a note like "bumped by <published title> on <date>" — never delete a topic outright.

4. **If the user says a planned topic was skipped/scrapped**, move it to `icebox` with a note, and optionally pull a topic from `icebox` into a later open slot if the user asks.

5. **Renumber days if dates shift.** Day N date = `startDate` + (N-1) days. Only renumber if the user changed the cadence (missed days, breaks); ask first.

6. **Prediction check (when analytics exist).** If new CSVs have appeared in `/workspace/agent/youtube/analysis/` since the video published, compare its 7-day views to the prediction range and report hit/miss. If a format has clearly over/under-performed predictions across 3+ videos, suggest re-balancing the plan (swap upcoming slots accordingly, with the user's confirmation).

7. **Verify the file still parses** after editing: extract the JSON block and run it through `python3 -m json.tool`.

8. Report back: which day was marked published, its prediction (if any), and what changed (icebox moves, renumbering).
