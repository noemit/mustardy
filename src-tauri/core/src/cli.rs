//! Headless CLI, used when the `mustardy` binary is invoked with arguments.
//!
//! Agents get a deterministic trim without a display:
//!
//! ```text
//! mustardy trim input.mp4                  # -> input-trimmed.mp4 (30 dB / 0.9 s)
//! mustardy trim input.mp4 -o out.mp4 --drop 24 --min 0.5 --normalize
//! mustardy trim input.mp4 --dry-run --json # report cuts, write nothing
//! mustardy install-cli                     # symlink onto PATH (macOS)
//! ```
//!
//! Human/progress lines go to stderr; machine output (`--json`) goes to stdout.

use std::path::{Path, PathBuf};

use crate::ffmpeg;
use crate::silence::{self, DEFAULT_SILENCE_DROP, DEFAULT_SILENCE_MIN};
use crate::visual;
use crate::{CoreResult, ExportPayload};

const USAGE: &str = "\
Mustardy — trim the pauses out of a video, offline.

USAGE:
    mustardy trim <input> [output] [options]
    mustardy install-cli
    mustardy --help
    mustardy --version

TRIM OPTIONS:
    -o, --output <path>     Output file (default: <input>-trimmed.mp4)
        --drop <dB>         How far below talking counts as a pause (8–35, default 30)
        --min <seconds>     Shortest pause to remove (default 0.9)
        --normalize         Apply podcast-style audio normalization (default)
        --no-normalize      Skip audio normalization
        --normalize-amount <0..1>  Normalization strength (default 0.7)
        --no-visual         Skip the visual-change scan (faster, less precise)
        --dry-run           Report the cuts but don't write a file
        --json              Print machine-readable JSON on stdout
    -h, --help              Show this help

EXAMPLES:
    mustardy trim talk.mp4
    mustardy trim talk.mp4 -o talk-cut.mp4 --drop 24 --min 0.5 --no-normalize
    mustardy trim talk.mp4 --dry-run --json
";

/// Entry point. Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest = if args.is_empty() { args } else { &args[1..] };
    let result = match cmd {
        "trim" | "cut" | "remove-silences" => run_trim(rest),
        "install-cli" => run_install_cli(),
        "help" | "--help" | "-h" | "" => {
            print!("{USAGE}");
            Ok(())
        }
        "--version" | "-V" | "version" => {
            println!("mustardy {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(crate::CoreError::msg(format!(
            "unknown command: {other}\n\n{USAGE}"
        ))),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("mustardy: {e}");
            1
        }
    }
}

#[derive(Debug)]
struct TrimArgs {
    input: String,
    output: Option<String>,
    drop_db: f64,
    min_dur: f64,
    normalize: bool,
    normalize_amount: f64,
    visual: bool,
    dry_run: bool,
    json: bool,
}

fn parse_trim(args: &[String]) -> Result<TrimArgs, String> {
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut drop_db = DEFAULT_SILENCE_DROP;
    let mut min_dur = DEFAULT_SILENCE_MIN;
    let mut normalize = true;
    let mut normalize_amount = 0.7;
    let mut visual = true;
    let mut dry_run = false;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            // Supports `--flag value` and `--flag=value`.
            if let Some((_, v)) = arg.split_once('=') {
                if !v.is_empty() {
                    return Ok(v.to_string());
                }
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.split('=').next().unwrap_or(arg) {
            "-o" | "--output" => output = Some(value("--output")?),
            "--drop" | "--silence-drop" => {
                let raw = value("--drop")?;
                let n: f64 = raw
                    .trim_end_matches(|c| c == 'b' || c == 'B' || c == 'd' || c == 'D')
                    .trim()
                    .parse()
                    .map_err(|_| format!("--drop expects a number in dB, got '{raw}'"))?;
                if !n.is_finite() {
                    return Err("--drop expects a finite number".into());
                }
                if n < 8.0 || n > 35.0 {
                    eprintln!("mustardy: --drop {n} out of range 8–35, clamping");
                }
                drop_db = n.clamp(8.0, 35.0);
            }
            "--min" | "--min-pause" => {
                let raw = value("--min")?;
                let n: f64 = raw
                    .trim_end_matches(|c| c == 's' || c == 'S')
                    .trim()
                    .parse()
                    .map_err(|_| format!("--min expects seconds, got '{raw}'"))?;
                if !n.is_finite() {
                    return Err("--min expects a finite number".into());
                }
                min_dur = n.max(0.02);
            }
            "--normalize" | "--normalise" => normalize = true,
            "--no-normalize" | "--no-normalise" => normalize = false,
            "--normalize-amount" | "--normalise-amount" => {
                let raw = value("--normalize-amount")?;
                normalize_amount = raw
                    .parse::<f64>()
                    .map_err(|_| format!("--normalize-amount expects 0..1, got '{raw}'"))?
                    .clamp(0.0, 1.0);
                normalize = true;
            }
            "--no-visual" => visual = false,
            "--dry-run" => dry_run = true,
            "--json" => json = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option: {other}\n\n{USAGE}"));
            }
            _ => {
                if input.is_none() {
                    input = Some(arg.to_string());
                } else if output.is_none() {
                    output = Some(arg.to_string());
                } else {
                    return Err(format!("unexpected argument: {arg}"));
                }
            }
        }
        i += 1;
    }

    let input = input.ok_or_else(|| format!("trim needs an input file\n\n{USAGE}"))?;
    Ok(TrimArgs {
        input,
        output,
        drop_db,
        min_dur,
        normalize,
        normalize_amount,
        visual,
        dry_run,
        json,
    })
}

fn default_output(input: &str) -> String {
    let p = Path::new(input);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    p.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}-trimmed.mp4"))
        .to_string_lossy()
        .into_owned()
}

fn run_trim(args: &[String]) -> CoreResult<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return Ok(());
    }
    let opts = parse_trim(args).map_err(crate::CoreError::msg)?;
    if opts.input == "-" {
        return Err(crate::CoreError::msg("input must be a file path"));
    }

    let meta = ffmpeg::probe(&opts.input)?;
    if meta.duration <= 0.0 {
        return Err(crate::CoreError::msg(format!(
            "could not read a duration from {}",
            opts.input
        )));
    }
    if !meta.has_audio {
        return Err(crate::CoreError::msg(format!(
            "{} has no audio track, so there are no pauses to find",
            opts.input
        )));
    }

    eprintln!(
        "mustardy: scanning {} (drop {} dB, min {}s)",
        meta.name, opts.drop_db, opts.min_dur
    );
    let env = ffmpeg::audio_envelope(&opts.input)?;
    let ranges = silence::silences_from_envelope(&env, opts.drop_db, opts.min_dur);

    let visuals: Vec<Vec<f64>> = if opts.visual && !ranges.is_empty() {
        eprintln!(
            "mustardy: checking {} pause(s) for visual changes",
            ranges.len()
        );
        visual::visual_change_times(&opts.input, &ranges).unwrap_or_default()
    } else {
        Vec::new()
    };
    let cuts = silence::silence_cuts(&ranges, &visuals, 0.18, 0.2);
    let removed: f64 = cuts.iter().map(|c| c.end - c.start).sum();
    let trimmed = (meta.duration - removed).max(0.0);

    let output = opts
        .output
        .clone()
        .unwrap_or_else(|| default_output(&opts.input));
    if !opts.dry_run && output == opts.input {
        return Err(crate::CoreError::msg(
            "output would overwrite the input — pass -o with a different path",
        ));
    }

    if opts.dry_run {
        if opts.json {
            print_json(&opts, &meta, &ranges, &cuts, removed, trimmed, None)?;
        } else {
            eprintln!(
                "mustardy: dry run — {} pause(s), {} cut(s), would remove {}",
                ranges.len(),
                cuts.len(),
                fmt_secs(removed)
            );
            for c in &cuts {
                eprintln!("  {} – {}  {}", fmt_secs(c.start), fmt_secs(c.end), c.label);
            }
        }
        return Ok(());
    }

    if cuts.is_empty() && !opts.normalize {
        return Err(crate::CoreError::msg(
            "no pauses found with these settings — nothing to trim",
        ));
    }

    let payload = ExportPayload {
        input: opts.input.clone(),
        duration: meta.duration,
        changes: cuts.clone(),
        output: output.clone(),
        normalize: opts.normalize,
        normalize_amount: opts.normalize_amount,
    };
    ffmpeg::export_project(&payload, |p| {
        eprintln!("mustardy: {}% — {}", p.progress, p.label);
    })?;

    if opts.json {
        print_json(
            &opts,
            &meta,
            &ranges,
            &cuts,
            removed,
            trimmed,
            Some(&output),
        )?;
    } else {
        eprintln!(
            "mustardy: done — {} → {} ({} → {}, removed {})",
            opts.input,
            output,
            fmt_secs(meta.duration),
            fmt_secs(trimmed),
            fmt_secs(removed)
        );
    }
    Ok(())
}

fn print_json(
    opts: &TrimArgs,
    meta: &crate::VideoMeta,
    ranges: &[crate::SilenceRange],
    cuts: &[crate::Change],
    removed: f64,
    trimmed: f64,
    output: Option<&str>,
) -> CoreResult<()> {
    let value = serde_json::json!({
        "input": opts.input,
        "output": output,
        "dryRun": opts.dry_run,
        "duration": meta.duration,
        "trimmedDuration": trimmed,
        "removed": removed,
        "dropDb": opts.drop_db,
        "minPause": opts.min_dur,
        "silences": ranges.len(),
        "cuts": cuts.iter().map(|c| serde_json::json!({
            "start": c.start,
            "end": c.end,
            "label": c.label,
        })).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn fmt_secs(seconds: f64) -> String {
    let s = if !seconds.is_finite() || seconds < 0.0 {
        0.0
    } else {
        seconds
    };
    let m = (s / 60.0).floor() as i64;
    format!("{m}:{:04.1}", s % 60.0)
}

#[cfg(target_os = "macos")]
fn run_install_cli() -> CoreResult<()> {
    install_symlink(Path::new("/usr/local/bin/mustardy"))
}

#[cfg(target_os = "linux")]
fn run_install_cli() -> CoreResult<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    install_symlink(&PathBuf::from(home).join(".local/bin/mustardy"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn run_install_cli() -> CoreResult<()> {
    Err(crate::CoreError::msg(
        "install-cli is only supported on macOS and Linux",
    ))
}

#[cfg(unix)]
fn install_symlink(dest: &Path) -> CoreResult<()> {
    let exe = std::env::current_exe()?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if dest.exists() || dest.symlink_metadata().is_ok() {
        let _ = std::fs::remove_file(dest);
    }
    match std::os::unix::fs::symlink(&exe, dest) {
        Ok(()) => {
            eprintln!("mustardy: linked {} → {}", dest.display(), exe.display());
            eprintln!("mustardy: run `mustardy --help` to confirm");
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(crate::CoreError::msg(format!(
                "couldn't write {} ({e}). Try:\n  sudo ln -sf '{}' '{}'",
                dest.display(),
                exe.display(),
                dest.display()
            )))
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_match_the_documented_settings() {
        let a = parse_trim(&args(&["talk.mp4"])).unwrap();
        assert_eq!(a.input, "talk.mp4");
        assert_eq!(a.output, None);
        assert_eq!(a.drop_db, 30.0);
        assert_eq!(a.min_dur, 0.9);
        assert!(a.normalize);
        assert!(a.visual);
    }

    #[test]
    fn accepts_flags_and_positional_output() {
        let a = parse_trim(&args(&[
            "in.mp4",
            "out.mp4",
            "--drop",
            "24",
            "--min=0.5",
            "--normalize",
            "--json",
        ]))
        .unwrap();
        assert_eq!(a.output.as_deref(), Some("out.mp4"));
        assert_eq!(a.drop_db, 24.0);
        assert_eq!(a.min_dur, 0.5);
        assert!(a.normalize);
        assert!(a.json);
    }

    #[test]
    fn clamps_drop_into_range() {
        assert_eq!(
            parse_trim(&args(&["in.mp4", "--drop", "99"]))
                .unwrap()
                .drop_db,
            35.0
        );
        assert_eq!(
            parse_trim(&args(&["in.mp4", "--drop", "0"]))
                .unwrap()
                .drop_db,
            8.0
        );
    }

    #[test]
    fn no_normalize_turns_it_off() {
        let a = parse_trim(&args(&["in.mp4", "--no-normalize"])).unwrap();
        assert!(!a.normalize);
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(parse_trim(&args(&["in.mp4", "--nope"])).is_err());
    }

    #[test]
    fn default_output_sits_next_to_input() {
        assert_eq!(default_output("/tmp/a/talk.mp4"), "/tmp/a/talk-trimmed.mp4");
        assert_eq!(default_output("talk.mov"), "talk-trimmed.mp4");
    }
}
