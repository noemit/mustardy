//! Mustardy — Tauri shell. All logic lives in mustardy_core; this file only
//! wires commands and events into the webview.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mustardy_core::ffmpeg;
use mustardy_core::visual;
use mustardy_core::{AudioEnvelope, ExportPayload, ExportProgress, SilenceRange, VideoMeta};
use tauri::{AppHandle, Emitter, Manager};

fn emit_export(app: &AppHandle, p: ExportProgress) {
    let _ = app.emit("export-progress", p);
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
async fn audio_envelope(path: String) -> Result<AudioEnvelope, String> {
    tauri::async_runtime::spawn_blocking(move || ffmpeg::audio_envelope(&path).map_err(err))
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
async fn export_project(app: AppHandle, payload: ExportPayload) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        ffmpeg::export_project(&payload, |p| emit_export(&app, p)).map_err(err)
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

/// Text-file commands only ever serve project files, both produced by native
/// dialogs — pin them to that extension (and cap reads) so a compromised
/// webview can't browse arbitrary files.
const TEXT_FILE_EXTS: [&str; 1] = ["json"];
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

fn main() {
    // CLI mode: any real argument (macOS Finder's `-psn_…` cookie aside) means
    // run headless and exit — `mustardy trim in.mp4 -o out.mp4`, `--help`, …
    // No arguments opens the GUI.
    let cli_args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("-psn"))
        .collect();
    if !cli_args.is_empty() {
        std::process::exit(mustardy_core::cli::run(&cli_args));
    }

    mustardy_core::log::line(&format!("mustardy starting (pid {})", std::process::id()));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            probe,
            audio_envelope,
            visual_change_times,
            export_project,
            write_text,
            read_text,
            log_line,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mustardy");
}
