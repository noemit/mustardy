# Mustardy npm CLI — spec

Status: draft, agreed to start after the agent-centric site hero ships.
Owner: whoever picks up packaging next.

## Goal

`npx mustardy trim talk.mp4` and `npm i -g mustardy` give anyone the
silence-removal CLI without installing the desktop app. Reuse the existing Rust
core (`mustardy-cli`), prebuild one binary per platform, ship them as npm
packages, and expose an MCP server so an agent can call the trim as a tool.

## Decisions

- Prebuilt binaries through `optionalDependencies` (esbuild-style). No
  postinstall download.
- Reuse the `ffmpeg-static` / `ffprobe-static` packages the desktop app already
  depends on; the launcher sets `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE`.
- The MCP server runs in Node and shells out to the same binary with `--json`,
  so the Rust side stays free of an MCP dependency.
- Names: `mustardy` (main) and `@mustardy/cli-<os>-<arch>` (binaries). Both
  `mustardy` and `@mustardy/cli` are unclaimed on npm as of this writing.

## Layout

Added under `npm/`. Generated platform packages are gitignored.

```
npm/
  mustardy/
    package.json
    bin/mustardy.js        launcher shim
    bin/mcp.js             MCP stdio server
    README.md
  cli-darwin-arm64/        generated: package.json + mustardy binary
  cli-darwin-x64/
  cli-linux-x64/
  cli-linux-arm64/
scripts/build-npm.mjs      build/copy/write packages
scripts/test-npm.mjs       smoke test against a generated clip
.github/workflows/release.yml
```

### Main package.json

```json
{
  "name": "mustardy",
  "version": "0.1.1",
  "description": "Trim the pauses out of a video. Command-line, offline, FFmpeg.",
  "license": "MIT",
  "type": "module",
  "bin": { "mustardy": "bin/mustardy.js" },
  "engines": { "node": ">=18" },
  "files": ["bin", "README.md"],
  "optionalDependencies": {
    "@mustardy/cli-darwin-arm64": "0.1.1",
    "@mustardy/cli-darwin-x64": "0.1.1",
    "@mustardy/cli-linux-x64": "0.1.1",
    "@mustardy/cli-linux-arm64": "0.1.1"
  },
  "dependencies": {
    "ffmpeg-static": "^5.2.0",
    "ffprobe-static": "^3.1.0"
  }
}
```

Platform package versions must match the main version exactly; the release
script writes the version into all of them.

### Platform package.json

```json
{
  "name": "@mustardy/cli-linux-x64",
  "version": "0.1.1",
  "license": "MIT",
  "os": ["linux"],
  "cpu": ["x64"],
  "files": ["mustardy"],
  "preferUnplugged": true
}
```

`os` / `cpu` make npm install only the matching optional dep.
`preferUnplugged` keeps the binary on disk for Yarn PnP users.

## Launcher shim

`bin/mustardy.js` (shebang `#!/usr/bin/env node`):

1. Route `mcp` to `bin/mcp.js`, and turn `install-cli` into a one-line "you're
   already installed" note.
2. Resolve the matching platform package with
   `createRequire(import.meta.url).resolve("@mustardy/cli-<os>-<arch>/mustardy")`.
   If it is missing, exit with an actionable message (unsupported platform, or
   optional deps were skipped — `npm i -g mustardy --include=optional`).
3. Set `MUSTARDY_FFMPEG` from `ffmpeg-static` (a string path) and
   `MUSTARDY_FFPROBE` from `ffprobe-static.path`, only when the user has not set
   them. Guard the null `ffmpeg-static` returns on unsupported platforms.
4. `spawnSync(bin, process.argv.slice(2), { stdio: "inherit", env })`. Exit with
   the child's status, or 1 if it died on a signal.

The Cargo target is `mustardy-cli`; copy and rename it to `mustardy` inside the
platform package, so the command and the packaged file name agree.

## ffmpeg

`ffmpeg-static` downloads the right build at install time; `ffprobe-static`
ships ffprobe in its package. Passing explicit paths means a machine without
ffmpeg on PATH still works, and a user with their own ffmpeg can override with
`MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE`.

Cost: roughly 70–80 MB for ffmpeg, so global installs are heavy but
self-contained. A slim package that requires system ffmpeg can come later.

## Build and publish

No CI exists yet. Add `.github/workflows/release.yml`, triggered by
`workflow_dispatch` and tags matching `v*`.

| runner | platform package |
| --- | --- |
| macos-14 | @mustardy/cli-darwin-arm64 |
| macos-13 | @mustardy/cli-darwin-x64 |
| ubuntu-latest | @mustardy/cli-linux-x64 |
| ubuntu-24.04-arm | @mustardy/cli-linux-arm64 |

Each job:

1. `cargo build --release -p mustardy-core --bin mustardy-cli`
2. `node scripts/build-npm.mjs --os <os> --arch <arch> --binary <path> --version <v>`
3. `npm publish npm/cli-<os>-<arch> --access public --provenance`

A final job publishes `npm/mustardy` once every platform package is up. Auth via
`NPM_TOKEN`, or npm trusted publishing (OIDC) for provenance without a token.

Add `strip = true` to the workspace release profile so the shipped binary is
small (the Tauri app is unaffected).

## macOS signing

npm extraction does not set the `com.apple.quarantine` attribute, so Gatekeeper
does not normally block an npm-installed binary. Signing and notarization are
still worth doing for trust and for anyone who copies the binary elsewhere.

v1: ship unsigned, document the fallback. Follow-up: reuse the Developer ID in
`src-tauri/tauri.conf.json`, `codesign --options runtime`, then `notarytool
submit` on a zip in the macos jobs, using the same `APPLE_ID` /
`APPLE_PASSWORD` / `APPLE_TEAM_ID` secrets as the app.

## MCP server

Command: `mustardy mcp` — JSON-RPC 2.0 over stdio, per the MCP spec. Server name
`mustardy`, version from the package. Use `@modelcontextprotocol/sdk` rather
than hand-rolling framing.

One tool, `trim_silences`:

```json
{
  "type": "object",
  "required": ["input"],
  "properties": {
    "input":     { "type": "string",  "description": "Path to the video file." },
    "output":    { "type": "string",  "description": "Output path. Default: <input>-trimmed.mp4." },
    "dropDb":    { "type": "number", "minimum": 8, "maximum": 35, "description": "How far below talking counts as a pause. Default 30." },
    "minPause":  { "type": "number", "minimum": 0.02, "description": "Shortest pause to remove, in seconds. Default 0.9." },
    "normalize": { "type": "boolean", "description": "Audio normalization. Default true." },
    "dryRun":    { "type": "boolean", "description": "Report the cuts without writing a file." }
  }
}
```

The handler spawns `mustardy trim <input> --json` (adding `--no-normalize` only
when `normalize === false`, and the other flags when present), then returns the
parsed JSON. Errors come back as tool errors, not crashes.

## Local dev and tests

- `node scripts/build-npm.mjs --local` builds the host binary into the matching
  platform package and points the launcher at it, so
  `node npm/mustardy/bin/mustardy.js trim …` works before any publish.
- `scripts/test-npm.mjs` generates a short clip with a known pause, runs trim,
  and asserts the output is shorter and `--json` parses. Extend the existing
  core tests rather than duplicating the algorithm.
- `npm pack --dry-run` in each package to confirm what ships.

## Open questions

1. Name: keep `mustardy`, or make `@mustardy/cli` canonical and `mustardy` an
   alias? Recommendation: `mustardy` for npx ergonomics; scope for platform
   packages.
2. Sign macOS binaries in v1 or later? Recommendation: later, with a documented
   fallback.
3. Add a minimal Node API (`import { trim } from "mustardy"`)? Not in v1 — MCP
   covers the agent case.
4. A Homebrew tap as well? Possibly later; npm first.

## Out of scope for v1

- Windows platform package (the app is macOS/Linux today).
- Bundling ffmpeg outside `ffmpeg-static`.
- Publishing from inside the desktop app bundle.
- Node API.
