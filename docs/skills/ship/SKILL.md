---
name: ship
description: >
  Build and deploy the current project to its hosting platform from inside the
  apple-container harness. Detects Vercel, Cloudflare Workers/Pages, and
  Firebase Hosting from repo files, runs pre-deploy checks (deps, build,
  lint/typecheck when present), deploys, and reports the live URL.
  Use when asked to ship, deploy, push to production, publish, release,
  or make a preview deployment. Triggers: /ship, ship it, deploy this,
  push to prod, publish site, preview deploy.
---

# Ship

Deploy the current repo to production (or a preview) from inside the container. You (the coding agent) run every step yourself — detect the platform, verify the build, deploy, report the URL.

Canonical skill path (inside the container):

```text
/opt/pi-skills/ship/
```

## 1. Detect the platform

Check the repo root, in this order. First match wins unless the user says otherwise:

| Signal files | Platform | Deploy command |
|---|---|---|
| `wrangler.toml` / `wrangler.json` / `wrangler.jsonc` | Cloudflare | `wrangler deploy` (Workers) or `wrangler pages deploy <dir>` (Pages — check for a `pages_build_output_dir` or Pages project) |
| `firebase.json` | Firebase | `firebase deploy` (add `--only hosting` unless the user wants functions/other targets) |
| `vercel.json`, `.vercel/`, `next.config.*`, or `next` in package.json deps | Vercel | `vercel --prod --yes` (production) or `vercel --yes` (preview) |
| Static site only (`index.html`, no build) | Vercel | `vercel --prod --yes` still works for static dirs |

If nothing matches, stop and ask the user where this project deploys — do not guess.

## 2. Pre-flight checks (do not skip)

1. **Auth**: confirm the platform credential exists before deploying:
   - Vercel: `VERCEL_TOKEN` set (auto-injected by the in-image wrapper — plain `vercel` commands work) or `vercel whoami` succeeds (login persists in the `pi-vercel` volume).
   - Cloudflare: `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` set.
   - Firebase: `FIREBASE_TOKEN` set or prior `firebase login` (persists in `pi-firebase-config`).
   - If missing, tell the user exactly which env var to add to `~/.secrets.zsh` on the host and stop.
2. **Deps**: if `package.json` exists and `node_modules/` is missing, install (`npm install`, or `pnpm install`/`bun install` if a pnpm/bun lockfile is present).
3. **Build**: if a `build` script exists, run it. Fix failures before continuing — never deploy a broken build.
4. **Lint/typecheck**: if `lint`, `typecheck`, or `test` scripts exist, run them. If they fail, report and ask whether to proceed or fix first.
5. **Git state**: note uncommitted changes. All three platforms deploy the local working tree, so a dirty tree is deployable — but remind the user to commit before or right after shipping.

## 3. Deploy

- **Production** when the user says ship/deploy/publish/prod; **preview** when they say preview or are just checking.
- Vercel first deploy in an unlinked repo: run `vercel link --yes` first, or let `vercel --yes` create the project. The `.vercel/` dir persists via the repo bind-mount, so linking is one-time.
- Cloudflare Pages: use `wrangler pages deploy <output-dir> --project-name <name>`; get the output dir from the framework build (e.g. `dist`, `.vercel/output/static`, `out`).
- Capture the command output and extract the final URL.

## 4. Report

End with:

- Live URL (production or preview)
- Platform and project name
- Commit hash if the tree was clean
- Anything skipped (failing lint overridden by user, missing env vars for other platforms, etc.)

## Notes

- Kill stale dev servers first if a port is needed for a build-time check: `killports`.
- Do not run `vercel login` / `wrangler login` browser flows — they need a localhost callback that cannot reach the container. Token env vars only.
