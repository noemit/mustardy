//! Mustardy core — everything except the Tauri shell.
//!
//! This crate intentionally has **no tauri imports** so it can be built and
//! tested headlessly.

pub mod ffmpeg;
pub mod log;
pub mod visual;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMeta {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    #[serde(rename = "hasAudio")]
    pub has_audio: bool,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SilenceRange {
    pub start: f64,
    pub end: f64,
}

/// RMS windows in dBFS. The UI re-thresholds this as the user drags sliders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioEnvelope {
    pub hop: f64,
    pub dbs: Vec<f32>,
}

/// One edit mark (mirrors `Change` in src/types.ts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub start: f64,
    pub end: f64,
    pub status: String,
    pub label: String,
    pub rationale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pan: Option<Pan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pan {
    pub kind: String,
}

fn default_normalize_amount() -> f64 {
    0.7
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPayload {
    pub input: String,
    pub duration: f64,
    pub changes: Vec<Change>,
    pub output: String,
    #[serde(default)]
    pub normalize: bool,
    #[serde(default = "default_normalize_amount", rename = "normalizeAmount")]
    pub normalize_amount: f64,
}

/// FFmpeg export progress, streamed to the frontend as `export-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct ExportProgress {
    pub progress: u32,
    pub label: String,
}

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Msg(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl CoreError {
    pub fn msg(s: impl Into<String>) -> Self {
        CoreError::Msg(s.into())
    }
}
