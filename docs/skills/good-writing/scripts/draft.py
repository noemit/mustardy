#!/usr/bin/env python3
"""Draft a single writing chunk via DeepInfra (Gemma).

The coding agent splits larger writing tasks into logical chunks and calls
this script once per chunk. The agent then stitches and edits the drafts.

Auth (first match wins):
  DEEPINFRA_API_KEY, DEEPINFRA_TOKEN, DEEP_INFRA_API_KEY

Optional env:
  GOOD_WRITING_MODEL   default: google/gemma-4-31B-it
  DEEPINFRA_BASE_URL   default: https://api.deepinfra.com/v1/openai
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_MODEL = "google/gemma-4-31B-it"
DEFAULT_BASE = "https://api.deepinfra.com/v1/openai"
SKILL_ROOT = Path(__file__).resolve().parent.parent
RULES_PATH = SKILL_ROOT / "references" / "writing-rules.md"


def api_key() -> str:
    for name in ("DEEPINFRA_API_KEY", "DEEPINFRA_TOKEN", "DEEP_INFRA_API_KEY"):
        value = os.environ.get(name, "").strip()
        if value:
            return value
    print(
        "error: missing DeepInfra API key. Set DEEPINFRA_API_KEY on the host "
        "(forwarded into the container by pi-docker/run.sh).",
        file=sys.stderr,
    )
    sys.exit(2)


def load_rules() -> str:
    if not RULES_PATH.is_file():
        print(f"error: writing rules not found at {RULES_PATH}", file=sys.stderr)
        sys.exit(2)
    return RULES_PATH.read_text(encoding="utf-8").strip()


def read_text_arg(value: str | None, path: str | None, stdin_ok: bool) -> str:
    if path:
        return Path(path).read_text(encoding="utf-8").strip()
    if value is not None:
        return value.strip()
    if stdin_ok and not sys.stdin.isatty():
        return sys.stdin.read().strip()
    return ""


def system_prompt(genre: str) -> str:
    rules = load_rules()
    return (
        "You are a first-draft writer for human-facing prose.\n"
        "Write only the requested chunk. Do not add meta commentary, "
        "preambles, or 'here is a draft' wrappers.\n"
        f"Genre/context: {genre}\n\n"
        f"{rules}\n\n"
        "If the brief includes source URLs or /workspace paths, weave them in "
        "as citations. If facts are missing, write clearly around what you "
        "know and mark unknowns as [NEED: ...] rather than inventing."
    )


def chat_complete(
    *,
    model: str,
    base_url: str,
    key: str,
    messages: list[dict[str, str]],
    temperature: float,
    max_tokens: int,
) -> str:
    url = base_url.rstrip("/") + "/chat/completions"
    body = {
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": max_tokens,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {key}",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        print(f"error: DeepInfra HTTP {exc.code}: {detail}", file=sys.stderr)
        sys.exit(1)
    except urllib.error.URLError as exc:
        print(f"error: DeepInfra request failed: {exc}", file=sys.stderr)
        sys.exit(1)

    try:
        content = payload["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError):
        print(
            "error: unexpected DeepInfra response:\n"
            + json.dumps(payload, indent=2)[:2000],
            file=sys.stderr,
        )
        sys.exit(1)

    if not isinstance(content, str) or not content.strip():
        print("error: empty draft from model", file=sys.stderr)
        sys.exit(1)
    return content.strip()


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        description="Draft one writing chunk with DeepInfra Gemma.",
    )
    p.add_argument(
        "--brief",
        help="What this chunk should cover (purpose, audience, tone, must-include points).",
    )
    p.add_argument(
        "--brief-file",
        help="Read the brief from a file instead of --brief.",
    )
    p.add_argument(
        "--context",
        default="",
        help="Facts, quotes, source links, prior section text, or project voice notes.",
    )
    p.add_argument(
        "--context-file",
        help="Read context from a file instead of --context.",
    )
    p.add_argument(
        "--genre",
        default="prose",
        help="Form factor: blog, docs, email, slack, case-study, proposal, website, etc.",
    )
    p.add_argument(
        "--chunk-label",
        default="",
        help="Optional label (e.g. 'section-2-how-it-works') printed on stderr for logs.",
    )
    p.add_argument(
        "--temperature",
        type=float,
        default=0.7,
        help="Sampling temperature (default 0.7).",
    )
    p.add_argument(
        "--max-tokens",
        type=int,
        default=2048,
        help="Max completion tokens (default 2048).",
    )
    p.add_argument(
        "--model",
        default=os.environ.get("GOOD_WRITING_MODEL", DEFAULT_MODEL),
        help=f"DeepInfra model id (default {DEFAULT_MODEL}).",
    )
    p.add_argument(
        "--out",
        help="Write draft to this file (also prints to stdout).",
    )
    p.add_argument(
        "--json",
        action="store_true",
        help="Emit JSON with draft + model metadata instead of plain text.",
    )
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    brief = read_text_arg(args.brief, args.brief_file, stdin_ok=True)
    if not brief:
        print(
            "error: provide --brief, --brief-file, or pipe a brief on stdin",
            file=sys.stderr,
        )
        return 2

    context = read_text_arg(args.context or None, args.context_file, stdin_ok=False)
    user_parts = [f"## Brief\n{brief}"]
    if context:
        user_parts.append(f"## Context\n{context}")
    user_parts.append(
        "## Output\nReturn only the prose for this chunk, ready for the editor."
    )
    user_content = "\n\n".join(user_parts)

    if args.chunk_label:
        print(f"drafting chunk: {args.chunk_label}", file=sys.stderr)

    base_url = os.environ.get("DEEPINFRA_BASE_URL", DEFAULT_BASE)
    draft = chat_complete(
        model=args.model,
        base_url=base_url,
        key=api_key(),
        messages=[
            {"role": "system", "content": system_prompt(args.genre)},
            {"role": "user", "content": user_content},
        ],
        temperature=args.temperature,
        max_tokens=args.max_tokens,
    )

    if args.out:
        out_path = Path(args.out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(draft + "\n", encoding="utf-8")
        print(f"wrote {out_path}", file=sys.stderr)

    if args.json:
        print(
            json.dumps(
                {
                    "model": args.model,
                    "genre": args.genre,
                    "chunk_label": args.chunk_label or None,
                    "draft": draft,
                },
                ensure_ascii=False,
                indent=2,
            )
        )
    else:
        print(draft)

    return 0


if __name__ == "__main__":
    sys.exit(main())
