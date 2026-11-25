//! Optional structured logging.
//!
//! By default the CLI does **not** write any files to disk and does **not**
//! spam stderr with internal diagnostics (`resolve ffmpeg`, etc.).
//! - Set `MUSTARDY_LOG=/path/to/file.log` (or call `set_file`) to enable
//!   append-only file logging (truncated past 2 MB).
//! - Call `set_verbosity(Verbosity::Verbose)` (`-v` / `--verbose`) to echo
//!   internal diagnostics to stderr.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 2_000_000;
const KEEP_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Verbosity {
    Quiet = 0,
    Normal = 1,
    Verbose = 2,
}

static VERBOSITY: AtomicU8 = AtomicU8::new(Verbosity::Normal as u8);
static LOG_FILE_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

pub fn set_verbosity(v: Verbosity) {
    VERBOSITY.store(v as u8, Ordering::Relaxed);
}

pub fn verbosity() -> Verbosity {
    match VERBOSITY.load(Ordering::Relaxed) {
        0 => Verbosity::Quiet,
        2 => Verbosity::Verbose,
        _ => Verbosity::Normal,
    }
}

pub fn is_quiet() -> bool {
    verbosity() == Verbosity::Quiet
}

pub fn is_verbose() -> bool {
    verbosity() == Verbosity::Verbose
}

pub fn set_file(path: Option<PathBuf>) {
    let lock = LOG_FILE_OVERRIDE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = path;
    }
}

fn resolved_log_path() -> Option<PathBuf> {
    if let Some(lock) = LOG_FILE_OVERRIDE.get() {
        if let Ok(guard) = lock.lock() {
            if let Some(p) = guard.as_ref() {
                return Some(p.clone());
            }
        }
    }
    if let Ok(v) = std::env::var("MUSTARDY_LOG") {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    None
}

fn rotate_if_needed(p: &PathBuf) {
    static DID: OnceLock<()> = OnceLock::new();
    DID.get_or_init(|| {
        let Ok(meta) = std::fs::metadata(p) else {
            return;
        };
        if meta.len() <= MAX_BYTES {
            return;
        }
        let Ok(data) = std::fs::read(p) else {
            return;
        };
        let keep_from = data.len().saturating_sub(KEEP_BYTES as usize);
        let mut tail = &data[keep_from..];
        if let Some(i) = tail.iter().position(|&b| b == b'\n') {
            tail = &tail[i + 1..];
        }
        let mut body = b"--- log truncated ---\n".to_vec();
        body.extend_from_slice(tail);
        let _ = std::fs::write(p, body);
    });
}

fn write_to_file(msg: &str) {
    let Some(p) = resolved_log_path() else {
        return;
    };
    rotate_if_needed(&p);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {msg}", timestamp());
    }
}

/// Internal diagnostic line: written to the log file (if `MUSTARDY_LOG` is set)
/// and echoed to stderr only when verbose mode is active.
pub fn debug(msg: &str) {
    if is_verbose() {
        eprintln!("mustardy: [debug] {msg}");
    }
    write_to_file(msg);
}

/// Backwards-compatible log entry point used by core and the Tauri shell.
pub fn line(msg: &str) {
    debug(msg);
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let rem = secs % 86400;
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{:03}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        now.subsec_millis()
    )
}

/// Howard Hinnant's civil-from-days algorithm (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
