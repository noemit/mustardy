// Stages sidecar binaries into src-tauri/binaries/ for `tauri build`:
// ffmpeg/ffprobe from the ffmpeg-static/ffprobe-static npm packages.
//
// Usage: node scripts/prep.mjs

import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const binDir = path.join(root, "src-tauri", "binaries");

const TRIPLES = {
  darwin: { arm64: "aarch64-apple-darwin", x64: "x86_64-apple-darwin" },
  linux: { arm64: "aarch64-unknown-linux-gnu", x64: "x86_64-unknown-linux-gnu" },
  win32: { x64: "x86_64-pc-windows-msvc" },
};

function triple() {
  return TRIPLES[process.platform]?.[process.arch];
}

const ARCH_FLAG = {
  darwin: { arm64: "arm64", x64: "x86_64" },
  linux: { arm64: "aarch64", x64: "x86-64" },
  win32: { x64: "x86-64" },
};

function expectedArchFlag() {
  return ARCH_FLAG[process.platform]?.[process.arch];
}

function archInfo(src) {
  const expected = expectedArchFlag();
  if (!expected) return { ok: true, actual: "" };
  const raw = spawnSync("file", [src], { encoding: "utf8" }).stdout || "";
  const desc = raw.split(": ").slice(1).join(": "); // ignore path prefix
  const actual = desc.match(/(arm64|aarch64|x86_64|x86-64)/i)?.[0] || "unknown";
  return { ok: desc.includes(expected), actual };
}

function stage(name, src) {
  const t = triple();
  if (!t) throw new Error(`no target triple for ${process.platform}/${process.arch}`);
  const suffix = process.platform === "win32" ? ".exe" : "";
  const dest = path.join(binDir, `${name}-${t}${suffix}`);
  const { ok, actual } = archInfo(src);
  if (!ok) {
    const expected = expectedArchFlag();
    console.warn(`  ⚠ skipping ${name}: ${src} is ${actual}, expected ${expected}.`);
    console.warn(`    Put a ${expected} build at ${dest} manually (e.g. \`cp $(brew --prefix ffmpeg)/bin/${name} ${dest}\`).`);
    return;
  }
  fs.copyFileSync(src, dest);
  fs.chmodSync(dest, 0o755);
  console.log("  ⇢", path.relative(root, dest));
}

fs.mkdirSync(binDir, { recursive: true });
const require = createRequire(import.meta.url);
console.log("Sidecars →", path.relative(process.cwd(), binDir));
stage("ffmpeg", require("ffmpeg-static"));
stage("ffprobe", require("ffprobe-static").path);

console.log("Done. Run npm run dev or npm run pack:mac.");
