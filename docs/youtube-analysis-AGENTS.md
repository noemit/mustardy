# YouTube Analytics — Agent Workflow

This folder holds monthly YouTube analytics reports. Follow this workflow whenever the user pastes/drops fresh YouTube Studio export data (CSV contents or files) and asks for analysis.

## Directory layout

```
analysis/
├── AGENTS.md                 ← this file
├── YYYY-MM/                  ← one folder per month, e.g. 2026-08 (sortable)
│   ├── data/                 ← raw CSVs for that month's exports
│   │   ├── Chart data.csv
│   │   ├── Table data.csv
│   │   └── Totals.csv
│   └── report-YYYY-MM-DD.html ← one report per run, named by run date
└── YYYY-MM/                  ← next month...
```

Never leave loose CSVs at the `analysis/` root — they always belong in `<month>/data/`.

## Workflow

1. **Month folder.** Determine today's date, create `analysis/YYYY-MM/` (year-month, zero-padded — sorts chronologically) if it doesn't exist. All reports run that month go in that folder.

2. **Data cleanup.** Create `YYYY-MM/data/` and put the raw CSVs there.
   - If the user pasted CSV contents inline (not files), write them to `data/` first (keep original YouTube Studio filenames: `Chart data.csv`, `Table data.csv`, `Totals.csv`), then analyze.
   - If the user dropped files somewhere else, move (not copy) them into `data/`.

3. **Analyze.** Compute at minimum: per-video views, watch hours, avg view duration, retention % (AVD ÷ duration), subs and subs/1k views, impressions, CTR, watch-hours/1k impressions. Group by content format (vibecode build / series / review / roast / tutorial / first-impressions / essay / offbeat). Account for recency bias (videos <3 days old) and small samples before drawing conclusions. Give recommendations per goal: more views, more watch time, more subscribers, monetization.

4. **Report.** Write a self-contained HTML report (no external dependencies) to `YYYY-MM/report-YYYY-MM-DD.html` using today's date in the filename. Match the style/structure of existing reports: summary cards, per-goal recommendations, sortable per-video table, format breakdown charts, caveats section. Verify the HTML parses before finishing.

5. **Cross-update the plan.** After a report, check `../plan/100-day-plan.html`: if a format has clearly over/under-performed its predictions across 3+ videos, suggest re-balancing upcoming slots (with the user's confirmation). The `published-the-video` skill handles per-video plan updates.

## Notes

- Report filenames use the run date, not the data window — multiple reports per month are expected and sort naturally.
- Keep raw CSVs immutable; never edit `data/` contents after writing them.
