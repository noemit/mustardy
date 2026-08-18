//! Mustardy core — everything except the Tauri shell.
//!
//! This crate intentionally has **no tauri imports** so it can be built and
//! smoke-tested headlessly (`cargo build --bin smoke`).

pub mod ai;
pub mod brain;
pub mod ears;
pub mod eyes;
pub mod ffmpeg;
pub mod llm;
pub mod log;
pub mod models;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Caption {
    pub t: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub t: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// One edit mark produced by a planner (mirrors `Change` in src/types.ts).
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPayload {
    pub input: String,
    pub duration: f64,
    pub changes: Vec<Change>,
    pub output: String,
}

/// Progress / status for an engine, streamed to the frontend as events.
#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub engine: &'static str, // "eyes" | "brain" | "ears" | "models"
    pub state: &'static str,  // "idle" | "downloading" | "loading" | "ready" | "error"
    pub progress: u32,
    pub label: String,
}

impl EngineStatus {
    pub fn new(engine: &'static str, state: &'static str, progress: u32, label: impl Into<String>) -> Self {
        Self { engine, state, progress, label: label.into() }
    }
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
    #[error("http: {0}")]
    Http(#[from] ureq::Error),
}

impl CoreError {
    pub fn msg(s: impl Into<String>) -> Self {
        CoreError::Msg(s.into())
    }
}
