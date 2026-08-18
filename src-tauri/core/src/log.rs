//! Append-only log: every line goes to stderr (the dev terminal) and to a
//! log file. Default path is `<repo>/mustardy.log` — the repo is the one
//! place both the desktop app and headless smoke tests can reach. Override
//! with `MUSTARDY_LOG=/path/to/file.log`. The file is truncated to its last
//! 1 MB once it grows past 2 MB (checked once per process).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 2_000_000;
const KEEP_BYTES: u64 = 1_000_000;

fn path() -> Option<&'static PathBuf> {
    static P: OnceLock<Option<PathBuf>> = OnceLock::new();
    P.get_or_init(|| {
        if let Ok(v) = std::env::var("MUSTARDY_LOG") {
            if !v.is_empty() {
                return Some(PathBuf::from(v));
            }
        }
        // Dev/packaged-from-repo: <repo>/mustardy.log
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mustardy.log");
        if repo.parent().map(|p| p.is_dir()).unwrap_or(false) {
            return Some(repo);
        }
        // Packaged app moved elsewhere: fall back to the temp dir.
        Some(std::env::temp_dir().join("mustardy.log"))
    })
    .as_ref()
}

fn rotate_once() {
    static DID: OnceLock<()> = OnceLock::new();
    DID.get_or_init(|| {
        let Some(p) = path() else { return };
        let Ok(meta) = std::fs::metadata(p) else { return };
        if meta.len() <= MAX_BYTES {
            return;
        }
        let Ok(data) = std::fs::read(p) else { return };
        let keep_from = data.len() - KEEP_BYTES as usize;
        let mut tail = &data[keep_from..];
        if let Some(i) = tail.iter().position(|&b| b == b'\n') {
            tail = &tail[i + 1..]; // start at a whole line
        }
        let mut body = b"--- log truncated ---\n".to_vec();
        body.extend_from_slice(tail);
        let _ = std::fs::write(p, body);
    });
}

/// Log one line: `[mustardy] …` on stderr, timestamped line in the file.
pub fn line(msg: &str) {
    eprintln!("[mustardy] {msg}");
    rotate_once();
    let Some(p) = path() else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{} {msg}", timestamp());
    }
}

fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
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
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month index [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
