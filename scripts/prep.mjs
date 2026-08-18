// Prepares everything the native app needs:
//  1. Downloads GGUF/ggml models into <repo>/models/ (dev cache; the app also
//     downloads them itself on first run).
//  2. Stages sidecar binaries into src-tauri/binaries/ for `tauri build`:
//     ffmpeg/ffprobe (from the ffmpeg-static/ffprobe-static npm packages) and
//     mustardy-ears (built from src-tauri/ears-cli).
//
// Usage: node scripts/prep.mjs [--skip-models] [--skip-sidecars]

import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const modelsDir = path.join(root, "models");
const binDir = path.join(root, "src-tauri", "binaries");

const args = new Set(process.argv.slice(2));
const skipModels = args.has("--skip-models");
const skipSidecars = args.has("--skip-sidecars");

const MODELS = [
  ["https://huggingface.co/ggml-org/SmolVLM-256M-Instruct-GGUF/resolve/main/SmolVLM-256M-Instruct-Q8_0.gguf", "SmolVLM-256M-Instruct-Q8_0.gguf"],
  ["https://huggingface.co/ggml-org/SmolVLM-256M-Instruct-GGUF/resolve/main/mmproj-SmolVLM-256M-Instruct-f16.gguf", "mmproj-SmolVLM-256M-Instruct-f16.gguf"],
  ["https://huggingface.co/HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF/resolve/main/smollm2-1.7b-instruct-q4_k_m.gguf", "smollm2-1.7b-instruct-q4_k_m.gguf"],
  ["https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin", "ggml-tiny.en.bin"],
];

const TRIPLES = {
  darwin: { arm64: "aarch64-apple-darwin", x64: "x86_64-apple-darwin" },
  linux: { arm64: "aarch64-unknown-linux-gnu", x64: "x86_64-unknown-linux-gnu" },
  win32: { x64: "x86_64-pc-windows-msvc" },
};

function triple() {
  return TRIPLES[process.platform]?.[process.arch];
}

async function download(url, dest) {
  if (fs.existsSync(dest)) {
    console.log("  ✓", path.basename(dest), "already here");
    return;
  }
  console.log("→", path.basename(dest));
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  const total = Number(res.headers.get("content-length") || 0);
  const tmp = `${dest}.part`;
  const out = fs.createWriteStream(tmp);
  let done = 0;
  const reader = res.body.getReader();
  for (;;) {
    const { done: fin, value } = await reader.read();
    if (fin) break;
    out.write(value);
    done += value.length;
    if (total) process.stdout.write(`\r   ${Math.round((100 * done) / total)}%   `);
  }
  await new Promise((r, j) => (out.close ? out.close(r) : out.end(r)).on("error", j));
  fs.renameSync(tmp, dest);
  console.log("\r   done      ");
}

function stage(name, src) {
  const t = triple();
  if (!t) throw new Error(`no target triple for ${process.platform}/${process.arch}`);
  const suffix = process.platform === "win32" ? ".exe" : "";
  const dest = path.join(binDir, `${name}-${t}${suffix}`);
  fs.copyFileSync(src, dest);
  fs.chmodSync(dest, 0o755);
  console.log("  ⇢", path.relative(root, dest));
}

if (!skipModels) {
  fs.mkdirSync(modelsDir, { recursive: true });
  console.log("Models →", path.relative(process.cwd(), modelsDir));
  for (const [url, file] of MODELS) await download(url, path.join(modelsDir, file));
}

if (!skipSidecars) {
  fs.mkdirSync(binDir, { recursive: true });
  const require = createRequire(import.meta.url);
  console.log("Sidecars →", path.relative(process.cwd(), binDir));
  stage("ffmpeg", require("ffmpeg-static"));
  stage("ffprobe", require("ffprobe-static").path);

  // Build + stage the whisper sidecar
  console.log("Building mustardy-ears sidecar (cargo release)…");
  const cargo = spawnSync("cargo", ["build", "-p", "mustardy-ears-cli", "--release"], {
    cwd: path.join(root, "src-tauri"),
    stdio: "inherit",
  });
  if (cargo.status !== 0) process.exit(cargo.status ?? 1);
  const suffix = process.platform === "win32" ? ".exe" : "";
  stage("mustardy-ears", path.join(root, "src-tauri", "target", "release", `mustardy-ears${suffix}`));
}

console.log("Done. Run npm run dev or npm run pack:mac.");
