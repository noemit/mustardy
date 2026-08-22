//! Mustardy — Tauri shell. All logic lives in mustardy_core; this file only
//! wires commands and events into the webview.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine as _;
use mustardy_core::ai::{self, AiSettings};
use mustardy_core::ffmpeg;
use mustardy_core::models::{self, ModelStatus};
use mustardy_core::{brain, ears, eyes, visual};
use mustardy_core::{Caption, ChatMessage, EngineStatus, ExportPayload, SilenceRange, Transcript, VideoMeta};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};

fn emit_status(app: &AppHandle, s: EngineStatus) {
    let _ = app.emit("engine-status", s);
}

fn err(e: impl std::fmt::Display) -> String {
    let s = e.to_string();
    mustardy_core::log::line(&format!("error: {s}"));
    s
}

/// One line from the UI into the shared log file.
#[tauri::command]
fn log_line(line: String) {
    mustardy_core::log::line(&format!("ui: {line}"));
}

#[tauri::command]
async fn probe(app: AppHandle, path: String) -> Result<VideoMeta, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Grant asset-protocol read access per opened file instead of a
        // static "**" scope — the webview can only ever fetch videos the
        // user actually opened through this app.
        app.asset_protocol_scope()
            .allow_file(&path)
            .map_err(err)?;
        ffmpeg::probe(&path).map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn detect_silence(path: String, noise: Option<String>, duration: Option<f64>) -> Result<Vec<SilenceRange>, String> {
    tauri::async_runtime::spawn_blocking(move || ffmpeg::detect_silence(&path, noise.as_deref(), duration).map_err(err))
        .await
        .map_err(err)?
}

/// Per silence range, the times where the picture changes mid-pause — the
/// frontend trims around those points instead of through them.
#[tauri::command]
async fn visual_change_times(path: String, ranges: Vec<SilenceRange>) -> Result<Vec<Vec<f64>>, String> {
    tauri::async_runtime::spawn_blocking(move || visual::visual_change_times(&path, &ranges).map_err(err))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn export_project(payload: ExportPayload) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || ffmpeg::export_project(&payload).map_err(err))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn chat_ai(
    provider: String,
    settings: Option<AiSettings>,
    messages: Vec<ChatMessage>,
    system: String,
) -> Result<ai::ChatOut, String> {
    tauri::async_runtime::spawn_blocking(move || ai::chat(&provider, settings, messages, system).map_err(err))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn list_ai_models(provider: String, settings: Option<AiSettings>) -> Result<ai::ModelsOut, String> {
    tauri::async_runtime::spawn_blocking(move || ai::list_models(&provider, settings).map_err(err))
        .await
        .map_err(err)?
}

#[tauri::command]
fn model_status() -> Vec<ModelStatus> {
    models::status()
}

#[tauri::command]
async fn download_models(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let missing = models::missing();
        if missing.is_empty() {
            emit_status(&app, EngineStatus::new("models", "ready", 100, "All models present"));
            return Ok(());
        }
        let total_all: u64 = missing.iter().map(|m| m.bytes).sum();
        let mut done_all: u64 = 0;
        for m in missing {
            emit_status(&app, EngineStatus::new("models", "downloading", 0, format!("Downloading {}", m.file)));
            let app2 = app.clone();
            let done_before = done_all;
            let path = models::download(m, move |done, _total| {
                let pct = ((100 * (done_before + done)) / total_all.max(1)) as u32;
                emit_status(
                    &app2,
                    EngineStatus::new("models", "downloading", pct, format!("{} · {pct}%", m.file)),
                );
            })
            .map_err(err)?;
            done_all = done_before + std::fs::metadata(&path).map(|x| x.len()).unwrap_or(m.bytes);
        }
        emit_status(&app, EngineStatus::new("models", "ready", 100, "All models downloaded"));
        Ok(())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn warm_engine(app: AppHandle, engine: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let name: &'static str = match engine.as_str() {
            "eyes" => "eyes",
            "brain" => "brain",
            _ => "ears",
        };
        emit_status(&app, EngineStatus::new(name, "loading", 50, format!("Loading {name}…")));
        let res = match engine.as_str() {
            "eyes" => eyes::load(),
            "brain" => brain::load(),
            _ => ears::load(),
        };
        match &res {
            Ok(()) => emit_status(&app, EngineStatus::new(name, "ready", 100, format!("{name} ready"))),
            Err(e) => emit_status(&app, EngineStatus::new(name, "error", 0, e.to_string())),
        }
        res.map_err(err)
    })
    .await
    .map_err(err)?
}

#[derive(Debug, Deserialize)]
struct FrameIn {
    t: f64,
    /// base64-encoded JPEG/PNG (data URL prefix already stripped by the frontend)
    data: String,
}

#[tauri::command]
async fn caption_frames(frames: Vec<FrameIn>, max_tokens: Option<usize>) -> Result<Vec<Caption>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut decoded = Vec::with_capacity(frames.len());
        for f in &frames {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&f.data)
                .map_err(err)?;
            decoded.push((f.t, bytes));
        }
        Ok(eyes::caption_frames(&decoded, max_tokens.unwrap_or(32)))
    })
    .await
    .map_err(err)?
}

/// Ask the brain to pick tools. Retries until it emits parseable JSON with a
/// non-empty changes array; Ok(None) means give up (frontend falls back to
/// the rule-based planner). When `path` is given, cut points are then snapped
/// to the exact moment with the eyes (silence-pinned cuts are left alone).
#[tauri::command]
async fn brain_plan(
    system: String,
    user: String,
    max_tokens: Option<usize>,
    attempts: Option<usize>,
    path: Option<String>,
    silences: Option<Vec<(f64, f64)>>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let refine = path.map(|p| brain::RefineCtx {
            path: p,
            silences: silences.unwrap_or_default(),
        });
        brain::plan(
            &system,
            &user,
            max_tokens.unwrap_or(420),
            attempts.unwrap_or(3),
            refine.as_ref(),
        )
        .map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn write_text(path: String, contents: String) -> Result<(), String> {
    ensure_text_scope(&path)?;
    std::fs::write(&path, contents).map_err(err)
}

#[tauri::command]
fn read_text(path: String) -> Result<String, String> {
    ensure_text_scope(&path)?;
    let meta = std::fs::metadata(&path).map_err(err)?;
    if meta.len() > MAX_READ_BYTES {
        return Err("project file too large (>16 MB)".into());
    }
    std::fs::read_to_string(&path).map_err(err)
}

/// Text-file commands only ever serve projects and transcript exports, both
/// produced by native dialogs — pin them to those extensions (and cap reads)
/// so a compromised webview can't browse arbitrary files.
const TEXT_FILE_EXTS: [&str; 3] = ["json", "txt", "srt"];
const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;

fn ensure_text_scope(path: &str) -> Result<(), String> {
    let ext_ok = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| TEXT_FILE_EXTS.contains(&e.as_str()));
    if ext_ok {
        Ok(())
    } else {
        mustardy_core::log::line(&format!("blocked out-of-scope file op: {path}"));
        Err(format!(
            "Mustardy only reads/writes {} files",
            TEXT_FILE_EXTS.join(", ")
        ))
    }
}

/// Provider API keys are persisted by the desktop app (app-data dir,
/// user-only perms), never in webview localStorage.
#[tauri::command]
fn save_kimi_key(key: String) -> Result<(), String> {
    ai::store_kimi_key(&key).map_err(err)
}

#[tauri::command]
fn kimi_key_saved() -> bool {
    ai::load_kimi_key().is_some()
}

#[tauri::command]
async fn transcribe(path: String, model: Option<String>) -> Result<Transcript, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let spec = models::ears_file(model.as_deref());
        if models::find(spec).is_none() {
            models::download(spec, |_, _| {}).map_err(err)?;
        }
        let pcm = ffmpeg::extract_pcm_16k(&path).map_err(err)?;
        ears::transcribe(&pcm, model.as_deref()).map_err(err)
    })
    .await
    .map_err(err)?
}

fn main() {
    mustardy_core::log::line(&format!("mustardy starting (pid {})", std::process::id()));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            probe,
            detect_silence,
            visual_change_times,
            export_project,
            chat_ai,
            list_ai_models,
            model_status,
            download_models,
            warm_engine,
            caption_frames,
            brain_plan,
            transcribe,
            write_text,
            read_text,
            save_kimi_key,
            kimi_key_saved,
            log_line,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mustardy");
}
