//! Ears: transcription client. The actual whisper.cpp inference runs in the
//! `mustardy-ears` sidecar process (llama.cpp and whisper.cpp both vendor
//! ggml; separate processes sidestep the symbol collision).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{self, EARS_MODEL};
use crate::{CoreError, CoreResult, Transcript};

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

/// Locate the mustardy-ears sidecar. Tauri externalBin and `npm run prep`
/// stage it triple-suffixed (`mustardy-ears-<triple>`); cargo builds it plain.
fn resolve_ears() -> CoreResult<String> {
    if let Ok(v) = std::env::var("MUSTARDY_EARS") {
        if !v.is_empty() && PathBuf::from(&v).is_file() {
            return Ok(v);
        }
    }
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let names = [format!("mustardy-ears-{TRIPLE}{ext}"), format!("mustardy-ears{ext}")];
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf()); // packaged: next to the app binary
            dirs.push(dir.join("binaries"));
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // <repo>/src-tauri/core
    dirs.push(manifest.join("../binaries")); // <repo>/src-tauri/binaries
    dirs.push(manifest.join("../../target/release")); // workspace target
    dirs.push(manifest.join("../../target/debug"));
    for dir in &dirs {
        for name in &names {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p.to_string_lossy().into_owned());
            }
        }
    }
    let tried: Vec<String> = dirs
        .iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n).to_string_lossy().into_owned()))
        .collect();
    Err(CoreError::msg(format!(
        "mustardy-ears sidecar not found (cargo build -p mustardy-ears-cli --release); tried: {}",
        tried.join(", ")
    )))
}

/// Validate that the sidecar binary and ggml model are available.
pub fn load() -> CoreResult<()> {
    models::find(&EARS_MODEL)
        .ok_or_else(|| CoreError::msg(format!("missing {} — run the model download", EARS_MODEL.file)))?;
    resolve_ears()?;
    Ok(())
}

pub fn loaded() -> bool {
    load().is_ok()
}

/// Transcribe 16 kHz mono f32 PCM via the sidecar. Word timestamps come from
/// whisper's DTW token timestamps. If the sidecar is killed outright (the
/// SIGKILL-with-empty-stderr pattern seen under macOS memory pressure while
/// the big llama.cpp models sit resident), the eyes/brain models are evicted
/// — every engine reloads lazily on its next use — and transcription runs
/// once more before giving up.
pub fn transcribe(pcm: &[f32], model_name: Option<&str>) -> CoreResult<Transcript> {
    let spec = models::ears_file(model_name);
    let model = models::find(spec)
        .ok_or_else(|| CoreError::msg(format!("missing {} — run the model download", spec.file)))?;
    let bin = resolve_ears()?;
    crate::log::line(&format!("ears sidecar: {bin}"));

    // Write PCM to a temp file
    let tmp = std::env::temp_dir().join(format!("mustardy-pcm-{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        let mut bytes = Vec::with_capacity(pcm.len() * 4);
        for s in pcm {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        f.write_all(&bytes)?;
    }

    let started = std::time::Instant::now();
    let audio_secs = pcm.len() as f64 / 16000.0;

    let mut out = match run_ears(&bin, &model, &tmp) {
        Ok(out) => out,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(CoreError::msg(format!("spawn ears: {e}")));
        }
    };
    if !out.status.success() && killed_quietly(&out) {
        crate::log::line(
            "error: ears was killed with no stderr (OS memory pressure?) — \
             unloading eyes/brain and retrying once",
        );
        crate::brain::unload();
        crate::eyes::unload();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        out = match run_ears(&bin, &model, &tmp) {
            Ok(out) => out,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(CoreError::msg(format!("spawn ears (retry): {e}")));
            }
        };
    }

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        crate::log::line(&format!(
            "error: ears died ({}) after {:.1?} on {audio_secs:.1}s of audio; stderr: {}",
            out.status,
            started.elapsed(),
            stderr.chars().take(400).collect::<String>()
        ));
        let _ = std::fs::remove_file(&tmp);
        return Err(CoreError::msg(format!(
            "ears exited {}: {}",
            out.status,
            stderr.chars().take(500).collect::<String>()
        )));
    }
    let parsed: CoreResult<Transcript> =
        serde_json::from_slice(&out.stdout).map_err(Into::into);
    let _ = std::fs::remove_file(&tmp);
    let transcript = parsed?;
    crate::log::line(&format!(
        "ears: transcribed {audio_secs:.1}s of audio, {} word(s), {} char(s)",
        transcript.words.len(),
        transcript.text.len()
    ));
    Ok(transcript)
}

fn run_ears(bin: &str, model: &Path, pcm_path: &Path) -> std::io::Result<std::process::Output> {
    Command::new(bin).arg(model).arg(pcm_path).output()
}

/// True when the sidecar died by signal (SIGKILL on unix) with nothing on
/// stderr — the signature of an OOM-style kill rather than a whisper crash.
fn killed_quietly(out: &std::process::Output) -> bool {
    if out.status.success() || !out.stderr.is_empty() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        out.status.signal() == Some(9) // SIGKILL
    }
    #[cfg(not(unix))]
    {
        // Where signals aren't exposed, 137 (128 + SIGKILL) is the usual
        // shell-level footprint of the same kill.
        out.status.code() == Some(137)
    }
}
