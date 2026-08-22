---
name: good-writing
description: >
  Write clear, human-sounding prose by drafting with DeepInfra Gemma first,
  then editing. Use when writing or editing customer-facing copy, documentation,
  emails, case studies, proposals, blog posts, Slack announcements, website copy,
  or any prose humans will read. Triggers: /good-writing, draft copy, edit prose,
  blog post, help article, changelog, release notes, cold email.
---

# Good Writing

First drafts come from **Gemma on DeepInfra**. You (the coding agent) plan chunks, call the draft script, then edit the result into final prose.

Canonical skill path (inside the container):

```text
/opt/pi-skills/good-writing/
```

Also linked into each agent skill root so Pi, Kimi, Codex, and Grok can discover it.

## When this applies

Any human-facing prose: website copy, docs, emails, case studies, proposals, blog posts, Slack announcements, help articles, changelogs, release notes.

## Required workflow (do not skip)

### 1. Gather inputs

Collect before drafting:

- Audience and goal
- Tone (warm, plain, technical, etc.)
- Hard constraints (length, CTA, legal)
- Facts and quotes to include
- Source links or `/workspace/...` paths for citations
- Project style guide / brand notes if present (those win over defaults)

### 2. Split into logical chunks

Never dump a whole long piece into one model call. Split by natural units, for example:

- Blog: intro, section 1, section 2, ..., close
- Email: subject + preview, body, CTA
- Docs: page intro, procedure steps, troubleshooting
- Slack: headline, body bullets, links

Keep each chunk short enough to stay coherent (roughly one section or ~200–400 words of intended output).

Pass prior-chunk text as `--context` when later chunks must continue voice or avoid repetition.

### 3. Draft each chunk with Gemma

Script (stdlib Python only; no pip install). Resolve the skill directory first:

```bash
SKILL_DIR="${GOOD_WRITING_SKILL_DIR:-/opt/pi-skills/good-writing}"
if [ ! -f "$SKILL_DIR/scripts/draft.py" ]; then
  for d in \
    /root/.kimi-code/skills/good-writing \
    /root/.pi/agent/skills/good-writing \
    /root/.agents/skills/good-writing \
    /root/.grok/skills/good-writing \
    /root/.codex/skills/good-writing; do
    [ -f "$d/scripts/draft.py" ] && SKILL_DIR="$d" && break
  done
fi

python3 "$SKILL_DIR/scripts/draft.py" \
  --genre blog \
  --chunk-label intro \
  --brief "..." \
  --context "..." \
  --out /tmp/good-writing-intro.md
```

Useful flags:

| Flag | Purpose |
|------|---------|
| `--brief` / `--brief-file` | What this chunk must cover |
| `--context` / `--context-file` | Facts, links, prior sections, voice notes |
| `--genre` | `blog`, `docs`, `email`, `slack`, `case-study`, `proposal`, `website`, ... |
| `--out PATH` | Save draft to a file (also prints to stdout) |
| `--json` | Structured output with model metadata |
| `--model` | Override model (default `google/gemma-4-31B-it`) |

Brief on stdin is fine:

```bash
python3 /opt/pi-skills/good-writing/scripts/draft.py --genre email <<'EOF'
Audience: existing Habit users.
Goal: announce weekly reflection export.
Must include: settings path, no price change.
EOF
```

### 4. Edit the drafts yourself

Gemma output is a **first draft**, not the deliverable. You must:

1. Stitch chunks into one piece
2. Fix factual errors and invented details
3. Resolve every `[NEED: ...]` marker (ask the user or look it up)
4. Enforce the writing rules in `references/writing-rules.md`
5. Prefer project voice / style guide over the defaults
6. Add or fix citations (absolute `/workspace/...` paths in-container, full URLs outside)
7. Only then show or write the final prose

### 5. Deliver

Return the final edited prose (or write it to the target file). Do not leave raw multi-chunk Gemma dumps as the user-facing result.

## Credentials

DeepInfra auth is an environment variable (never hardcode, never commit):

- `DEEPINFRA_API_KEY` (preferred)
- also accepted: `DEEPINFRA_TOKEN`, `DEEP_INFRA_API_KEY`

`pi-docker/run.sh` forwards `DEEPINFRA_API_KEY` from the host into the container.

If the key is missing, tell the user to export it on the host (e.g. in `~/.zshrc`) and relaunch. Do not search the filesystem for keys.

Optional:

- `GOOD_WRITING_MODEL` — override default `google/gemma-4-31B-it`
- `DEEPINFRA_BASE_URL` — default `https://api.deepinfra.com/v1/openai`

## Writing rules

Single source of truth:

```text
/opt/pi-skills/good-writing/references/writing-rules.md
```

The draft script injects those rules into Gemma's system prompt. You apply the same rules when editing.

## Fallback

If DeepInfra is down or the key is unset and the user still wants copy now, write the prose yourself using `references/writing-rules.md`, and say that Gemma drafting was skipped.
