//! Mustardy core — everything except the Tauri shell.
//!
//! This crate intentionally has **no tauri imports** so it can be built and
//! tested headlessly.

pub mod cli;
pub mod config;
pub mod ffmpeg;
pub mod log;
pub mod nle;
pub mod plan;
pub mod progress;
pub mod silence;
pub mod visual;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMeta {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    #[serde(rename = "hasAudio")]
    pub has_audio: bool,
    #[serde(rename = "hasVideo", default = "default_true")]
    pub has_video: bool,
    #[serde(rename = "startTime", default)]
    pub start_time: f64,
    #[serde(rename = "videoCodec", skip_serializing_if = "Option::is_none", default)]
    pub video_codec: Option<String>,
    #[serde(rename = "audioCodec", skip_serializing_if = "Option::is_none", default)]
    pub audio_codec: Option<String>,
    #[serde(rename = "sampleRate", skip_serializing_if = "Option::is_none", default)]
    pub sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channels: Option<u32>,
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

/// Summary audio diagnostics computed alongside silence detection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AudioStats {
    #[serde(rename = "speechDb")]
    pub speech_db: f64,
    #[serde(rename = "noiseFloorDb")]
    pub noise_floor_db: f64,
    #[serde(rename = "thresholdDb")]
    pub threshold_db: f64,
    #[serde(rename = "contrastDb")]
    pub contrast_db: f64,
    #[serde(rename = "lowContrast")]
    pub low_contrast: bool,
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
    pub origin: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoCodec {
    /// Software libx264 (default, most consistent quality/size).
    Software,
    /// Probe best available hardware H.264 encoder, falling back to libx264.
    Hardware,
    /// Explicit FFmpeg encoder name (e.g. `h264_nvenc`, `h264_videotoolbox`, `libx265`).
    Custom(String),
}

impl Default for VideoCodec {
    fn default() -> Self {
        VideoCodec::Software
    }
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub codec: VideoCodec,
    pub crf: u8,
    pub preset: String,
    /// Short audio fade-in/out at cut boundaries (milliseconds) to prevent pops/clicks.
    pub crossfade_ms: f64,
    /// Keyframe-snapped stream copy (much faster, less frame-accurate).
    pub fast_copy: bool,
    /// Parallel worker count for chunked exports (0 = auto).
    pub jobs: usize,
    /// Optional explicit temp directory (defaults to a hidden dir next to output).
    pub temp_dir: Option<PathBuf>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            codec: VideoCodec::Software,
            crf: 18,
            preset: "veryfast".to_string(),
            crossfade_ms: 8.0,
            fast_copy: false,
            jobs: 0,
            temp_dir: None,
        }
    }
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
    #[error("{message}")]
    Coded {
        code: &'static str,
        message: String,
    },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl CoreError {
    pub fn msg(s: impl Into<String>) -> Self {
        CoreError::Msg(s.into())
    }

    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        CoreError::Coded {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            CoreError::Coded { code, .. } => code,
            CoreError::Io(_) => "IO_ERROR",
            CoreError::Json(_) => "JSON_ERROR",
            CoreError::Msg(_) => "ERROR",
        }
    }
}

/// Round floating-point seconds to 3 decimal places for clean JSON output.
pub fn round3(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v * 1000.0).round() / 1000.0
}
