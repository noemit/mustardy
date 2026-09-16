//! FFmpeg/ffprobe CLI layer — port of electron/ffmpeg.cjs.
//!
//! Binary resolution order (each call):
//! 1. `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE` env vars
//! 2. Tauri sidecar dir: `<exe>/binaries/ffmpeg-<triple>` and plain `<exe>/ffmpeg-*`
//! 3. `src-tauri/binaries/` (dev, via CARGO_MANIFEST_DIR)
//! 4. bare name on PATH

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{AudioEnvelope, Change, CoreError, CoreResult, ExportPayload, ExportProgress, SilenceRange, VideoMeta};

const TRIPLE: &str = {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "aarch64-apple-darwin" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "x86_64-apple-darwin" }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { "aarch64-unknown-linux-gnu" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "x86_64-unknown-linux-gnu" }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "x86_64-pc-windows-msvc" }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    { "unknown" }
};

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn exists(p: &Path) -> bool {
    p.is_file()
}

pub fn resolve_bin(name: &str) -> String {
    let env_key = format!("MUSTARDY_{}", name.to_uppercase());
    if let Ok(v) = std::env::var(&env_key) {
        if !v.is_empty() && exists(Path::new(&v)) {
            crate::log::line(&format!("resolve {name}: env {v}"));
            return v;
        }
    }
    let plain = exe_name(name);
    let tripled = exe_name(&format!("{name}-{TRIPLE}"));
    let mut dirs: Vec<PathBuf> = Vec::new();
    // Bundled sidecars first — avoids Homebrew dylibs tripping the hardened runtime (macOS).
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    dirs.push(manifest.join("../binaries"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            dirs.push(dir.join("binaries"));
        }
    }
    for dir in &dirs {
        for cand in [&tripled, &plain] {
            let p = dir.join(cand);
            if exists(&p) {
                let s = p.to_string_lossy().into_owned();
                crate::log::line(&format!("resolve {name}: {s}"));
                return s;
            }
        }
    }
    // dev fallback: the npm ffmpeg-static / ffprobe-static packages
    let nm = manifest.join("../../node_modules");
    let npm_cands: Vec<PathBuf> = if name == "ffmpeg" {
        vec![nm.join("ffmpeg-static").join(&plain)]
    } else {
        let (os, arch) = if cfg!(target_os = "macos") {
            ("darwin", if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" })
        } else if cfg!(windows) {
            ("win32", "x64")
        } else {
            ("linux", if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" })
        };
        vec![nm.join("ffprobe-static").join("bin").join(os).join(arch).join(&plain)]
    };
    for p in npm_cands {
        if exists(&p) {
            let s = p.to_string_lossy().into_owned();
            crate::log::line(&format!("resolve {name}: npm {s}"));
            return s;
        }
    }
    crate::log::line(&format!("resolve {name}: fallback PATH:{name}"));
    name.to_string() // PATH
}

/// Run a bundled CLI to completion and capture its output. Both pipes are
/// drained concurrently (`output()` reads them on worker threads) — reading
/// stdout and then stderr sequentially deadlocks once a chatty child (ffmpeg
/// progress lines) fills the pipe nobody is draining yet.
fn run(cmd: &str, args: &[String]) -> CoreResult<(String, String)> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| CoreError::msg(format!("spawn {cmd}: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok((stdout, stderr))
    } else {
        let mut msg = if stderr.trim().is_empty() {
            format!("{cmd} exited {}", out.status)
        } else {
            ffmpeg_error_line(&stderr)
        };
        // Hardened-runtime dylib failure on macOS (Homebrew ffmpeg) — hint bundled binary.
        if msg.contains("code signature") || msg.contains("libavdevice") || msg.contains("mapped file") {
            msg = format!("{msg} — use bundled ffmpeg/ffprobe (check src-tauri/binaries/)");
        }
        Err(CoreError::msg(msg))
    }
}

/// Pull the single most useful line out of ffmpeg/ffprobe stderr: version and
/// configuration banners are skipped, explicit error lines win, and the last
/// substantive line is the fallback — so users see "Invalid argument …"
/// instead of a wall of --enable-* build flags.
fn ffmpeg_error_line(stderr: &str) -> String {
    const BANNER_PREFIXES: [&str; 13] = [
        "configuration:",
        "built with",
        "ffmpeg version",
        "ffprobe version",
        "libavutil",
        "libavcodec",
        "libavformat",
        "libavdevice",
        "libswresample",
        "libswscale",
        "libpostproc",
        "Copyright",
        "--",
    ];
    let banner = |raw: &str| {
        let t = raw.trim_start();
        t.is_empty()
            || BANNER_PREFIXES.iter().any(|p| t.starts_with(p))
            || t.starts_with("Press [q]")
            || t.starts_with("frame=")
            || t.starts_with("size=")
    };
    let errish = |raw: &str| {
        let t = raw.to_lowercase();
        [
            "error",
            "invalid",
            "no such",
            "could not",
            "cannot",
            "unable",
            "failed",
            "denied",
            "not found",
            "unrecognized",
            "does not exist",
        ]
        .iter()
        .any(|k| t.contains(k))
    };
    // Progress updates reuse one row via \r, so split on both separators.
    let lines: Vec<&str> = stderr
        .split('\n')
        .flat_map(|l| l.split('\r'))
        .map(str::trim)
        .filter(|l| !banner(l))
        .collect();
    let chosen = lines
        .iter()
        .rev()
        .find(|l| errish(l))
        .or_else(|| lines.iter().rev().find(|l| !l.is_empty()))
        .copied()
        .unwrap_or("no diagnostic output");
    let mut short: String = chosen.chars().take(300).collect();
    if short.len() < chosen.len() {
        short.push('…');
    }
    short
}

fn s(v: &str) -> String {
    v.to_string()
}

pub fn probe(file_path: &str) -> CoreResult<VideoMeta> {
    let ffprobe = resolve_bin("ffprobe");
    let (stdout, _) = run(
        &ffprobe,
        &[
            s("-v"), s("quiet"), s("-print_format"), s("json"),
            s("-show_format"), s("-show_streams"), s(file_path),
        ],
    )?;
    let data: serde_json::Value = serde_json::from_str(&stdout)?;
    let streams = data["streams"].as_array().cloned().unwrap_or_default();
    let find = |kind: &str| {
        streams
            .iter()
            .find(|st| st["codec_type"].as_str() == Some(kind))
            .cloned()
    };
    let video = find("video").unwrap_or(serde_json::Value::Null);
    let audio = find("audio");
    let num = |v: &serde_json::Value| v.as_str().and_then(|x| x.parse::<f64>().ok()).or_else(|| v.as_f64()).unwrap_or(0.0);
    let fps = video["avg_frame_rate"]
        .as_str()
        .map(eval_frac)
        .unwrap_or(0.0);
    Ok(VideoMeta {
        duration: num(&data["format"]["duration"]).max(num(&video["duration"])),
        width: video["width"].as_u64().unwrap_or(0) as u32,
        height: video["height"].as_u64().unwrap_or(0) as u32,
        fps: if fps > 0.0 { fps } else { 30.0 },
        has_audio: audio.is_some(),
        name: Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size: data["format"]["size"].as_str().and_then(|x| x.parse::<u64>().ok()).unwrap_or(0),
    })
}

fn eval_frac(v: &str) -> f64 {
    if v.is_empty() || v == "0/0" {
        return 0.0;
    }
    let mut it = v.split('/');
    let a: f64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
    let b: f64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
    if b == 0.0 {
        a
    } else {
        a / b
    }
}

/// Silence detection from the actual audio: decode to 16 kHz mono PCM, take
/// 100 ms RMS windows, and collect runs below a dB threshold. If the caller's
/// floor (default −30 dB) finds nothing but the file's own noise floor sits
/// higher, re-scan at floor + 6 dB — that adapts to loud rooms and quiet
/// microphones instead of trusting one absolute number.
pub fn detect_silence(file_path: &str, noise: Option<&str>, duration: Option<f64>) -> CoreResult<Vec<SilenceRange>> {
    let pcm = extract_pcm_16k(file_path)?;
    let user_db = noise.and_then(parse_db).unwrap_or(-30.0);
    let min_dur = duration.unwrap_or(0.6).max(0.02);
    let ranges = silence_from_pcm(&pcm, 16000, user_db, min_dur);
    crate::log::line(&format!(
        "silence scan ({} dB, min {min_dur}s) of {}: {} range(s): {:?}",
        user_db,
        Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_path.to_string()),
        ranges.len(),
        ranges.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>()
    ));
    Ok(ranges)
}

/// Decode once to 20 ms RMS windows. The UI slides floor / min-length over this.
pub fn audio_envelope(file_path: &str) -> CoreResult<AudioEnvelope> {
    let pcm = extract_pcm_16k(file_path)?;
    let env = crate::silence::envelope_from_pcm(&pcm, 16000);
    crate::log::line(&format!(
        "audio envelope: {} windows ({:.0} ms) of {}",
        env.dbs.len(),
        env.hop * 1000.0,
        Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_path.to_string()),
    ));
    Ok(env)
}

fn parse_db(s: &str) -> Option<f64> {
    s.trim()
        .trim_end_matches(|c| c == 'b' || c == 'B')
        .trim_end_matches(|c| c == 'd' || c == 'D')
        .trim()
        .parse()
        .ok()
}

/// How far below the floor audio must fall to count as silence. The floor is
/// a reference point, not the cut threshold: quiet speech often dips just
/// under it, so only stretches that bottom out DEEP_DIP_DB lower count as
/// pauses. Shallower windows *break* a run instead of extending it — that's
/// what kept swallowing "spoke a bit quieter" words sitting between pauses.
const DEEP_DIP_DB: f64 = 10.0;

/// Breathing room kept at each edge of a cut: word onsets and endings fade
/// through the threshold, so an unpadded cut clips them.
const CUT_PAD_S: f64 = 0.22;

/// After padding, ranges leaving less than this aren't worth cutting.
const MIN_REMAINDER_S: f64 = 0.18;

fn silence_from_pcm(pcm: &[f32], sr: usize, user_db: f64, min_dur: f64) -> Vec<SilenceRange> {
    let win = (sr / 10).max(1); // 100 ms windows
    if pcm.len() < win * 2 {
        return Vec::new();
    }
    let dbs: Vec<f64> = pcm
        .chunks(win)
        .map(|c| {
            let rms = (c.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / c.len() as f64).sqrt();
            20.0 * rms.max(1e-9).log10()
        })
        .collect();
    let win_dur = win as f64 / sr as f64;

    // Pauses = runs of windows well below the floor.
    let ranges = scan_quiet(&dbs, win_dur, user_db - DEEP_DIP_DB, min_dur);
    if !ranges.is_empty() {
        return pad_ranges(ranges);
    }
    // Nothing that deep. If the file never even crosses the user's floor, its
    // own noise floor sits higher (loud room, quiet mic) — fit the threshold
    // to the file, but only when loud content clearly stands above it
    // (otherwise a video with no real gaps would be flagged as one silence).
    // If there IS sub-floor audio that just isn't deep, that's quiet speech —
    // leave it alone.
    if !scan_quiet(&dbs, win_dur, user_db, min_dur).is_empty() {
        crate::log::line("silence scan: audio dips below the floor but never deep enough for a pause — left alone");
        return Vec::new();
    }
    let mut sorted = dbs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[sorted.len() / 7]; // ~14th percentile = "typical gap level"
    let loud = sorted[sorted.len() * 3 / 4]; // 75th percentile = "typical content level"
    let adapted = floor + 6.0;
    if adapted > user_db && floor > -80.0 && loud - adapted >= 6.0 {
        crate::log::line(&format!(
            "silence scan: nothing below {user_db} dB; file noise floor {floor:.1} dB — rescanning at {adapted:.1} dB"
        ));
        return pad_ranges(scan_quiet(&dbs, win_dur, adapted, min_dur));
    }
    Vec::new()
}

/// Shrink every range inward so cuts never land on word edges, dropping
/// ranges too short to be worth removing afterwards.
fn pad_ranges(ranges: Vec<SilenceRange>) -> Vec<SilenceRange> {
    ranges
        .into_iter()
        .filter_map(|r| {
            let start = r.start + CUT_PAD_S;
            let end = r.end - CUT_PAD_S;
            (end - start >= MIN_REMAINDER_S).then_some(SilenceRange { start, end })
        })
        .collect()
}

/// Runs of windows under `thr_db`, at least `min_dur` long; runs separated by
/// a blip shorter than 0.2 s (a click, a breath pop) merge into one range.
fn scan_quiet(dbs: &[f64], win_dur: f64, thr_db: f64, min_dur: f64) -> Vec<SilenceRange> {
    let mut ranges: Vec<SilenceRange> = Vec::new();
    let mut start: Option<f64> = None;
    for (i, &db) in dbs.iter().enumerate() {
        let t = i as f64 * win_dur;
        if db < thr_db {
            if start.is_none() {
                start = Some(t);
            }
        } else if let Some(st) = start.take() {
            if t - st >= min_dur {
                ranges.push(SilenceRange { start: st, end: t });
            }
        }
    }
    if let Some(st) = start {
        let end = dbs.len() as f64 * win_dur;
        if end - st >= min_dur {
            ranges.push(SilenceRange { start: st, end });
        }
    }
    // Merge ranges split by a sub-0.2 s blip.
    let mut merged: Vec<SilenceRange> = Vec::new();
    for r in ranges {
        if let Some(last) = merged.last_mut() {
            if r.start - last.end < 0.2 {
                last.end = r.end;
                continue;
            }
        }
        merged.push(r);
    }
    merged
}

// ---------- export ----------

#[derive(Debug, Clone, Copy)]
pub struct Seg {
    start: f64,
    end: f64,
}

pub fn invert_cuts(duration: f64, cuts: &[Change]) -> Vec<Seg> {
    let mut sorted: Vec<Seg> = cuts
        .iter()
        .map(|c| Seg { start: c.start.max(0.0), end: c.end.min(duration) })
        .filter(|c| c.end - c.start > 0.01)
        .collect();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));

    let mut merged: Vec<Seg> = Vec::new();
    for c in sorted {
        if let Some(last) = merged.last_mut() {
            if c.start <= last.end + 0.01 {
                last.end = last.end.max(c.end);
                continue;
            }
        }
        merged.push(c);
    }

    let mut keep = Vec::new();
    let mut cursor = 0.0;
    for c in &merged {
        if c.start > cursor + 0.02 {
            keep.push(Seg { start: cursor, end: c.start });
        }
        cursor = cursor.max(c.end);
    }
    if cursor < duration - 0.02 {
        keep.push(Seg { start: cursor, end: duration });
    }
    keep
}

fn esc_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\'', "\u{2019}")
        .replace(':', "\\:")
        .replace('%', "%%")
}

fn en(c: &Change) -> String {
    format!("enable='between(t,{:.3},{:.3})'", c.start, c.end)
}

fn overlay_filter(c: &Change, font: &str) -> String {
    let start = format!("{:.3}", c.start);
    let end = format!("{:.3}", c.end);
    let text = esc_text(c.text.as_deref().unwrap_or(""));
    let font = if font.is_empty() { String::new() } else { format!(":fontfile={font}") };
    match c.style.as_deref().unwrap_or("youtube") {
        "minimal" => format!("drawtext=text='{text}'{font}:fontsize=h*0.045:fontcolor=white:x=(w-text_w)/2:y=h*0.86:shadowcolor=black@0.6:shadowx=0:shadowy=2:enable='between(t,{start},{end})'"),
        "bold-caption" => format!("drawtext=text='{text}'{font}:fontsize=h*0.07:fontcolor=white:borderw=5:bordercolor=black:x=(w-text_w)/2:y=h*0.78:enable='between(t,{start},{end})'"),
        _ => format!("drawtext=text='{text}'{font}:fontsize=h*0.078:fontcolor=0xFFE566:borderw=6:bordercolor=black:x=(w-text_w)/2:y=h*0.76:enable='between(t,{start},{end})'"),
    }
}

fn pan_crop_expr(pans: &[Change]) -> Option<(String, String, String)> {
    if pans.is_empty() {
        return None;
    }
    let mut x = "(iw-ow)/2".to_string();
    for p in pans.iter().rev() {
        let t0 = format!("{:.3}", p.start);
        let t1 = format!("{:.3}", p.end);
        let span = format!("{:.3}", (p.end - p.start).max(0.01));
        let kind = p.pan.as_ref().map(|p| p.kind.as_str()).unwrap_or("zoom-in");
        let part = match kind {
            "pan-left" => format!("if(between(t,{t0},{t1}),(iw-ow)*(1-(t-{t0})/{span})"),
            "pan-right" => format!("if(between(t,{t0},{t1}),(iw-ow)*((t-{t0})/{span})"),
            _ => format!("if(between(t,{t0},{t1}),(iw-ow)/2"),
        };
        x = format!("{part},{x})");
    }
    let mut y = "(ih-oh)/2".to_string();
    for p in pans.iter().rev() {
        let t0 = format!("{:.3}", p.start);
        let t1 = format!("{:.3}", p.end);
        let span = format!("{:.3}", (p.end - p.start).max(0.01));
        let kind = p.pan.as_ref().map(|p| p.kind.as_str()).unwrap_or("zoom-in");
        let part = match kind {
            "zoom-in" => format!("if(between(t,{t0},{t1}),(ih-oh)*((t-{t0})/{span})*0.35"),
            "zoom-out" => format!("if(between(t,{t0},{t1}),(ih-oh)*(1-(t-{t0})/{span})*0.35"),
            _ => format!("if(between(t,{t0},{t1}),(ih-oh)/2"),
        };
        y = format!("{part},{y})");
    }
    let mut z = "1".to_string();
    for p in pans.iter().rev() {
        let t0 = format!("{:.3}", p.start);
        let t1 = format!("{:.3}", p.end);
        let span = format!("{:.3}", (p.end - p.start).max(0.01));
        let kind = p.pan.as_ref().map(|p| p.kind.as_str()).unwrap_or("zoom-in");
        let part = match kind {
            "zoom-out" => format!("if(between(t,{t0},{t1}),1.18-0.18*((t-{t0})/{span})"),
            "zoom-in" => format!("if(between(t,{t0},{t1}),1+0.18*((t-{t0})/{span})"),
            _ => format!("if(between(t,{t0},{t1}),1.12"),
        };
        z = format!("{part},{z})");
    }
    Some((x, y, z))
}

fn pick_font() -> String {
    let candidates = [
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Impact.ttf",
        "/Library/Fonts/Arial Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    ];
    candidates
        .iter()
        .find(|p| exists(Path::new(p)))
        .map(|p| p.to_string())
        .unwrap_or_default()
}

fn remap(list: &[Change], seg: Seg) -> Vec<Change> {
    list.iter()
        .filter(|c| c.end > seg.start && c.start < seg.end)
        .map(|c| {
            let mut c = c.clone();
            c.start = (c.start - seg.start).max(0.0);
            c.end = c.end.min(seg.end) - seg.start;
            c
        })
        .collect()
}

fn fx_filters(c: &Change) -> Vec<String> {
    match c.kind.as_str() {
        "shake" => vec![format!(
            "crop=iw/1.08:ih/1.08:'(iw-ow)/2+18*sin(22*t)':'(ih-oh)/2+14*cos(19*t)':{}",
            en(c)
        )],
        "tilt" => vec![format!("rotate=-0.12:fillcolor=black:{}", en(c))],
        "flash" => vec![format!("eq=brightness=0.7:{}", en(c))],
        "bounce" | "punch" => vec![format!("scale=iw*1.16:ih*1.16,crop=iw/1.16:ih/1.16:{}", en(c))],
        "glitch" => vec![format!("hue=h=35:s=2:{}", en(c))],
        "spotlight" => vec![format!("vignette=PI/4:{}", en(c))],
        _ => vec![],
    }
}

fn textcard_filters(c: &Change, font: &str) -> Vec<String> {
    let text = esc_text(c.text.as_deref().unwrap_or("NOPE"));
    let font = if font.is_empty() { String::new() } else { format!(":fontfile={font}") };
    vec![
        format!("drawbox=x=0:y=0:w=iw:h=ih:color=black@1:t=fill:{}", en(c)),
        format!("drawtext=text='{text}'{font}:fontsize=h*0.12:fontcolor=0xFFE566:borderw=6:bordercolor=black:x=(w-text_w)/2:y=(h-text_h)/2:{}", en(c)),
    ]
}

pub fn export_project<F>(payload: &ExportPayload, progress: F) -> CoreResult<()>
where
    F: FnMut(ExportProgress),
{
    let mut progress = progress;
    let mut last_log_bucket: i32 = -1;
    let mut report = |pct: u32, label: String| {
        let pct = pct.min(100);
        progress(ExportProgress { progress: pct, label: label.clone() });
        let bucket = (pct / 5) * 5;
        if pct == 0 || pct == 100 || bucket as i32 != last_log_bucket {
            last_log_bucket = bucket as i32;
            crate::log::line(&format!("export: {pct}% — {label}"));
        }
    };

    crate::log::line(&format!(
        "export: {} → {} ({} change(s))",
        payload.input, payload.output,
        payload.changes.iter().filter(|c| c.status == "accepted").count()
    ));
    report(0, "Preparing export".into());
    let t0 = std::time::Instant::now();
    let accepted: Vec<&Change> = payload.changes.iter().filter(|c| c.status == "accepted").collect();
    let cuts: Vec<Change> = accepted.iter().filter(|c| c.kind == "cut").map(|c| (*c).clone()).collect();
    let overlays: Vec<Change> = accepted
        .iter()
        .filter(|c| (c.kind == "overlay" || c.kind == "textcard" || c.kind == "impact") && c.text.is_some())
        .map(|c| (*c).clone())
        .collect();
    let pans: Vec<Change> = accepted.iter().filter(|c| c.kind == "pan").map(|c| (*c).clone()).collect();
    let fx: Vec<Change> = accepted
        .iter()
        .filter(|c| {
            ["shake", "tilt", "flash", "bounce", "glitch", "spotlight", "slow", "fast", "freeze", "punch"]
                .contains(&c.kind.as_str())
        })
        .map(|c| (*c).clone())
        .collect();

    if cuts.is_empty() && overlays.is_empty() && pans.is_empty() && fx.is_empty() {
        if !payload.normalize {
            return Err(CoreError::msg(
                "Nothing to export — add a cut or turn on volume normalize.",
            ));
        }
        crate::log::line("export: normalize only (copy video, re-encode audio)");
        report(88, "Normalizing audio".into());
        apply_podcast_norm(
            &resolve_bin("ffmpeg"),
            Path::new(&payload.input),
            Path::new(&payload.output),
            payload.normalize_amount,
        )?;
        verify_mp4(&payload.output)?;
        report(100, "Export done".into());
        crate::log::line(&format!("export done in {:.1?}", t0.elapsed()));
        return Ok(());
    }

    let mut keep = invert_cuts(payload.duration, &cuts);
    keep.retain(|s| s.end - s.start >= 0.08);
    if keep.is_empty() {
        return Err(CoreError::msg("Nothing left to export after cuts."));
    }

    let ffmpeg = resolve_bin("ffmpeg");
    let fps = probe(&payload.input).map(|m| if m.fps > 1.0 { m.fps } else { 30.0 }).unwrap_or(30.0);
    crate::log::line(&format!("export: {} keep-segment(s) @ {:.2} fps", keep.len(), fps));
    report(5, format!("Exporting {} keep-segment(s) @ {:.2} fps", keep.len(), fps));

    let font = pick_font();
    let tmp = std::env::temp_dir().join(format!("mustardy-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let mut parts: Vec<PathBuf> = Vec::new();

    let result = (|| -> CoreResult<()> {
        for (i, seg) in keep.iter().enumerate() {
            let seg_path = tmp.join(format!("seg-{i}.mp4"));
            let mut vf: Vec<String> = Vec::new();

            let local_pans: Vec<Change> = pans
                .iter()
                .filter(|p| p.end > seg.start && p.start < seg.end)
                .map(|p| {
                    let mut p = p.clone();
                    p.start = (p.start - seg.start).max(0.0);
                    p.end = p.end.min(seg.end) - seg.start;
                    p
                })
                .collect();
            if let Some((x, y, _z)) = pan_crop_expr(&local_pans) {
                vf.push(s("scale=iw*1.2:ih*1.2"));
                vf.push(format!("crop=iw/1.2:ih/1.2:{x}:{y}"));
            }
            for f in remap(&fx, *seg) {
                vf.extend(fx_filters(&f));
            }
            for o in remap(&overlays, *seg) {
                if o.kind == "textcard" || o.kind == "impact" {
                    vf.extend(textcard_filters(&o, &font));
                } else {
                    vf.push(overlay_filter(&o, &font));
                }
            }

            let local_fx = remap(&fx, *seg);
            let speed = local_fx.iter().find(|f| f.kind == "slow" || f.kind == "fast" || f.kind == "freeze");
            let rate = match speed.map(|f| f.kind.as_str()) {
                Some("slow") => speed.and_then(|f| f.rate).unwrap_or(0.5),
                Some("fast") => speed.and_then(|f| f.rate).unwrap_or(1.8),
                _ => 1.0,
            };
            if let Some(sp) = speed {
                if sp.kind != "freeze" {
                    vf.push(format!("setpts=PTS/{rate}"));
                } else {
                    vf.push(s("tblend=all_mode=average"));
                    vf.push(s("framestep=2"));
                }
            }

            let mut args = vec![
                s("-y"), s("-ss"), format!("{}", seg.start), s("-to"), format!("{}", seg.end),
                s("-i"), payload.input.clone(),
            ];
            if !vf.is_empty() {
                args.push(s("-vf"));
                args.push(vf.into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(","));
            }
            if let Some(sp) = speed {
                if sp.kind != "freeze" {
                    let tempo = rate.clamp(0.5, 2.0);
                    args.push(s("-af"));
                    args.push(format!("atempo={tempo}"));
                }
            }
            args.extend([
                s("-r"), format!("{:.3}", fps),
                s("-vsync"), s("cfr"),
                s("-c:v"), s("libx264"), s("-preset"), s("veryfast"), s("-crf"), s("18"),
                s("-c:a"), s("aac"), s("-b:a"), s("192k"),
                s("-pix_fmt"), s("yuv420p"),
                seg_path.to_string_lossy().into_owned(),
            ]);
            run(&ffmpeg, &args)?;
            parts.push(seg_path);
            let pct = 5 + (((i + 1) as f64 / keep.len().max(1) as f64) * 85.0).round() as u32;
            report(pct, format!("Encoded segment {}/{}", i + 1, keep.len()));
        }

        report(92, "Joining segments".into());
        let staged = tmp.join("out.mp4");
        if parts.len() == 1 {
            remux_faststart(&ffmpeg, &parts[0], &staged)?;
        } else {
            let list_path = tmp.join("list.txt");
            let list = parts
                .iter()
                .map(|p| format!("file '{}'", p.to_string_lossy().replace('\'', "'\\''")))
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&list_path, list)?;
            let joined = tmp.join("joined.mp4");
            run(
                &ffmpeg,
                &[
                    s("-y"), s("-f"), s("concat"), s("-safe"), s("0"),
                    s("-i"), list_path.to_string_lossy().into_owned(),
                    s("-c"), s("copy"),
                    joined.to_string_lossy().into_owned(),
                ],
            )?;
            remux_faststart(&ffmpeg, &joined, &staged)?;
        }
        verify_mp4(&staged)?;
        report(96, "Finalizing MP4".into());
        if payload.normalize {
            report(98, "Normalizing audio".into());
            let normed = tmp.join("norm.mp4");
            apply_podcast_norm(&ffmpeg, &staged, &normed, payload.normalize_amount)?;
            verify_mp4(&normed)?;
            std::fs::copy(&normed, &payload.output)?;
        } else {
            std::fs::copy(&staged, &payload.output)?;
        }
        verify_mp4(&payload.output)?;
        report(100, "Export done".into());
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    match &result {
        Ok(()) => crate::log::line(&format!("export done in {:.1?}", t0.elapsed())),
        Err(e) => crate::log::line(&format!("export failed: {e}")),
    }
    result
}

/// Podcast evenness: high-pass rumble, dynaudnorm to lift far-from-mic speech
/// (capped so noise doesn't roar), then loudnorm to −16 LUFS with a tight LRA
/// so the file doesn't keep huge level swings.
fn podcast_af(amount: f64) -> String {
    let a = amount.clamp(0.0, 1.0);
    let maxgain = 5.0 + a * 11.0; // linear factor ≈ 14–24 dB
    let compress = 4.0 + a * 8.0;
    let lra = 9.0 - a * 4.0; // 9 … 5 LU
    format!(
        "highpass=f=80,dynaudnorm=f=180:g=17:p=0.93:m={maxgain:.1}:r=0.16:s={compress:.1},loudnorm=I=-16:TP=-1.5:LRA={lra:.1}"
    )
}

fn apply_podcast_norm(ffmpeg: &str, input: &Path, output: &Path, amount: f64) -> CoreResult<()> {
    let af = podcast_af(amount);
    crate::log::line(&format!("podcast norm: {af}"));
    run(
        ffmpeg,
        &[
            s("-y"),
            s("-i"),
            input.to_string_lossy().into_owned(),
            s("-c:v"),
            s("copy"),
            s("-af"),
            af,
            s("-c:a"),
            s("aac"),
            s("-b:a"),
            s("192k"),
            s("-ar"),
            s("48000"),
            s("-movflags"),
            s("+faststart"),
            output.to_string_lossy().into_owned(),
        ],
    )?;
    Ok(())
}

fn remux_faststart(ffmpeg: &str, input: &std::path::Path, output: &std::path::Path) -> CoreResult<()> {
    run(
        ffmpeg,
        &[
            s("-y"),
            s("-i"),
            input.to_string_lossy().into_owned(),
            s("-c"),
            s("copy"),
            s("-movflags"),
            s("+faststart"),
            output.to_string_lossy().into_owned(),
        ],
    )?;
    Ok(())
}

fn verify_mp4(path: impl AsRef<std::path::Path>) -> CoreResult<()> {
    let p = path.as_ref().to_string_lossy().into_owned();
    let meta = probe(&p)?;
    if meta.duration < 0.4 {
        return Err(CoreError::msg(
            "export produced an incomplete file (no duration / missing moov). Try Export again.",
        ));
    }
    crate::log::line(&format!("export verify ok: {:.1}s {}", meta.duration, p));
    Ok(())
}

// ---------- audio / frames for inference ----------

/// Decode a media file to 16 kHz mono f32 PCM (for whisper).
pub fn extract_pcm_16k(input: &str) -> CoreResult<Vec<f32>> {
    let ffmpeg = resolve_bin("ffmpeg");
    let out = Command::new(ffmpeg)
        .args([
            "-y", "-i", input, "-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "-acodec", "pcm_f32le", "-",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| CoreError::msg(format!("ffmpeg pcm: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::msg("ffmpeg pcm extraction failed"));
    }
    let bytes = out.stdout;
    let mut pcm = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        pcm.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(pcm)
}

/// Grab JPEG frames at the given timestamps (used by the smoke test; the app
/// grabs frames in the webview canvas instead).
pub fn extract_frames(input: &str, times: &[f64], width: u32) -> CoreResult<Vec<(f64, Vec<u8>)>> {
    let ffmpeg = resolve_bin("ffmpeg");
    let mut out = Vec::new();
    for &t in times {
        let res = Command::new(&ffmpeg)
            .args([
                "-y", "-ss", &format!("{t:.3}"), "-i", input, "-frames:v", "1",
                "-vf", &format!("scale='min({width},iw)':-2"), "-f", "image2pipe", "-c:v", "mjpeg", "-q:v", "4", "-",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| CoreError::msg(format!("ffmpeg frame: {e}")))?;
        if res.status.success() && !res.stdout.is_empty() {
            out.push((t, res.stdout));
        }
    }
    Ok(out)
}

/// Grab small raw RGB24 frames sampled at `fps` across [start, end] in ONE
/// ffmpeg pass. For pixel-diffing, not inference — frames come back as
/// `w*h*3` byte chunks; frame i sits at roughly `start + i/fps`.
pub fn extract_range_frames(
    input: &str,
    start: f64,
    end: f64,
    fps: f64,
    w: u32,
    h: u32,
) -> CoreResult<Vec<Vec<u8>>> {
    let ffmpeg = resolve_bin("ffmpeg");
    let out = Command::new(ffmpeg)
        .args([
            s("-y"),
            s("-ss"),
            format!("{start:.3}"),
            s("-to"),
            format!("{end:.3}"),
            s("-i"),
            s(input),
            s("-vf"),
            format!("fps={fps:.3},scale={w}:{h}"),
            s("-f"),
            s("rawvideo"),
            s("-pix_fmt"),
            s("rgb24"),
            s("-"),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| CoreError::msg(format!("ffmpeg range frames: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::msg("ffmpeg range frame extraction failed"));
    }
    let frame_len = (w * h * 3) as usize;
    Ok(out.stdout.chunks_exact(frame_len).map(|c| c.to_vec()).collect())
}
