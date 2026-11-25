//! TTY-aware progress rendering (ANSI progress bar, plain lines, NDJSON/jsonl, or silent).

use std::io::{IsTerminal, Write};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    Auto,
    Bar,
    Plain,
    Jsonl,
    None,
}

pub struct ProgressReporter {
    mode: ProgressMode,
    start: Instant,
    last_pct: Option<u32>,
    bar_active: bool,
}

impl ProgressReporter {
    pub fn new(mode: ProgressMode, quiet: bool) -> Self {
        let resolved = if quiet || mode == ProgressMode::None {
            ProgressMode::None
        } else if mode == ProgressMode::Auto {
            if std::io::stderr().is_terminal() {
                ProgressMode::Bar
            } else {
                ProgressMode::Plain
            }
        } else {
            mode
        };
        Self {
            mode: resolved,
            start: Instant::now(),
            last_pct: None,
            bar_active: false,
        }
    }

    pub fn info(&mut self, stage: &str, msg: &str) {
        match self.mode {
            ProgressMode::None => {}
            ProgressMode::Jsonl => {
                let ev = serde_json::json!({
                    "event": "stage",
                    "stage": stage,
                    "message": msg,
                });
                eprintln!("{ev}");
            }
            ProgressMode::Bar => {
                self.clear_bar();
                eprintln!("mustardy: {msg}");
            }
            ProgressMode::Plain | ProgressMode::Auto => {
                eprintln!("mustardy: {msg}");
            }
        }
    }

    pub fn warn(&mut self, msg: &str) {
        if self.mode == ProgressMode::None {
            return;
        }
        if self.mode == ProgressMode::Jsonl {
            let ev = serde_json::json!({
                "event": "warning",
                "message": msg,
            });
            eprintln!("{ev}");
            return;
        }
        self.clear_bar();
        eprintln!("mustardy: warning — {msg}");
    }

    pub fn update(&mut self, stage: &str, pct: u32, label: &str) {
        let pct = pct.min(100);
        if self.last_pct == Some(pct) && pct < 100 {
            return;
        }
        self.last_pct = Some(pct);

        match self.mode {
            ProgressMode::None => {}
            ProgressMode::Jsonl => {
                let ev = serde_json::json!({
                    "event": "progress",
                    "stage": stage,
                    "percent": pct,
                    "label": label,
                    "elapsedMs": self.start.elapsed().as_millis() as u64,
                });
                eprintln!("{ev}");
            }
            ProgressMode::Bar => {
                let width = 24usize;
                let filled = ((pct as usize) * width) / 100;
                let bar = format!("{}{}", "█".repeat(filled), "░".repeat(width - filled));
                let elapsed = self.start.elapsed().as_secs_f64();
                let eta = if pct >= 5 && pct < 100 && elapsed > 1.0 {
                    let rem = elapsed * (100.0 - pct as f64) / (pct as f64);
                    format!(" ETA {}", fmt_dur(rem))
                } else {
                    String::new()
                };
                let mut err = std::io::stderr();
                let _ = write!(err, "\r\x1b[2Kmustardy: [{bar}] {pct:3}%{eta} — {label}");
                let _ = err.flush();
                self.bar_active = true;
                if pct >= 100 {
                    self.clear_bar();
                }
            }
            ProgressMode::Plain | ProgressMode::Auto => {
                eprintln!("mustardy: {pct}% — {label}");
            }
        }
    }

    pub fn clear_bar(&mut self) {
        if self.bar_active {
            let mut err = std::io::stderr();
            let _ = write!(err, "\r\x1b[2K");
            let _ = err.flush();
            self.bar_active = false;
        }
    }
}

impl Drop for ProgressReporter {
    fn drop(&mut self) {
        self.clear_bar();
    }
}

fn fmt_dur(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}
