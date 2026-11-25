//! FFmpeg/ffprobe execution layer.
//!
//! Binary resolution order:
//! 1. `MUSTARDY_FFMPEG` / `MUSTARDY_FFPROBE` env vars
//! 2. Bundled sidecar dirs (`<exe>/`, `<exe>/binaries/`, `src-tauri/binaries/`)
//! 3. Dev npm `ffmpeg-static` / `ffprobe-static`
//! 4. Bare binary name on `PATH`

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::{
    AudioEnvelope, Change, CoreError, CoreResult, ExportOptions, ExportPayload, ExportProgress,
    VideoCodec, VideoMeta,
};

const SINGLE_PASS_MAX_SEGS: usize = 48;
const CHUNK_SEGS: usize = 40;

const TRIPLE: &str = {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "unknown"
    }
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

fn s(v: &str) -> String {
    v.to_string()
}

// ---------- temp directory cleanup registry ----------

static ACTIVE_TEMP_DIRS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

fn temp_registry() -> &'static Mutex<Vec<PathBuf>> {
    ACTIVE_TEMP_DIRS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Remove any in-flight temporary working directories (called on Ctrl-C).
pub fn cleanup_registered_temp_dirs() {
    if let Ok(mut guard) = temp_registry().lock() {
        for dir in guard.drain(..) {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn create(preferred_parent: Option<&Path>, explicit_base: Option<&Path>) -> CoreResult<Self> {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(".mustardy-tmp-{}-{seq}", std::process::id());
        let candidates: Vec<PathBuf> = [
            explicit_base.map(PathBuf::from),
            preferred_parent.map(|p| {
                if p.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    p.to_path_buf()
                }
            }),
            Some(std::env::temp_dir()),
        ]
        .into_iter()
        .flatten()
        .collect();

        let mut last_err = None;
        for base in candidates {
            let dir = base.join(&name);
            match std::fs::create_dir_all(&dir) {
                Ok(()) => {
                    if let Ok(mut guard) = temp_registry().lock() {
                        guard.push(dir.clone());
                    }
                    return Ok(Self { path: dir });
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(CoreError::Io(last_err.unwrap_or_else(|| {
            std::io::Error::other("could not create temp directory")
        })))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
        if let Ok(mut guard) = temp_registry().lock() {
            guard.retain(|p| p != &self.path);
        }
    }
}

/// Move `staged` to `dest` atomically when on the same filesystem, falling back
/// to copy + remove across filesystems.
fn finalize_file(staged: &Path, dest: &Path) -> CoreResult<()> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    match std::fs::rename(staged, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(staged, dest)?;
            let _ = std::fs::remove_file(staged);
            Ok(())
        }
    }
}

// ---------- binary resolution & capability probing ----------

fn resolve_bin_uncached(name: &str) -> String {
    let plain = exe_name(name);
    let tripled = exe_name(&format!("{name}-{TRIPLE}"));
    let mut dirs: Vec<PathBuf> = Vec::new();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    dirs.push(manifest.join("src-tauri/binaries"));
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
                let resolved = p.to_string_lossy().into_owned();
                crate::log::debug(&format!("resolve {name}: {resolved}"));
                return resolved;
            }
        }
    }
    let nm = manifest.join("node_modules");
    let npm_cands: Vec<PathBuf> = if name == "ffmpeg" {
        vec![nm.join("ffmpeg-static").join(&plain)]
    } else {
        let (os, arch) = if cfg!(target_os = "macos") {
            (
                "darwin",
                if cfg!(target_arch = "aarch64") {
                    "arm64"
                } else {
                    "x64"
                },
            )
        } else if cfg!(windows) {
            ("win32", "x64")
        } else {
            (
                "linux",
                if cfg!(target_arch = "aarch64") {
                    "arm64"
                } else {
                    "x64"
                },
            )
        };
        vec![nm
            .join("ffprobe-static")
            .join("bin")
            .join(os)
            .join(arch)
            .join(&plain)]
    };
    for p in npm_cands {
        if exists(&p) {
            let resolved = p.to_string_lossy().into_owned();
            crate::log::debug(&format!("resolve {name}: npm {resolved}"));
            return resolved;
        }
    }
    crate::log::debug(&format!("resolve {name}: PATH:{name}"));
    name.to_string()
}

pub fn resolve_bin(name: &str) -> String {
    let env_key = format!("MUSTARDY_{}", name.to_uppercase());
    if let Ok(v) = std::env::var(&env_key) {
        if !v.is_empty() && exists(Path::new(&v)) {
            return v;
        }
    }
    match name {
        "ffmpeg" => {
            static FFMPEG: OnceLock<String> = OnceLock::new();
            FFMPEG.get_or_init(|| resolve_bin_uncached("ffmpeg")).clone()
        }
        "ffprobe" => {
            static FFPROBE: OnceLock<String> = OnceLock::new();
            FFPROBE
                .get_or_init(|| resolve_bin_uncached("ffprobe"))
                .clone()
        }
        _ => resolve_bin_uncached(name),
    }
}

pub fn binary_version(name: &str) -> CoreResult<String> {
    let bin = resolve_bin(name);
    let out = Command::new(&bin)
        .arg("-version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| CoreError::coded("FFMPEG_MISSING", format!("could not run {bin}: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::coded(
            "FFMPEG_ERROR",
            format!("{bin} -version failed ({})", out.status),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().next().unwrap_or("unknown").trim().to_string())
}

/// Detect whether the resolved `ffmpeg` supports `-fps_mode` (FFmpeg 5.1+).
fn supports_fps_mode() -> bool {
    static SUPPORTS: OnceLock<bool> = OnceLock::new();
    *SUPPORTS.get_or_init(|| {
        let ffmpeg = resolve_bin("ffmpeg");
        let out = Command::new(&ffmpeg)
            .args(["-hide_banner", "-h", "full"])
            .stdin(Stdio::null())
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout);
                if s.contains("fps_mode") {
                    true
                } else {
                    !s.contains("-vsync")
                }
            }
            Err(_) => true,
        }
    })
}

fn cfr_args() -> [String; 2] {
    if supports_fps_mode() {
        [s("-fps_mode"), s("cfr")]
    } else {
        [s("-vsync"), s("cfr")]
    }
}

/// List hardware H.264 encoders compiled into the resolved FFmpeg binary.
pub fn listed_hw_encoders() -> Vec<String> {
    let ffmpeg = resolve_bin("ffmpeg");
    let Ok(out) = Command::new(&ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let preferred: &[&str] = if cfg!(target_os = "macos") {
        &["h264_videotoolbox"]
    } else if cfg!(windows) {
        &["h264_nvenc", "h264_amf", "h264_qsv"]
    } else {
        &["h264_nvenc", "h264_vaapi", "h264_qsv", "h264_amf"]
    };
    preferred
        .iter()
        .filter(|&&enc| text.lines().any(|l| l.split_whitespace().nth(1) == Some(enc)))
        .map(|&enc| enc.to_string())
        .collect()
}

/// Probe whether a specific encoder actually initializes on this machine's hardware.
pub fn probe_encoder_works(encoder: &str) -> bool {
    let ffmpeg = resolve_bin("ffmpeg");
    let mut args = vec![
        s("-hide_banner"),
        s("-v"),
        s("error"),
        s("-f"),
        s("lavfi"),
        s("-i"),
        s("color=c=black:s=128x128:d=0.05:r=30"),
        s("-frames:v"),
        s("1"),
    ];
    if encoder == "h264_vaapi" {
        args.extend([
            s("-vaapi_device"),
            s("/dev/dri/renderD128"),
            s("-vf"),
            s("format=nv12,hwupload"),
        ]);
    }
    args.extend([s("-c:v"), s(encoder), s("-f"), s("null"), s("-")]);
    Command::new(&ffmpeg)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|st| st.success())
}

/// Return the first hardware H.264 encoder that actually works on this host.
pub fn detect_working_hw_encoder() -> Option<String> {
    static HW: OnceLock<Option<String>> = OnceLock::new();
    HW.get_or_init(|| {
        for enc in listed_hw_encoders() {
            if probe_encoder_works(&enc) {
                crate::log::debug(&format!("detected working hw encoder: {enc}"));
                return Some(enc);
            }
        }
        None
    })
    .clone()
}

fn video_encoder_args(opts: &ExportOptions) -> (String, Vec<String>, bool) {
    let chosen = match &opts.codec {
        VideoCodec::Software => "libx264".to_string(),
        VideoCodec::Hardware => {
            detect_working_hw_encoder().unwrap_or_else(|| "libx264".to_string())
        }
        VideoCodec::Custom(c) => c.clone(),
    };
    let crf = opts.crf.clamp(0, 51);
    let mut args = Vec::new();
    let is_vaapi = chosen == "h264_vaapi";
    match chosen.as_str() {
        "libx264" | "libx265" => {
            args.extend([
                s("-c:v"),
                chosen.clone(),
                s("-preset"),
                opts.preset.clone(),
                s("-crf"),
                crf.to_string(),
                s("-pix_fmt"),
                s("yuv420p"),
            ]);
        }
        "h264_videotoolbox" | "hevc_videotoolbox" => {
            // Map CRF (0..51, lower=better) onto VideoToolbox q:v (1..100, higher=better).
            let q = (100i32 - (crf as i32 * 2)).clamp(35, 90);
            args.extend([
                s("-c:v"),
                chosen.clone(),
                s("-q:v"),
                q.to_string(),
                s("-pix_fmt"),
                s("yuv420p"),
            ]);
        }
        "h264_nvenc" | "hevc_nvenc" => {
            args.extend([
                s("-c:v"),
                chosen.clone(),
                s("-preset"),
                s("p4"),
                s("-cq"),
                crf.to_string(),
                s("-pix_fmt"),
                s("yuv420p"),
            ]);
        }
        "h264_vaapi" => {
            args.extend([s("-c:v"), chosen.clone(), s("-qp"), crf.to_string()]);
        }
        "h264_qsv" => {
            args.extend([
                s("-c:v"),
                chosen.clone(),
                s("-global_quality"),
                crf.to_string(),
                s("-pix_fmt"),
                s("yuv420p"),
            ]);
        }
        _ => {
            args.extend([
                s("-c:v"),
                chosen.clone(),
                s("-crf"),
                crf.to_string(),
                s("-pix_fmt"),
                s("yuv420p"),
            ]);
        }
    }
    (chosen, args, is_vaapi)
}

// ---------- process execution & diagnostics ----------

/// Run a bundled CLI to completion and capture its output.
fn run(cmd: &str, args: &[String]) -> CoreResult<(String, String)> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| CoreError::coded("FFMPEG_MISSING", format!("spawn {cmd}: {e}")))?;
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
        if msg.contains("code signature")
            || msg.contains("libavdevice")
            || msg.contains("mapped file")
        {
            msg = format!("{msg} — use bundled ffmpeg/ffprobe (check src-tauri/binaries/)");
        }
        Err(CoreError::coded("FFMPEG_ERROR", msg))
    }
}

/// Run ffmpeg with `-progress pipe:1` to stream real-time out_time_us updates.
fn run_with_progress<F>(
    ffmpeg: &str,
    args: &[String],
    expected_duration: f64,
    mut on_fraction: F,
) -> CoreResult<()>
where
    F: FnMut(f64),
{
    let mut full_args = Vec::with_capacity(args.len() + 4);
    full_args.push(s("-hide_banner"));
    full_args.push(s("-nostats"));
    full_args.push(s("-progress"));
    full_args.push(s("pipe:1"));
    full_args.extend_from_slice(args);

    let mut child = Command::new(ffmpeg)
        .args(&full_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::coded("FFMPEG_MISSING", format!("spawn {ffmpeg}: {e}")))?;

    let stderr_handle = child.stderr.take().map(|mut err_pipe| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = err_pipe.read_to_string(&mut buf);
            buf
        })
    });

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(val) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = val.trim().parse::<i64>() {
                    if us >= 0 && expected_duration > 0.0 {
                        let frac = ((us as f64) / 1_000_000.0 / expected_duration).clamp(0.0, 1.0);
                        on_fraction(frac);
                    }
                }
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| CoreError::coded("FFMPEG_ERROR", format!("wait {ffmpeg}: {e}")))?;
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if status.success() {
        on_fraction(1.0);
        Ok(())
    } else {
        let msg = if stderr.trim().is_empty() {
            format!("{ffmpeg} exited {status}")
        } else {
            ffmpeg_error_line(&stderr)
        };
        Err(CoreError::coded("FFMPEG_ERROR", msg))
    }
}

/// Extract the most informative error lines from ffmpeg/ffprobe stderr.
///
/// Skips version/config banners, progress rows, and generic useless trailer
/// lines like `"Conversion failed!"`, returning up to 3 specific diagnostic
/// lines so root causes are never hidden.
pub fn ffmpeg_error_line(stderr: &str) -> String {
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
    const GENERIC_TRAILERS: [&str; 5] = [
        "conversion failed!",
        "error closing file",
        "nothing was written into output file",
        "error initializing output stream",
        "exiting normally",
    ];
    let banner = |raw: &str| {
        let t = raw.trim_start();
        t.is_empty()
            || BANNER_PREFIXES.iter().any(|p| t.starts_with(p))
            || t.starts_with("Press [q]")
            || t.starts_with("frame=")
            || t.starts_with("size=")
            || t.starts_with("out_time")
    };
    let generic_trailer = |raw: &str| {
        let lower = raw.to_lowercase();
        GENERIC_TRAILERS.iter().any(|g| lower.contains(g))
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
            "unknown",
            "unsupported",
            "no space left",
            "permission",
        ]
        .iter()
        .any(|k| t.contains(k))
    };

    let lines: Vec<&str> = stderr
        .split('\n')
        .flat_map(|l| l.split('\r'))
        .map(str::trim)
        .filter(|l| !banner(l))
        .collect();

    let mut specific: Vec<&str> = Vec::new();
    for &l in lines.iter().rev() {
        if errish(l) && !generic_trailer(l) && !specific.contains(&l) {
            specific.push(l);
            if specific.len() >= 3 {
                break;
            }
        }
    }
    specific.reverse();

    let joined = if !specific.is_empty() {
        specific.join(" | ")
    } else {
        lines
            .iter()
            .rev()
            .find(|l| !generic_trailer(l) && !l.is_empty())
            .or_else(|| lines.iter().rev().find(|l| !l.is_empty()))
            .copied()
            .unwrap_or("no diagnostic output")
            .to_string()
    };

    let mut short: String = joined.chars().take(600).collect();
    if short.len() < joined.len() {
        short.push('…');
    }
    short
}

// ---------- probe & streaming envelope ----------

pub fn probe(file_path: &str) -> CoreResult<VideoMeta> {
    let ffprobe = resolve_bin("ffprobe");
    let (stdout, _) = run(
        &ffprobe,
        &[
            s("-v"),
            s("error"),
            s("-print_format"),
            s("json"),
            s("-show_format"),
            s("-show_streams"),
            s(file_path),
        ],
    )?;
    let data: serde_json::Value = serde_json::from_str(&stdout)?;
    let streams = data["streams"].as_array().cloned().unwrap_or_default();
    // Ignore attached cover art (`disposition.attached_pic == 1`) when checking for real video.
    let video = streams.iter().find(|st| {
        st["codec_type"].as_str() == Some("video")
            && st["disposition"]["attached_pic"].as_i64().unwrap_or(0) == 0
    });
    let audio = streams
        .iter()
        .find(|st| st["codec_type"].as_str() == Some("audio"));

    let num = |v: &serde_json::Value| {
        v.as_str()
            .and_then(|x| x.parse::<f64>().ok())
            .or_else(|| v.as_f64())
            .unwrap_or(0.0)
    };

    let fps = video
        .and_then(|v| v["avg_frame_rate"].as_str())
        .map(eval_frac)
        .filter(|&f| f > 0.0 && f.is_finite())
        .or_else(|| {
            video
                .and_then(|v| v["r_frame_rate"].as_str())
                .map(eval_frac)
                .filter(|&f| f > 0.0 && f.is_finite())
        })
        .unwrap_or(0.0);

    let has_video = video.is_some();
    let v_dur = video.map(|v| num(&v["duration"])).unwrap_or(0.0);
    let a_dur = audio.map(|a| num(&a["duration"])).unwrap_or(0.0);
    let f_dur = num(&data["format"]["duration"]);
    let duration = f_dur.max(v_dur).max(a_dur);

    let start_time = num(&data["format"]["start_time"]).max(0.0);

    Ok(VideoMeta {
        duration,
        width: video.and_then(|v| v["width"].as_u64()).unwrap_or(0) as u32,
        height: video.and_then(|v| v["height"].as_u64()).unwrap_or(0) as u32,
        fps: if has_video {
            if fps > 0.0 {
                fps
            } else {
                30.0
            }
        } else {
            0.0
        },
        has_audio: audio.is_some(),
        has_video,
        start_time,
        video_codec: video
            .and_then(|v| v["codec_name"].as_str())
            .map(ToString::to_string),
        audio_codec: audio
            .and_then(|a| a["codec_name"].as_str())
            .map(ToString::to_string),
        sample_rate: audio.and_then(|a| {
            a["sample_rate"]
                .as_str()
                .and_then(|x| x.parse::<u32>().ok())
                .or_else(|| a["sample_rate"].as_u64().map(|x| x as u32))
        }),
        channels: audio
            .and_then(|a| a["channels"].as_u64())
            .map(|x| x as u32),
        name: Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size: data["format"]["size"]
            .as_str()
            .and_then(|x| x.parse::<u64>().ok())
            .unwrap_or(0),
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

/// Stream 16 kHz mono f32 PCM from FFmpeg and compute 20 ms RMS windows in
/// O(1) memory (~4 KB buffer instead of buffering hundreds of MB of raw PCM).
pub fn audio_envelope(file_path: &str) -> CoreResult<AudioEnvelope> {
    let ffmpeg = resolve_bin("ffmpeg");
    const SR: usize = 16_000;
    const HOP_SAMPLES: usize = 320; // 20 ms @ 16 kHz
    const HOP_S: f64 = HOP_SAMPLES as f64 / SR as f64;
    const CHUNK_BYTES: usize = HOP_SAMPLES * 4;

    let mut child = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-i",
            file_path,
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::coded("FFMPEG_MISSING", format!("ffmpeg pcm: {e}")))?;

    let stderr_handle = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = pipe.read_to_string(&mut s);
            s
        })
    });

    let mut dbs: Vec<f32> = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::with_capacity(64 * 1024, stdout);
        let mut buf = [0u8; CHUNK_BYTES];
        let mut filled = 0usize;
        loop {
            match reader.read(&mut buf[filled..]) {
                Ok(0) => {
                    if filled >= 4 {
                        let samples = filled / 4;
                        let mut sum = 0.0f64;
                        for chunk in buf[..samples * 4].chunks_exact(4) {
                            let x =
                                f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64;
                            sum += x * x;
                        }
                        let rms = (sum / samples as f64).sqrt();
                        dbs.push((20.0 * rms.max(1e-9).log10()) as f32);
                    }
                    break;
                }
                Ok(n) => {
                    filled += n;
                    if filled == CHUNK_BYTES {
                        let mut sum = 0.0f64;
                        for chunk in buf.chunks_exact(4) {
                            let x =
                                f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64;
                            sum += x * x;
                        }
                        let rms = (sum / HOP_SAMPLES as f64).sqrt();
                        dbs.push((20.0 * rms.max(1e-9).log10()) as f32);
                        filled = 0;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(CoreError::Io(e)),
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| CoreError::coded("FFMPEG_ERROR", format!("ffmpeg pcm wait: {e}")))?;
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if !status.success() {
        let detail = if stderr.trim().is_empty() {
            format!("ffmpeg pcm extraction failed ({status})")
        } else {
            ffmpeg_error_line(&stderr)
        };
        return Err(CoreError::coded("FFMPEG_ERROR", detail));
    }

    crate::log::debug(&format!(
        "audio envelope: {} windows ({:.0} ms) of {}",
        dbs.len(),
        HOP_S * 1000.0,
        Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_path.to_string()),
    ));
    Ok(AudioEnvelope { hop: HOP_S, dbs })
}

// ---------- export ----------

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Seg {
    pub start: f64,
    pub end: f64,
}

/// Invert cut ranges into kept media segments.
pub fn invert_cuts(duration: f64, cuts: &[Change]) -> Vec<Seg> {
    let mut sorted: Vec<Seg> = cuts
        .iter()
        .map(|c| Seg {
            start: c.start.max(0.0),
            end: c.end.min(duration),
        })
        .filter(|c| c.end - c.start > 0.01)
        .collect();
    sorted.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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
            keep.push(Seg {
                start: cursor,
                end: c.start,
            });
        }
        cursor = cursor.max(c.end);
    }
    if cursor < duration - 0.02 {
        keep.push(Seg {
            start: cursor,
            end: duration,
        });
    }
    keep
}

/// Kept segments actually exported (matches the `>= 0.08s` sliver filter).
pub fn kept_segments(duration: f64, cuts: &[Change]) -> Vec<Seg> {
    let mut keep = invert_cuts(duration, cuts);
    keep.retain(|s| s.end - s.start >= 0.08);
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
    let font = if font.is_empty() {
        String::new()
    } else {
        format!(":fontfile={font}")
    };
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
        "bounce" | "punch" => vec![format!(
            "scale=iw*1.16:ih*1.16,crop=iw/1.16:ih/1.16:{}",
            en(c)
        )],
        "glitch" => vec![format!("hue=h=35:s=2:{}", en(c))],
        "spotlight" => vec![format!("vignette=PI/4:{}", en(c))],
        _ => vec![],
    }
}

fn textcard_filters(c: &Change, font: &str) -> Vec<String> {
    let text = esc_text(c.text.as_deref().unwrap_or("NOPE"));
    let font = if font.is_empty() {
        String::new()
    } else {
        format!(":fontfile={font}")
    };
    vec![
        format!("drawbox=x=0:y=0:w=iw:h=ih:color=black@1:t=fill:{}", en(c)),
        format!("drawtext=text='{text}'{font}:fontsize=h*0.12:fontcolor=0xFFE566:borderw=6:bordercolor=black:x=(w-text_w)/2:y=(h-text_h)/2:{}", en(c)),
    ]
}

/// Build an FFmpeg `filter_complex` script for video+audio cuts with optional
/// micro-fades and optional inline podcast normalization.
fn build_video_cuts_filter(
    segs: &[Seg],
    start_offset: f64,
    total_duration: f64,
    crossfade_ms: f64,
    inline_norm: Option<f64>,
    is_vaapi: bool,
) -> String {
    let fade_s = (crossfade_ms / 1000.0).clamp(0.0, 0.05);
    let mut script = String::new();
    let mut concat_inputs = String::new();

    for (i, seg) in segs.iter().enumerate() {
        let s0 = (seg.start + start_offset).max(0.0);
        let s1 = (seg.end + start_offset).max(s0 + 0.01);
        let dur = seg.end - seg.start;

        script.push_str(&format!(
            "[0:v]trim=start={s0:.3}:end={s1:.3},setpts=PTS-STARTPTS[v{i}];\n"
        ));

        let mut a_chain = format!("[0:a]atrim=start={s0:.3}:end={s1:.3},asetpts=PTS-STARTPTS");
        if fade_s > 0.001 && dur > 0.03 {
            let f = fade_s.min(dur / 3.0);
            if seg.start > 0.02 {
                a_chain.push_str(&format!(",afade=t=in:st=0:d={f:.4}"));
            }
            if seg.end < total_duration - 0.02 {
                let st = (dur - f).max(0.0);
                a_chain.push_str(&format!(",afade=t=out:st={st:.4}:d={f:.4}"));
            }
        }
        a_chain.push_str(&format!("[a{i}];\n"));
        script.push_str(&a_chain);

        concat_inputs.push_str(&format!("[v{i}][a{i}]"));
    }

    let n = segs.len();
    if is_vaapi {
        script.push_str(&format!(
            "{concat_inputs}concat=n={n}:v=1:a=1[vcat][acat];\n[vcat]format=nv12,hwupload[vout]"
        ));
    } else {
        script.push_str(&format!(
            "{concat_inputs}concat=n={n}:v=1:a=1[vout][acat]"
        ));
    }

    if let Some(amount) = inline_norm {
        let af = podcast_af(amount);
        script.push_str(&format!(";\n[acat]{af}[aout]\n"));
    } else {
        script.push_str(";\n[acat]anull[aout]\n");
    }

    script
}

/// Build an FFmpeg `filter_complex` script for audio-only cuts.
fn build_audio_cuts_filter(
    segs: &[Seg],
    start_offset: f64,
    total_duration: f64,
    crossfade_ms: f64,
    inline_norm: Option<f64>,
) -> String {
    let fade_s = (crossfade_ms / 1000.0).clamp(0.0, 0.05);
    let mut script = String::new();
    let mut concat_inputs = String::new();

    for (i, seg) in segs.iter().enumerate() {
        let s0 = (seg.start + start_offset).max(0.0);
        let s1 = (seg.end + start_offset).max(s0 + 0.01);
        let dur = seg.end - seg.start;

        let mut a_chain = format!("[0:a]atrim=start={s0:.3}:end={s1:.3},asetpts=PTS-STARTPTS");
        if fade_s > 0.001 && dur > 0.03 {
            let f = fade_s.min(dur / 3.0);
            if seg.start > 0.02 {
                a_chain.push_str(&format!(",afade=t=in:st=0:d={f:.4}"));
            }
            if seg.end < total_duration - 0.02 {
                let st = (dur - f).max(0.0);
                a_chain.push_str(&format!(",afade=t=out:st={st:.4}:d={f:.4}"));
            }
        }
        a_chain.push_str(&format!("[a{i}];\n"));
        script.push_str(&a_chain);
        concat_inputs.push_str(&format!("[a{i}]"));
    }

    let n = segs.len();
    script.push_str(&format!("{concat_inputs}concat=n={n}:v=0:a=1[acat]"));
    if let Some(amount) = inline_norm {
        let af = podcast_af(amount);
        script.push_str(&format!(";\n[acat]{af}[aout]\n"));
    } else {
        script.push_str(";\n[acat]anull[aout]\n");
    }
    script
}

/// Choose appropriate audio encoder flags for an output file extension.
fn audio_codec_args_for_path(path: &Path) -> Vec<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp3" => vec![s("-c:a"), s("libmp3lame"), s("-b:a"), s("192k")],
        "wav" => vec![s("-c:a"), s("pcm_s16le")],
        "flac" => vec![s("-c:a"), s("flac")],
        "ogg" | "opus" => vec![s("-c:a"), s("libopus"), s("-b:a"), s("160k")],
        "m4a" | "aac" | "mp4" | "mov" => {
            vec![s("-c:a"), s("aac"), s("-b:a"), s("192k"), s("-ar"), s("48000")]
        }
        _ => vec![s("-c:a"), s("aac"), s("-b:a"), s("192k")],
    }
}

fn is_mp4_like(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "m4v" | "m4a" | "mov")
    )
}

/// Backwards-compatible export entry point (used by Tauri shell).
pub fn export_project<F>(payload: &ExportPayload, progress: F) -> CoreResult<()>
where
    F: FnMut(ExportProgress),
{
    export_project_with_options(payload, &ExportOptions::default(), progress)
}

/// Export a video or audio project with full control over encoder, quality,
/// crossfades, fast-copy mode, and concurrency.
pub fn export_project_with_options<F>(
    payload: &ExportPayload,
    opts: &ExportOptions,
    progress: F,
) -> CoreResult<()>
where
    F: FnMut(ExportProgress),
{
    let mut progress = progress;
    let mut last_log_bucket: i32 = -1;
    let mut report = |pct: u32, label: String| {
        let pct = pct.min(100);
        progress(ExportProgress {
            progress: pct,
            label: label.clone(),
        });
        let bucket = (pct / 5) * 5;
        if pct == 0 || pct == 100 || bucket as i32 != last_log_bucket {
            last_log_bucket = bucket as i32;
            crate::log::debug(&format!("export: {pct}% — {label}"));
        }
    };

    crate::log::debug(&format!(
        "export: {} → {} ({} change(s))",
        payload.input,
        payload.output,
        payload
            .changes
            .iter()
            .filter(|c| c.status == "accepted")
            .count()
    ));
    report(0, "Preparing export".into());
    let t0 = std::time::Instant::now();

    let accepted: Vec<&Change> = payload
        .changes
        .iter()
        .filter(|c| c.status == "accepted")
        .collect();
    let cuts: Vec<Change> = accepted
        .iter()
        .filter(|c| c.kind == "cut")
        .map(|c| (*c).clone())
        .collect();
    let overlays: Vec<Change> = accepted
        .iter()
        .filter(|c| {
            (c.kind == "overlay" || c.kind == "textcard" || c.kind == "impact") && c.text.is_some()
        })
        .map(|c| (*c).clone())
        .collect();
    let pans: Vec<Change> = accepted
        .iter()
        .filter(|c| c.kind == "pan")
        .map(|c| (*c).clone())
        .collect();
    let fx: Vec<Change> = accepted
        .iter()
        .filter(|c| {
            [
                "shake",
                "tilt",
                "flash",
                "bounce",
                "glitch",
                "spotlight",
                "slow",
                "fast",
                "freeze",
                "punch",
            ]
            .contains(&c.kind.as_str())
        })
        .map(|c| (*c).clone())
        .collect();

    let ffmpeg = resolve_bin("ffmpeg");
    let meta = probe(&payload.input)?;
    let out_path = Path::new(&payload.output);
    let tmp = TempDirGuard::create(out_path.parent(), opts.temp_dir.as_deref())?;
    let out_ext = out_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or(if meta.has_video { "mp4" } else { "m4a" });
    let staged_final = tmp.path().join(format!("final.{out_ext}"));

    // 1. Normalize-only path (no cuts or visual edits).
    if cuts.is_empty() && overlays.is_empty() && pans.is_empty() && fx.is_empty() {
        if !payload.normalize {
            return Err(CoreError::coded(
                "NOTHING_TO_EXPORT",
                "Nothing to export — add a cut or turn on volume normalize.",
            ));
        }
        crate::log::debug("export: normalize only (copy video, re-encode audio)");
        report(10, "Normalizing audio".into());
        apply_podcast_norm_file(
            &ffmpeg,
            Path::new(&payload.input),
            &staged_final,
            meta.has_video,
            payload.normalize_amount,
            meta.duration,
            |frac| {
                let pct = 10 + (frac * 85.0).round() as u32;
                report(pct, "Normalizing audio".into());
            },
        )?;
        verify_media(&staged_final)?;
        finalize_file(&staged_final, out_path)?;
        report(100, "Export done".into());
        crate::log::debug(&format!("export done in {:.1?}", t0.elapsed()));
        return Ok(());
    }

    let keep = kept_segments(payload.duration, &cuts);
    if keep.is_empty() {
        return Err(CoreError::coded(
            "NOTHING_LEFT",
            "Nothing left to export after cuts.",
        ));
    }
    let expected_dur: f64 = keep.iter().map(|seg| seg.end - seg.start).sum();

    // 2. Audio-only export path (podcasts / voiceovers).
    if !meta.has_video {
        report(
            5,
            format!("Trimming audio ({} kept segment(s))", keep.len()),
        );
        let inline_norm = payload.normalize.then_some(payload.normalize_amount);
        if keep.len() <= SINGLE_PASS_MAX_SEGS {
            let script = build_audio_cuts_filter(
                &keep,
                meta.start_time,
                payload.duration,
                opts.crossfade_ms,
                inline_norm,
            );
            let mut args = vec![
                s("-y"),
                s("-i"),
                payload.input.clone(),
                s("-filter_complex"),
                script,
                s("-map"),
                s("[aout]"),
                s("-vn"),
            ];
            args.extend(audio_codec_args_for_path(out_path));
            if is_mp4_like(out_path) {
                args.extend([s("-movflags"), s("+faststart")]);
            }
            args.push(staged_final.to_string_lossy().into_owned());
            run_with_progress(&ffmpeg, &args, expected_dur, |frac| {
                let pct = 5 + (frac * 90.0).round() as u32;
                report(pct, "Encoding audio".into());
            })?;
        } else {
            // Chunk audio into lossless WAV pieces, then concat + final encode/norm once.
            let chunks: Vec<&[Seg]> = keep.chunks(CHUNK_SEGS).collect();
            let mut chunk_files: Vec<PathBuf> = Vec::with_capacity(chunks.len());
            for (ci, chunk) in chunks.iter().enumerate() {
                let cpath = tmp.path().join(format!("achunk-{ci}.wav"));
                let script = build_audio_cuts_filter(
                    chunk,
                    meta.start_time,
                    payload.duration,
                    opts.crossfade_ms,
                    None,
                );
                let seek = (chunk[0].start - 0.5).max(0.0);
                let end_t = chunk.last().unwrap().end + 0.5;
                run(
                    &ffmpeg,
                    &[
                        s("-y"),
                        s("-ss"),
                        format!("{seek:.3}"),
                        s("-to"),
                        format!("{end_t:.3}"),
                        s("-copyts"),
                        s("-i"),
                        payload.input.clone(),
                        s("-filter_complex"),
                        script,
                        s("-map"),
                        s("[aout]"),
                        s("-vn"),
                        s("-c:a"),
                        s("pcm_s16le"),
                        cpath.to_string_lossy().into_owned(),
                    ],
                )?;
                chunk_files.push(cpath);
                let pct = 5 + (((ci + 1) as f64 / chunks.len() as f64) * 75.0).round() as u32;
                report(pct, format!("Trimmed audio chunk {}/{}", ci + 1, chunks.len()));
            }
            let list_path = tmp.path().join("alist.txt");
            let list = chunk_files
                .iter()
                .map(|p| format!("file '{}'", p.to_string_lossy().replace('\'', "'\\''")))
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&list_path, list)?;
            let mut args = vec![
                s("-y"),
                s("-f"),
                s("concat"),
                s("-safe"),
                s("0"),
                s("-i"),
                list_path.to_string_lossy().into_owned(),
            ];
            if let Some(amount) = inline_norm {
                args.extend([s("-af"), podcast_af(amount)]);
            }
            args.extend(audio_codec_args_for_path(out_path));
            if is_mp4_like(out_path) {
                args.extend([s("-movflags"), s("+faststart")]);
            }
            args.push(staged_final.to_string_lossy().into_owned());
            report(85, "Finalizing audio".into());
            run(&ffmpeg, &args)?;
        }

        verify_media(&staged_final)?;
        finalize_file(&staged_final, out_path)?;
        report(100, "Export done".into());
        crate::log::debug(&format!("export done in {:.1?}", t0.elapsed()));
        return Ok(());
    }

    let fps = if meta.fps > 1.0 { meta.fps } else { 30.0 };
    let cuts_only = overlays.is_empty() && pans.is_empty() && fx.is_empty();

    // 3. Fast stream-copy mode (keyframe-snapped, cuts-only).
    if cuts_only && opts.fast_copy {
        report(
            5,
            format!("Fast stream-copying {} segment(s)", keep.len()),
        );
        let mut parts = Vec::with_capacity(keep.len());
        for (i, seg) in keep.iter().enumerate() {
            let seg_path = tmp.path().join(format!("copy-{i}.{out_ext}"));
            run(
                &ffmpeg,
                &[
                    s("-y"),
                    s("-ss"),
                    format!("{:.3}", seg.start),
                    s("-to"),
                    format!("{:.3}", seg.end),
                    s("-i"),
                    payload.input.clone(),
                    s("-c"),
                    s("copy"),
                    s("-avoid_negative_ts"),
                    s("make_zero"),
                    seg_path.to_string_lossy().into_owned(),
                ],
            )?;
            parts.push(seg_path);
            let pct = 5 + (((i + 1) as f64 / keep.len() as f64) * 80.0).round() as u32;
            report(pct, format!("Copied segment {}/{}", i + 1, keep.len()));
        }
        let list_path = tmp.path().join("list.txt");
        let list = parts
            .iter()
            .map(|p| format!("file '{}'", p.to_string_lossy().replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&list_path, list)?;
        report(88, "Joining segments".into());
        let joined = tmp.path().join(format!("joined.{out_ext}"));
        let mut args = vec![
            s("-y"),
            s("-f"),
            s("concat"),
            s("-safe"),
            s("0"),
            s("-i"),
            list_path.to_string_lossy().into_owned(),
            s("-c"),
            s("copy"),
        ];
        if is_mp4_like(out_path) && !payload.normalize {
            args.extend([s("-movflags"), s("+faststart")]);
        }
        args.push(joined.to_string_lossy().into_owned());
        run(&ffmpeg, &args)?;
        if payload.normalize {
            report(92, "Normalizing audio".into());
            apply_podcast_norm_file(
                &ffmpeg,
                &joined,
                &staged_final,
                true,
                payload.normalize_amount,
                expected_dur,
                |_| {},
            )?;
        } else {
            finalize_file(&joined, &staged_final)?;
        }
        verify_media(&staged_final)?;
        finalize_file(&staged_final, out_path)?;
        report(100, "Export done".into());
        crate::log::debug(&format!("export done in {:.1?}", t0.elapsed()));
        return Ok(());
    }

    // 4. Fast cuts-only video export (single-pass or chunked-parallel).
    if cuts_only {
        let (enc_name, enc_args, is_vaapi) = video_encoder_args(opts);
        crate::log::debug(&format!(
            "export: {} keep-segment(s) @ {:.2} fps using {enc_name}",
            keep.len(),
            fps
        ));
        report(
            5,
            format!(
                "Exporting {} segment(s) @ {:.2} fps ({enc_name})",
                keep.len(),
                fps
            ),
        );

        if keep.len() <= SINGLE_PASS_MAX_SEGS {
            // ONE FFmpeg pass: trim + micro-fades + concat + inline norm + faststart!
            let inline_norm = payload.normalize.then_some(payload.normalize_amount);
            let script = build_video_cuts_filter(
                &keep,
                meta.start_time,
                payload.duration,
                opts.crossfade_ms,
                inline_norm,
                is_vaapi,
            );

            let mut args = vec![s("-y")];
            if is_vaapi {
                args.extend([s("-vaapi_device"), s("/dev/dri/renderD128")]);
            }
            args.extend([
                s("-i"),
                payload.input.clone(),
                s("-filter_complex"),
                script,
                s("-map"),
                s("[vout]"),
                s("-map"),
                s("[aout]"),
                s("-r"),
                format!("{fps:.3}"),
            ]);
            args.extend(cfr_args());
            args.extend(enc_args);
            args.extend(audio_codec_args_for_path(out_path));
            if is_mp4_like(out_path) {
                args.extend([s("-movflags"), s("+faststart")]);
            }
            args.push(staged_final.to_string_lossy().into_owned());

            run_with_progress(&ffmpeg, &args, expected_dur, |frac| {
                let pct = 5 + (frac * 92.0).round() as u32;
                report(pct, "Encoding video".into());
            })?;
        } else {
            // Chunked parallel export: ~40 segments per chunk across N workers,
            // using lossless PCM audio inside MKV chunks when normalizing so AAC
            // is only encoded ONCE at the end.
            let chunks: Vec<&[Seg]> = keep.chunks(CHUNK_SEGS).collect();
            let total_chunks = chunks.len();
            let workers = if opts.jobs == 0 {
                std::thread::available_parallelism()
                    .map(|n| (n.get() / 2).max(1))
                    .unwrap_or(2)
                    .clamp(1, 4)
            } else {
                opts.jobs.clamp(1, 8)
            }
            .min(total_chunks);

            let chunk_ext = if payload.normalize { "mkv" } else { "mp4" };
            let next_idx = AtomicUsize::new(0);
            let done_count = AtomicUsize::new(0);
            let first_err: Mutex<Option<CoreError>> = Mutex::new(None);
            let (tx, rx) = std::sync::mpsc::channel::<usize>();

            std::thread::scope(|scope| {
                for _ in 0..workers {
                    let next_idx = &next_idx;
                    let done_count = &done_count;
                    let first_err = &first_err;
                    let tx = tx.clone();
                    let chunks = &chunks;
                    let enc_args = &enc_args;
                    let ffmpeg = &ffmpeg;
                    let tmp_path = tmp.path();
                    scope.spawn(move || loop {
                        if first_err.lock().map(|g| g.is_some()).unwrap_or(false) {
                            break;
                        }
                        let ci = next_idx.fetch_add(1, Ordering::Relaxed);
                        if ci >= total_chunks {
                            break;
                        }
                        let chunk = chunks[ci];
                        let chunk_path = tmp_path.join(format!("chunk-{ci:04}.{chunk_ext}"));
                        let script = build_video_cuts_filter(
                            chunk,
                            meta.start_time,
                            payload.duration,
                            opts.crossfade_ms,
                            None,
                            is_vaapi,
                        );
                        let seek = (chunk[0].start - 1.5).max(0.0);
                        let end_t = (chunk.last().unwrap().end + 1.0).min(payload.duration + 1.0);
                        let mut args = vec![s("-y")];
                        if is_vaapi {
                            args.extend([s("-vaapi_device"), s("/dev/dri/renderD128")]);
                        }
                        args.extend([
                            s("-ss"),
                            format!("{seek:.3}"),
                            s("-to"),
                            format!("{end_t:.3}"),
                            s("-copyts"),
                            s("-i"),
                            payload.input.clone(),
                            s("-filter_complex"),
                            script,
                            s("-map"),
                            s("[vout]"),
                            s("-map"),
                            s("[aout]"),
                            s("-r"),
                            format!("{fps:.3}"),
                        ]);
                        args.extend(cfr_args());
                        args.extend(enc_args.iter().cloned());
                        if payload.normalize {
                            args.extend([s("-c:a"), s("pcm_s16le"), s("-ar"), s("48000")]);
                        } else {
                            args.extend(audio_codec_args_for_path(out_path));
                        }
                        args.push(chunk_path.to_string_lossy().into_owned());

                        if let Err(e) = run(ffmpeg, &args) {
                            let mut g = first_err.lock().unwrap();
                            if g.is_none() {
                                *g = Some(e);
                            }
                            break;
                        }
                        let done = done_count.fetch_add(1, Ordering::Relaxed) + 1;
                        let _ = tx.send(done);
                    });
                }
                drop(tx);
                for done in rx {
                    let pct = 5 + ((done as f64 / total_chunks as f64) * 80.0).round() as u32;
                    report(pct, format!("Encoded chunk {done}/{total_chunks}"));
                }
            });

            if let Some(err) = first_err.into_inner().ok().flatten() {
                return Err(err);
            }

            let list_path = tmp.path().join("list.txt");
            let list = (0..total_chunks)
                .map(|ci| {
                    let p = tmp.path().join(format!("chunk-{ci:04}.{chunk_ext}"));
                    format!("file '{}'", p.to_string_lossy().replace('\'', "'\\''"))
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&list_path, list)?;

            if payload.normalize {
                report(88, "Joining chunks and normalizing audio".into());
                let af = podcast_af(payload.normalize_amount);
                let mut args = vec![
                    s("-y"),
                    s("-f"),
                    s("concat"),
                    s("-safe"),
                    s("0"),
                    s("-i"),
                    list_path.to_string_lossy().into_owned(),
                    s("-c:v"),
                    s("copy"),
                    s("-af"),
                    af,
                ];
                args.extend(audio_codec_args_for_path(out_path));
                if is_mp4_like(out_path) {
                    args.extend([s("-movflags"), s("+faststart")]);
                }
                args.push(staged_final.to_string_lossy().into_owned());
                run_with_progress(&ffmpeg, &args, expected_dur, |frac| {
                    let pct = 88 + (frac * 10.0).round() as u32;
                    report(pct, "Joining & normalizing".into());
                })?;
            } else {
                report(90, "Joining chunks".into());
                let mut args = vec![
                    s("-y"),
                    s("-f"),
                    s("concat"),
                    s("-safe"),
                    s("0"),
                    s("-i"),
                    list_path.to_string_lossy().into_owned(),
                    s("-c"),
                    s("copy"),
                ];
                if is_mp4_like(out_path) {
                    args.extend([s("-movflags"), s("+faststart")]);
                }
                args.push(staged_final.to_string_lossy().into_owned());
                run(&ffmpeg, &args)?;
            }
        }

        verify_media(&staged_final)?;
        finalize_file(&staged_final, out_path)?;
        report(100, "Export done".into());
        crate::log::debug(&format!("export done in {:.1?}", t0.elapsed()));
        return Ok(());
    }

    // 5. Legacy per-segment path when desktop overlays/pans/FX are present.
    crate::log::debug(&format!(
        "export (fx): {} keep-segment(s) @ {:.2} fps",
        keep.len(),
        fps
    ));
    report(
        5,
        format!("Exporting {} keep-segment(s) @ {:.2} fps", keep.len(), fps),
    );
    let font = pick_font();
    let mut parts: Vec<PathBuf> = Vec::new();
    let (_, enc_args, _) = video_encoder_args(opts);

    for (i, seg) in keep.iter().enumerate() {
        let seg_path = tmp.path().join(format!("seg-{i}.mp4"));
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
        let speed = local_fx
            .iter()
            .find(|f| f.kind == "slow" || f.kind == "fast" || f.kind == "freeze");
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
            s("-y"),
            s("-ss"),
            format!("{}", seg.start),
            s("-to"),
            format!("{}", seg.end),
            s("-i"),
            payload.input.clone(),
        ];
        if !vf.is_empty() {
            args.push(s("-vf"));
            args.push(
                vf.into_iter()
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if let Some(sp) = speed {
            if sp.kind != "freeze" {
                let tempo = rate.clamp(0.5, 2.0);
                args.push(s("-af"));
                args.push(format!("atempo={tempo}"));
            }
        }
        args.extend([s("-r"), format!("{fps:.3}")]);
        args.extend(cfr_args());
        args.extend(enc_args.iter().cloned());
        args.extend(audio_codec_args_for_path(out_path));
        args.push(seg_path.to_string_lossy().into_owned());
        run(&ffmpeg, &args)?;
        parts.push(seg_path);
        let pct = 5 + (((i + 1) as f64 / keep.len().max(1) as f64) * 85.0).round() as u32;
        report(pct, format!("Encoded segment {}/{}", i + 1, keep.len()));
    }

    report(92, "Joining segments".into());
    let list_path = tmp.path().join("list.txt");
    let list = parts
        .iter()
        .map(|p| format!("file '{}'", p.to_string_lossy().replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_path, list)?;
    let joined = tmp.path().join(format!("joined.{out_ext}"));
    run(
        &ffmpeg,
        &[
            s("-y"),
            s("-f"),
            s("concat"),
            s("-safe"),
            s("0"),
            s("-i"),
            list_path.to_string_lossy().into_owned(),
            s("-c"),
            s("copy"),
            s("-movflags"),
            s("+faststart"),
            joined.to_string_lossy().into_owned(),
        ],
    )?;

    if payload.normalize {
        report(96, "Normalizing audio".into());
        apply_podcast_norm_file(
            &ffmpeg,
            &joined,
            &staged_final,
            true,
            payload.normalize_amount,
            expected_dur,
            |_| {},
        )?;
    } else {
        finalize_file(&joined, &staged_final)?;
    }

    verify_media(&staged_final)?;
    finalize_file(&staged_final, out_path)?;
    report(100, "Export done".into());
    crate::log::debug(&format!("export done in {:.1?}", t0.elapsed()));
    Ok(())
}

/// Podcast evenness: high-pass rumble, dynaudnorm to lift far-from-mic speech
/// (capped so noise doesn't roar), then loudnorm to −16 LUFS with a tight LRA.
pub fn podcast_af(amount: f64) -> String {
    let a = if amount.is_finite() {
        amount.clamp(0.0, 1.0)
    } else {
        0.7
    };
    let maxgain = 5.0 + a * 11.0; // linear factor ≈ 14–24 dB
    let compress = 4.0 + a * 8.0;
    let lra = 9.0 - a * 4.0; // 9 … 5 LU
    format!(
        "highpass=f=80,dynaudnorm=f=180:g=17:p=0.93:m={maxgain:.1}:r=0.16:s={compress:.1},loudnorm=I=-16:TP=-1.5:LRA={lra:.1}"
    )
}

fn apply_podcast_norm_file<F>(
    ffmpeg: &str,
    input: &Path,
    output: &Path,
    has_video: bool,
    amount: f64,
    expected_dur: f64,
    on_fraction: F,
) -> CoreResult<()>
where
    F: FnMut(f64),
{
    let af = podcast_af(amount);
    crate::log::debug(&format!("podcast norm: {af}"));
    let mut args = vec![s("-y"), s("-i"), input.to_string_lossy().into_owned()];
    if has_video {
        args.extend([s("-c:v"), s("copy")]);
    } else {
        args.push(s("-vn"));
    }
    args.extend([s("-af"), af]);
    args.extend(audio_codec_args_for_path(output));
    if is_mp4_like(output) {
        args.extend([s("-movflags"), s("+faststart")]);
    }
    args.push(output.to_string_lossy().into_owned());
    run_with_progress(ffmpeg, &args, expected_dur, on_fraction)
}

fn verify_media(path: impl AsRef<Path>) -> CoreResult<()> {
    let p = path.as_ref();
    let file_size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if file_size == 0 {
        return Err(CoreError::coded(
            "EXPORT_INVALID",
            "export produced an empty file",
        ));
    }
    let meta = probe(&p.to_string_lossy())?;
    if meta.duration <= 0.0 || !meta.duration.is_finite() {
        return Err(CoreError::coded(
            "EXPORT_INVALID",
            "export produced an incomplete file (no duration / missing container header)",
        ));
    }
    crate::log::debug(&format!(
        "export verify ok: {:.2}s ({} bytes) {}",
        meta.duration,
        file_size,
        p.display()
    ));
    Ok(())
}

/// Grab small raw RGB24 frames sampled at `fps` across [start, end] in one
/// ffmpeg pass for visual-change pixel diffing.
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
            s("-hide_banner"),
            s("-v"),
            s("error"),
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
        .map_err(|e| CoreError::coded("FFMPEG_MISSING", format!("ffmpeg range frames: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::coded(
            "FFMPEG_ERROR",
            "ffmpeg range frame extraction failed",
        ));
    }
    let frame_len = (w * h * 3) as usize;
    Ok(out
        .stdout
        .chunks_exact(frame_len)
        .map(|c| c.to_vec())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_extractor_skips_banner_and_generic_conversion_failed() {
        let stderr = "\
ffmpeg version n9.0.2 Copyright (c) 2000-2026 the FFmpeg developers
  built with gcc 16 (GCC)
  configuration: --enable-gpl --enable-libx264
Unrecognized option 'vsync'.
Error splitting the argument list: Option not found
Conversion failed!
";
        let msg = ffmpeg_error_line(stderr);
        assert!(msg.contains("Unrecognized option 'vsync'"), "got: {msg}");
        assert!(!msg.contains("Conversion failed!"), "got: {msg}");
        assert!(!msg.contains("--enable-gpl"), "got: {msg}");
    }

    #[test]
    fn eval_frac_handles_standard_and_zero_rates() {
        assert!((eval_frac("30000/1001") - 29.97002997).abs() < 1e-4);
        assert!((eval_frac("60/1") - 60.0).abs() < 1e-9);
        assert_eq!(eval_frac("0/0"), 0.0);
        assert_eq!(eval_frac(""), 0.0);
    }

    #[test]
    fn invert_cuts_merges_overlaps_and_preserves_edges() {
        let cuts = vec![
            Change {
                id: "1".into(),
                kind: "cut".into(),
                start: 2.0,
                end: 4.0,
                status: "accepted".into(),
                label: "".into(),
                rationale: "".into(),
                origin: None,
                text: None,
                style: None,
                pan: None,
                rate: None,
            },
            Change {
                id: "2".into(),
                kind: "cut".into(),
                start: 3.995,
                end: 6.0,
                status: "accepted".into(),
                label: "".into(),
                rationale: "".into(),
                origin: None,
                text: None,
                style: None,
                pan: None,
                rate: None,
            },
        ];
        let keep = invert_cuts(10.0, &cuts);
        assert_eq!(keep.len(), 2);
        assert!((keep[0].start - 0.0).abs() < 1e-9);
        assert!((keep[0].end - 2.0).abs() < 1e-9);
        assert!((keep[1].start - 6.0).abs() < 1e-9);
        assert!((keep[1].end - 10.0).abs() < 1e-9);
    }

    #[test]
    fn podcast_af_sanitizes_nan_and_bounds() {
        let nan_af = podcast_af(f64::NAN);
        assert!(!nan_af.contains("NaN"));
        assert!(nan_af.contains("loudnorm=I=-16"));

        let hi_af = podcast_af(5.0);
        assert!(hi_af.contains("m=16.0"));
        assert!(hi_af.contains("LRA=5.0"));
    }

    #[test]
    fn video_filter_builds_microfades_and_inline_norm() {
        let segs = vec![
            Seg {
                start: 0.0,
                end: 2.0,
            },
            Seg {
                start: 5.0,
                end: 10.0,
            },
        ];
        let f = build_video_cuts_filter(&segs, 0.0, 10.0, 8.0, Some(0.7), false);
        assert!(f.contains("trim=start=0.000:end=2.000"));
        assert!(f.contains("afade=t=out"));
        assert!(f.contains("afade=t=in"));
        assert!(f.contains("concat=n=2:v=1:a=1"));
        assert!(f.contains("loudnorm=I=-16"));
    }
}
