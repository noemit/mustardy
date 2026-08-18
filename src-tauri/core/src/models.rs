//! Model registry: where GGUF/ggml files live and how they get there.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::CoreResult;

#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    pub id: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub bytes: u64,
}

pub const EYES_MODEL: ModelFile = ModelFile {
    id: "eyes",
    file: "SmolVLM-256M-Instruct-Q8_0.gguf",
    url: "https://huggingface.co/ggml-org/SmolVLM-256M-Instruct-GGUF/resolve/main/SmolVLM-256M-Instruct-Q8_0.gguf",
    bytes: 175_000_000,
};

pub const EYES_MMPROJ: ModelFile = ModelFile {
    id: "eyes-mmproj",
    file: "mmproj-SmolVLM-256M-Instruct-f16.gguf",
    url: "https://huggingface.co/ggml-org/SmolVLM-256M-Instruct-GGUF/resolve/main/mmproj-SmolVLM-256M-Instruct-f16.gguf",
    bytes: 190_000_000,
};

pub const BRAIN_MODEL: ModelFile = ModelFile {
    id: "brain",
    file: "smollm2-1.7b-instruct-q4_k_m.gguf",
    url: "https://huggingface.co/HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF/resolve/main/smollm2-1.7b-instruct-q4_k_m.gguf",
    bytes: 1_056_000_000,
};

/// Lightweight alternative — set `MUSTARDY_BRAIN=smollm2-360m-instruct-q8_0.gguf`.
pub const BRAIN_MODEL_SMALL: ModelFile = ModelFile {
    id: "brain-small",
    file: "smollm2-360m-instruct-q8_0.gguf",
    url: "https://huggingface.co/HuggingFaceTB/SmolLM2-360M-Instruct-GGUF/resolve/main/smollm2-360m-instruct-q8_0.gguf",
    bytes: 386_000_000,
};

pub const EARS_MODEL: ModelFile = ModelFile {
    id: "ears",
    file: "ggml-tiny.en.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
    bytes: 75_000_000,
};

/// Better word timestamps for cutting from the transcript. Not downloaded
/// until the user picks small.en — ~466 MB, slower than tiny.
pub const EARS_MODEL_SMALL: ModelFile = ModelFile {
    id: "ears-small",
    file: "ggml-small.en.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
    bytes: 466_000_000,
};

pub const ALL_MODELS: &[ModelFile] = &[EYES_MODEL, EYES_MMPROJ, BRAIN_MODEL, EARS_MODEL];

pub fn ears_file(name: Option<&str>) -> &'static ModelFile {
    match name.unwrap_or("tiny.en") {
        "small.en" | "ggml-small.en.bin" => &EARS_MODEL_SMALL,
        _ => &EARS_MODEL,
    }
}

/// Per-user app data dir (no tauri dependency here).
pub fn app_data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    if cfg!(target_os = "macos") {
        PathBuf::from(home).join("Library/Application Support/studio.mustardy.editor")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var("APPDATA").unwrap_or(home)).join("studio.mustardy.editor")
    } else {
        PathBuf::from(home).join(".local/share/studio.mustardy.editor")
    }
}

/// Directories searched for model files, in priority order.
pub fn roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(v) = std::env::var("MUSTARDY_MODELS") {
        if !v.is_empty() {
            out.push(PathBuf::from(v));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // packaged macOS: Mustardy.app/Contents/Resources/models
            out.push(dir.join("../Resources/models"));
        }
    }
    out.push(app_data_dir().join("models"));
    // dev: <repo>/models (this crate lives in <repo>/src-tauri/core)
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models"));
    out
}

/// Writable dir used for downloads: $MUSTARDY_MODELS, else <repo>/models in
/// dev, else the per-user app data dir.
pub fn download_root() -> PathBuf {
    if let Ok(v) = std::env::var("MUSTARDY_MODELS") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
    if dev.exists() || cfg!(debug_assertions) {
        return dev;
    }
    app_data_dir().join("models")
}

pub fn find(file: &ModelFile) -> Option<PathBuf> {
    roots()
        .into_iter()
        .map(|r| r.join(file.file))
        .find(|p| p.is_file())
}

/// Resolve an absolute path or a bare filename against the model roots.
pub fn resolve_name(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(name);
    if p.is_absolute() && p.is_file() {
        return Some(p);
    }
    roots()
        .into_iter()
        .map(|r| r.join(name))
        .find(|p| p.is_file())
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub file: String,
    pub present: bool,
    pub bytes: u64,
    pub path: Option<String>,
}

pub fn status() -> Vec<ModelStatus> {
    ALL_MODELS
        .iter()
        .map(|m| {
            let path = find(m);
            ModelStatus {
                id: m.id.to_string(),
                file: m.file.to_string(),
                present: path.is_some(),
                bytes: path.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0),
                path: path.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}

pub fn missing() -> Vec<&'static ModelFile> {
    ALL_MODELS.iter().filter(|m| find(m).is_none()).collect()
}

/// Download one model file, reporting `(downloaded_bytes, total_bytes)`.
pub fn download<F: Fn(u64, u64)>(m: &ModelFile, progress: F) -> CoreResult<PathBuf> {
    if let Some(p) = find(m) {
        return Ok(p);
    }
    let root = download_root();
    std::fs::create_dir_all(&root)?;
    let dest = root.join(m.file);
    let tmp = root.join(format!("{}.part", m.file));

    let resp = ureq::get(m.url).call()?;
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(m.bytes);
    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(&tmp)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    drop(file);
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}
