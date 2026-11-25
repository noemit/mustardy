//! Mustardy headless CLI (`clap`-powered).
//!
//! Subcommands:
//! - `trim`        Remove pauses (single file or batch, video or audio-only)
//! - `scan`        Analyze silences & visual transitions and write a reusable plan JSON
//! - `apply`       Render a saved/edited plan JSON without re-scanning
//! - `doctor`      Inspect FFmpeg, hardware encoders, and configuration
//! - `completions` Generate shell completions (bash, zsh, fish, powershell)
//! - `install-cli` Symlink the binary onto PATH

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::config::{load_config, UserConfig};
use crate::ffmpeg;
use crate::log::Verbosity;
use crate::nle;
use crate::plan::{cuts_from_srt, parse_cut_range, CutPlan, PlanSettings};
use crate::progress::{ProgressMode, ProgressReporter};
use crate::silence::{
    self, SilenceCutOptions, DEFAULT_PAD_S, DEFAULT_SILENCE_DROP, DEFAULT_SILENCE_MIN,
    DEFAULT_TRANSITION_MARGIN_S, MAX_DROP, MIN_DROP,
};
use crate::visual::{self, DEFAULT_MAX_RANGES};
use crate::{round3, Change, CoreError, CoreResult, ExportOptions, ExportPayload, VideoCodec};

/// Exit codes:
/// - 0: success
/// - 1: runtime / FFmpeg / I/O error
/// - 2: CLI usage / invalid argument
/// - 3: unsupported input (e.g., no audio track)
/// - 130: interrupted (Ctrl-C)
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_UNSUPPORTED: i32 = 3;

#[derive(Debug, Parser)]
#[command(
    name = "mustardy",
    version,
    about = "Trim the pauses out of a video or audio file, offline.",
    long_about = "Mustardy trims silences from video and audio files offline using FFmpeg.\n\n\
        Detects pauses from the audio RMS envelope, checks each quiet stretch visually\n\
        so scene cuts and slide switches stay intact, and exports in a single fast pass."
)]
pub struct Cli {
    /// Suppress non-error progress output on stderr
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Emit internal debug logs (FFmpeg binary resolution, timings, filters)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TrimPreset {
    /// Snappy YouTube pacing: drop 24 dB, min 0.45s, tighter 100ms/80ms breath
    Tight,
    /// Balanced default pacing: drop 30 dB, min 0.9s, 180ms breath
    Natural,
    /// Podcast pacing: drop 28 dB, min 0.7s, 180ms/140ms breath, stronger normalization
    Podcast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Export trimmed video/audio media via FFmpeg (default)
    Media,
    /// Export a Final Cut Pro XML v1.10 timeline (.fcpxml, zero re-encode)
    Fcpxml,
    /// Export a CMX 3600 EDL timeline (.edl, zero re-encode)
    Edl,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ProgressArg {
    Auto,
    Bar,
    Plain,
    Jsonl,
    None,
}

impl From<ProgressArg> for ProgressMode {
    fn from(value: ProgressArg) -> Self {
        match value {
            ProgressArg::Auto => ProgressMode::Auto,
            ProgressArg::Bar => ProgressMode::Bar,
            ProgressArg::Plain => ProgressMode::Plain,
            ProgressArg::Jsonl => ProgressMode::Jsonl,
            ProgressArg::None => ProgressMode::None,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Trim silences from one or more video/audio files
    #[command(visible_aliases = ["cut", "remove-silences"])]
    Trim(TrimCmd),

    /// Scan a file and emit a reusable cut plan JSON (no encoding)
    #[command(visible_alias = "analyze")]
    Scan(ScanCmd),

    /// Apply a saved (or hand-edited) cut plan JSON directly without re-scanning
    Apply(ApplyCmd),

    /// Check FFmpeg/ffprobe installation, versions, hardware encoders, and config
    Doctor(DoctorCmd),

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },

    /// Symlink `mustardy` onto PATH (/usr/local/bin on macOS, ~/.local/bin on Linux)
    InstallCli,
}

#[derive(Debug, Args, Clone)]
pub struct DetectionArgs {
    /// Pacing preset (overridden by explicit flags)
    #[arg(long, value_enum)]
    pub preset: Option<TrimPreset>,

    /// How far below talking counts as a pause in dB (8–35, default 30)
    #[arg(long, alias = "silence-drop", value_parser = parse_db_arg)]
    pub drop: Option<f64>,

    /// Shortest pause to remove in seconds (default 0.9)
    #[arg(long, alias = "min-pause", value_parser = parse_secs_arg)]
    pub min: Option<f64>,

    /// Breathing room kept on both sides of a cut in seconds (default 0.18)
    #[arg(long, value_parser = parse_secs_arg)]
    pub pad: Option<f64>,

    /// Breathing room kept after speech ends (start of pause, seconds)
    #[arg(long, value_parser = parse_secs_arg)]
    pub pad_before: Option<f64>,

    /// Breathing room kept before speech resumes (end of pause, seconds)
    #[arg(long, value_parser = parse_secs_arg)]
    pub pad_after: Option<f64>,

    /// Protected margin around mid-pause visual changes in seconds (default 0.2)
    #[arg(long, value_parser = parse_secs_arg)]
    pub transition_margin: Option<f64>,

    /// Keep padding on leading/trailing silence at 0:00 and EOF instead of trimming cleanly
    #[arg(long)]
    pub keep_edges: bool,

    /// Skip the visual-change scan (faster, less precise on slide/screen recordings)
    #[arg(long)]
    pub no_visual: bool,

    /// Maximum pauses to visually inspect (0 = unlimited, default 1000)
    #[arg(long)]
    pub max_visual_ranges: Option<usize>,

    /// Extra manual cut range (repeatable), e.g. --cut 0:00-0:12 --cut 14:20-end
    #[arg(long = "cut", value_name = "START-END")]
    pub manual_cuts: Vec<String>,

    /// Optional SRT subtitle file for deterministic word/phrase cutting
    #[arg(long, value_name = "FILE")]
    pub srt: Option<PathBuf>,

    /// Comma-separated words or phrases to cut from --srt (e.g. "um,uh,cut that")
    #[arg(long, value_delimiter = ',')]
    pub cut_words: Vec<String>,
}

#[derive(Debug, Args, Clone)]
pub struct EncodeArgs {
    /// Apply podcast-style audio normalization (default: on)
    #[arg(long, alias = "normalise", conflicts_with = "no_normalize")]
    pub normalize: bool,

    /// Skip audio normalization
    #[arg(long, alias = "no-normalise")]
    pub no_normalize: bool,

    /// Normalization strength 0..1 (default 0.7)
    #[arg(long, alias = "normalise-amount", value_parser = parse_unit_arg)]
    pub normalize_amount: Option<f64>,

    /// Use hardware H.264 encoder when available (VideoToolbox / NVENC / VAAPI / QSV)
    #[arg(long)]
    pub hw: bool,

    /// Explicit FFmpeg video encoder (e.g. libx264, h264_nvenc, h264_videotoolbox)
    #[arg(long)]
    pub codec: Option<String>,

    /// Video quality CRF/CQ/QP (0–51, lower = higher quality, default 18)
    #[arg(long)]
    pub crf: Option<u8>,

    /// Software encoder preset (ultrafast, veryfast, fast, medium; default veryfast)
    #[arg(long)]
    pub encode_preset: Option<String>,

    /// Audio micro-fade at cut joins in ms to prevent pops (default 8, 0 to disable)
    #[arg(long)]
    pub crossfade: Option<f64>,

    /// Keyframe-snapped stream-copy mode (very fast drafts, less frame-accurate)
    #[arg(long)]
    pub fast_copy: bool,

    /// Parallel worker threads for visual scan and chunked export (0 = auto)
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,
}

#[derive(Debug, Args)]
pub struct TrimCmd {
    /// Input media file(s). Passing two paths where the second does not exist
    /// treats the second as the output path for backwards compatibility.
    #[arg(required = true)]
    pub inputs: Vec<String>,

    /// Output file path (single-file mode; default: <input>-trimmed.<ext>)
    #[arg(short, long)]
    pub output: Option<String>,

    /// Write trimmed outputs into this directory (ideal for batch globs)
    #[arg(long, value_name = "DIR")]
    pub out_dir: Option<PathBuf>,

    /// Atomically replace each input file in place
    #[arg(long, conflicts_with_all = ["output", "out_dir"])]
    pub in_place: bool,

    /// When using --in-place, keep a <input>.bak copy of the original
    #[arg(long, requires = "in_place")]
    pub backup: bool,

    /// Skip inputs whose output file already exists
    #[arg(long)]
    pub skip_existing: bool,

    /// Reuse a previously saved plan JSON instead of re-scanning
    #[arg(long, value_name = "PLAN.json")]
    pub plan: Option<PathBuf>,

    /// Save the computed cut plan to this JSON file
    #[arg(long, value_name = "PLAN.json")]
    pub save_plan: Option<PathBuf>,

    /// Output format: media (default), fcpxml (Final Cut / Resolve / Premiere), or edl (CMX 3600).
    /// Also auto-detected when -o ends with .fcpxml or .edl.
    #[arg(long, value_enum)]
    pub format: Option<OutputFormat>,

    /// Report detected cuts without writing any media file
    #[arg(long)]
    pub dry_run: bool,

    /// Print machine-readable JSON on stdout (progress stays on stderr)
    #[arg(long)]
    pub json: bool,

    /// Progress output style on stderr
    #[arg(long, value_enum, default_value_t = ProgressArg::Auto)]
    pub progress: ProgressArg,

    #[command(flatten)]
    pub detect: DetectionArgs,

    #[command(flatten)]
    pub encode: EncodeArgs,
}

#[derive(Debug, Args)]
pub struct ScanCmd {
    /// Input video or audio file to scan
    pub input: String,

    /// Write plan JSON to this file (prints to stdout if omitted with --json)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Print machine-readable plan JSON to stdout
    #[arg(long)]
    pub json: bool,

    /// Progress output style on stderr
    #[arg(long, value_enum, default_value_t = ProgressArg::Auto)]
    pub progress: ProgressArg,

    /// Parallel worker threads for visual scan (0 = auto)
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,

    #[command(flatten)]
    pub detect: DetectionArgs,
}

#[derive(Debug, Args)]
pub struct ApplyCmd {
    /// Path to a plan JSON produced by `mustardy scan` (or hand-edited)
    pub plan: PathBuf,

    /// Override input media path (defaults to the `input` field in the plan)
    #[arg(short, long)]
    pub input: Option<String>,

    /// Output media or timeline path (defaults to plan `output` or <input>-trimmed.<ext>)
    #[arg(short, long)]
    pub output: Option<String>,

    /// Output format: media (default), fcpxml, or edl (auto-detected from -o extension)
    #[arg(long, value_enum)]
    pub format: Option<OutputFormat>,

    /// Print machine-readable JSON result on stdout
    #[arg(long)]
    pub json: bool,

    /// Progress output style on stderr
    #[arg(long, value_enum, default_value_t = ProgressArg::Auto)]
    pub progress: ProgressArg,

    #[command(flatten)]
    pub encode: EncodeArgs,
}

#[derive(Debug, Args)]
pub struct DoctorCmd {
    /// Print diagnostics as JSON on stdout
    #[arg(long)]
    pub json: bool,
}

// ---------- custom value parsers ----------

fn parse_db_arg(raw: &str) -> Result<f64, String> {
    let n: f64 = raw
        .trim()
        .trim_end_matches(|c| c == 'b' || c == 'B' || c == 'd' || c == 'D')
        .trim()
        .parse()
        .map_err(|_| format!("expects a number in dB (8–35), got '{raw}'"))?;
    if !n.is_finite() {
        return Err("expects a finite number".into());
    }
    Ok(n)
}

fn parse_secs_arg(raw: &str) -> Result<f64, String> {
    let n: f64 = raw
        .trim()
        .trim_end_matches(|c| c == 's' || c == 'S')
        .trim()
        .parse()
        .map_err(|_| format!("expects seconds, got '{raw}'"))?;
    if !n.is_finite() || n < 0.0 {
        return Err("expects a finite non-negative number".into());
    }
    Ok(n)
}

fn parse_unit_arg(raw: &str) -> Result<f64, String> {
    let n: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("expects a number in 0..1, got '{raw}'"))?;
    if !n.is_finite() {
        return Err("expects a finite number in 0..1".into());
    }
    Ok(n.clamp(0.0, 1.0))
}

// ---------- entry point ----------

fn install_ctrlc_handler() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = ctrlc::set_handler(|| {
            ffmpeg::cleanup_registered_temp_dirs();
            eprintln!("\nmustardy: interrupted — cleaned up temporary files");
            std::process::exit(130);
        });
    });
}

/// Entry point called by both `mustardy` and `mustardy-cli`.
pub fn run(args: &[String]) -> i32 {
    install_ctrlc_handler();

    let argv: Vec<String> = std::iter::once("mustardy".to_string())
        .chain(args.iter().cloned())
        .collect();

    let json_requested = args.iter().any(|a| a == "--json");

    let cli = match Cli::try_parse_from(&argv) {
        Ok(c) => c,
        Err(e) => {
            if e.use_stderr() {
                if json_requested {
                    let payload = serde_json::json!({
                        "ok": false,
                        "code": "INVALID_ARGS",
                        "error": e.to_string().trim(),
                    });
                    println!("{payload}");
                } else {
                    let _ = e.print();
                }
                return EXIT_USAGE;
            } else {
                let _ = e.print();
                return EXIT_OK;
            }
        }
    };

    if cli.quiet {
        crate::log::set_verbosity(Verbosity::Quiet);
    } else if cli.verbose {
        crate::log::set_verbosity(Verbosity::Verbose);
    } else {
        crate::log::set_verbosity(Verbosity::Normal);
    }

    let (cfg, cfg_path) = load_config();

    let result = match cli.command {
        Command::Trim(cmd) => run_trim(cmd, &cfg, cli.quiet),
        Command::Scan(cmd) => run_scan(cmd, &cfg, cli.quiet),
        Command::Apply(cmd) => run_apply(cmd, &cfg, cli.quiet),
        Command::Doctor(cmd) => run_doctor(cmd, cfg_path),
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "mustardy", &mut std::io::stdout());
            Ok(())
        }
        Command::InstallCli => run_install_cli(),
    };

    match result {
        Ok(()) => EXIT_OK,
        Err(e) => {
            let code = e.code();
            let exit = match code {
                "INVALID_ARGS" | "INVALID_PLAN" => EXIT_USAGE,
                "NO_AUDIO" => EXIT_UNSUPPORTED,
                _ => EXIT_ERR,
            };
            if json_requested {
                let payload = serde_json::json!({
                    "ok": false,
                    "code": code,
                    "error": e.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
            } else {
                eprintln!("mustardy: {e}");
            }
            exit
        }
    }
}

// ---------- settings resolution ----------

struct ResolvedSettings {
    drop_db: f64,
    min_dur: f64,
    pad_before: f64,
    pad_after: f64,
    transition_margin: f64,
    trim_edges: bool,
    visual: bool,
    max_visual_ranges: usize,
    normalize: bool,
    normalize_amount: f64,
    export: ExportOptions,
}

fn preset_from_name(name: &str) -> Option<TrimPreset> {
    match name.trim().to_ascii_lowercase().as_str() {
        "tight" => Some(TrimPreset::Tight),
        "natural" => Some(TrimPreset::Natural),
        "podcast" => Some(TrimPreset::Podcast),
        _ => None,
    }
}

fn resolve_settings(
    detect: &DetectionArgs,
    encode: Option<&EncodeArgs>,
    cfg: &UserConfig,
    jobs_override: Option<usize>,
) -> ResolvedSettings {
    let preset = detect
        .preset
        .or_else(|| cfg.preset.as_deref().and_then(preset_from_name));

    let (mut drop_db, mut min_dur, mut pad_before, mut pad_after, mut norm_default, mut norm_amt) =
        match preset {
            Some(TrimPreset::Tight) => (24.0, 0.45, 0.10, 0.08, true, 0.7),
            Some(TrimPreset::Podcast) => (28.0, 0.70, 0.18, 0.14, true, 0.8),
            Some(TrimPreset::Natural) | None => (
                DEFAULT_SILENCE_DROP,
                DEFAULT_SILENCE_MIN,
                DEFAULT_PAD_S,
                DEFAULT_PAD_S,
                true,
                0.7,
            ),
        };

    // Apply config overrides if preset wasn't explicitly set on CLI.
    if detect.preset.is_none() {
        if let Some(v) = cfg.drop {
            drop_db = v;
        }
        if let Some(v) = cfg.min {
            min_dur = v;
        }
        if let Some(v) = cfg.pad {
            pad_before = v;
            pad_after = v;
        }
        if let Some(v) = cfg.pad_before {
            pad_before = v;
        }
        if let Some(v) = cfg.pad_after {
            pad_after = v;
        }
    }
    if let Some(v) = cfg.normalize {
        norm_default = v;
    }
    if let Some(v) = cfg.normalize_amount {
        norm_amt = v.clamp(0.0, 1.0);
    }

    // Apply explicit CLI flags.
    if let Some(raw) = detect.drop {
        if !(MIN_DROP..=MAX_DROP).contains(&raw) {
            eprintln!("mustardy: --drop {raw} out of range {MIN_DROP}–{MAX_DROP}, clamping");
        }
        drop_db = raw.clamp(MIN_DROP, MAX_DROP);
    } else {
        drop_db = drop_db.clamp(MIN_DROP, MAX_DROP);
    }

    if let Some(raw) = detect.min {
        min_dur = raw.max(0.02);
    } else {
        min_dur = min_dur.max(0.02);
    }

    if let Some(p) = detect.pad {
        pad_before = p;
        pad_after = p;
    }
    if let Some(p) = detect.pad_before {
        pad_before = p;
    }
    if let Some(p) = detect.pad_after {
        pad_after = p;
    }

    let transition_margin = detect
        .transition_margin
        .or(cfg.transition_margin)
        .unwrap_or(DEFAULT_TRANSITION_MARGIN_S)
        .max(0.0);

    let trim_edges = if detect.keep_edges {
        false
    } else {
        cfg.trim_edges.unwrap_or(true)
    };

    let visual = if detect.no_visual {
        false
    } else {
        cfg.visual.unwrap_or(true)
    };

    let max_visual_ranges = detect
        .max_visual_ranges
        .or(cfg.max_visual_ranges)
        .unwrap_or(DEFAULT_MAX_RANGES);

    let mut normalize = norm_default;
    let mut normalize_amount = norm_amt;
    let mut export = ExportOptions::default();

    if let Some(c) = &cfg.codec {
        export.codec = VideoCodec::Custom(c.clone());
    } else if cfg.hw == Some(true) {
        export.codec = VideoCodec::Hardware;
    }
    if let Some(crf) = cfg.crf {
        export.crf = crf;
    }
    if let Some(p) = &cfg.encode_preset {
        export.preset = p.clone();
    }
    if let Some(cf) = cfg.crossfade_ms {
        export.crossfade_ms = cf.max(0.0);
    }
    if let Some(j) = cfg.jobs {
        export.jobs = j;
    }

    if let Some(enc) = encode {
        if enc.no_normalize {
            normalize = false;
        } else if enc.normalize {
            normalize = true;
        }
        if let Some(amt) = enc.normalize_amount {
            normalize_amount = amt;
            if !enc.no_normalize {
                normalize = true;
            }
        }
        if let Some(codec) = &enc.codec {
            export.codec = VideoCodec::Custom(codec.clone());
        } else if enc.hw {
            export.codec = VideoCodec::Hardware;
        }
        if let Some(crf) = enc.crf {
            export.crf = crf;
        }
        if let Some(p) = &enc.encode_preset {
            export.preset = p.clone();
        }
        if let Some(cf) = enc.crossfade {
            export.crossfade_ms = cf.max(0.0);
        }
        export.fast_copy = enc.fast_copy;
        if let Some(j) = enc.jobs {
            export.jobs = j;
        }
    }

    if let Some(j) = jobs_override {
        export.jobs = j;
    }

    ResolvedSettings {
        drop_db,
        min_dur,
        pad_before,
        pad_after,
        transition_margin,
        trim_edges,
        visual,
        max_visual_ranges,
        normalize,
        normalize_amount,
        export,
    }
}

impl ResolvedSettings {
    fn to_plan_settings(&self) -> PlanSettings {
        PlanSettings {
            drop_db: self.drop_db,
            min_pause: self.min_dur,
            pad_before: self.pad_before,
            pad_after: self.pad_after,
            transition_margin: self.transition_margin,
            trim_edges: self.trim_edges,
            visual: self.visual,
            normalize: self.normalize,
            normalize_amount: self.normalize_amount,
        }
    }
}

// ---------- helpers ----------

fn resolve_format(explicit: Option<OutputFormat>, output: Option<&str>) -> OutputFormat {
    if let Some(fmt) = explicit {
        return fmt;
    }
    if let Some(out) = output {
        match Path::new(out)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("fcpxml") => return OutputFormat::Fcpxml,
            Some("edl") => return OutputFormat::Edl,
            _ => {}
        }
    }
    OutputFormat::Media
}

pub fn default_output(input: &str, has_video: bool) -> String {
    default_output_for_format(input, has_video, OutputFormat::Media)
}

fn default_output_for_format(input: &str, has_video: bool, fmt: OutputFormat) -> String {
    let p = Path::new(input);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let ext = match fmt {
        OutputFormat::Fcpxml => "fcpxml",
        OutputFormat::Edl => "edl",
        OutputFormat::Media => {
            if has_video {
                "mp4"
            } else {
                p.extension()
                    .and_then(|e| e.to_str())
                    .filter(|e| !e.is_empty())
                    .unwrap_or("m4a")
            }
        }
    };
    p.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}-trimmed.{ext}"))
        .to_string_lossy()
        .into_owned()
}

fn output_in_dir(input: &str, dir: &Path, has_video: bool, fmt: OutputFormat) -> String {
    let p = Path::new(input);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let ext = match fmt {
        OutputFormat::Fcpxml => "fcpxml",
        OutputFormat::Edl => "edl",
        OutputFormat::Media => {
            if has_video {
                "mp4"
            } else {
                p.extension()
                    .and_then(|e| e.to_str())
                    .filter(|e| !e.is_empty())
                    .unwrap_or("m4a")
            }
        }
    };
    dir.join(format!("{stem}-trimmed.{ext}"))
        .to_string_lossy()
        .into_owned()
}

fn same_path(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (
        std::fs::canonicalize(Path::new(a)),
        std::fs::canonicalize(Path::new(b)),
    ) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
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

/// Analyze one media file into a `CutPlan` and raw `Change` list.
fn analyze_input(
    input: &str,
    detect: &DetectionArgs,
    settings: &ResolvedSettings,
    dry_run: bool,
    progress: &mut ProgressReporter,
) -> CoreResult<(CutPlan, Vec<Change>, crate::VideoMeta)> {
    if input == "-" {
        return Err(CoreError::coded("INVALID_ARGS", "input must be a file path"));
    }
    let meta = ffmpeg::probe(input)?;
    if meta.duration <= 0.0 {
        return Err(CoreError::coded(
            "INVALID_MEDIA",
            format!("could not read a duration from {input}"),
        ));
    }
    if !meta.has_audio {
        return Err(CoreError::coded(
            "NO_AUDIO",
            format!("{input} has no audio track, so there are no pauses to find"),
        ));
    }

    progress.info(
        "scan",
        &format!(
            "scanning {} (drop {} dB, min {}s)",
            meta.name, settings.drop_db, settings.min_dur
        ),
    );
    let env = ffmpeg::audio_envelope(input)?;
    let analysis = silence::analyze_envelope(&env, settings.drop_db, settings.min_dur);
    if analysis.stats.low_contrast {
        progress.warn(&format!(
            "low audio contrast ({:.1} dB between speech {:.1} dBFS and noise {:.1} dBFS) — background music or continuous noise may limit pause detection; try a lower --drop (e.g. --drop 16)",
            analysis.stats.contrast_db, analysis.stats.speech_db, analysis.stats.noise_floor_db
        ));
    }

    let ranges = analysis.ranges;
    let visuals: Vec<Vec<f64>> = if settings.visual && meta.has_video && !ranges.is_empty() {
        progress.info(
            "visual",
            &format!("checking {} pause(s) for visual changes", ranges.len()),
        );
        let vres = visual::visual_change_times_parallel(
            input,
            &ranges,
            settings.export.jobs,
            settings.max_visual_ranges,
            |_, _| {},
        )?;
        if vres.truncated > 0 {
            progress.warn(&format!(
                "skipped visual check on {} pause(s) past --max-visual-ranges ({})",
                vres.truncated, settings.max_visual_ranges
            ));
        }
        vres.times
    } else {
        Vec::new()
    };

    let mut cuts = silence::silence_cuts_with_options(
        &ranges,
        &visuals,
        &SilenceCutOptions {
            pad_before: settings.pad_before,
            pad_after: settings.pad_after,
            transition_margin: settings.transition_margin,
            trim_edges: settings.trim_edges,
            duration: Some(meta.duration),
        },
    );

    // Append manual --cut ranges.
    for spec in &detect.manual_cuts {
        cuts.push(parse_cut_range(spec, meta.duration)?);
    }

    // Append optional SRT word/phrase cuts.
    if let Some(srt_path) = &detect.srt {
        if !detect.cut_words.is_empty() {
            let srt_cuts = cuts_from_srt(srt_path, &detect.cut_words, meta.duration)?;
            progress.info(
                "srt",
                &format!(
                    "matched {} cue(s) from {}",
                    srt_cuts.len(),
                    srt_path.display()
                ),
            );
            cuts.extend(srt_cuts);
        }
    }

    cuts.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let plan = CutPlan::from_analysis(
        input.to_string(),
        None,
        dry_run,
        &meta,
        Some(analysis.stats),
        settings.to_plan_settings(),
        ranges.len(),
        &cuts,
    );
    Ok((plan, cuts, meta))
}

// ---------- subcommand handlers ----------

fn run_trim(mut cmd: TrimCmd, cfg: &UserConfig, quiet: bool) -> CoreResult<()> {
    let settings = resolve_settings(&cmd.detect, Some(&cmd.encode), cfg, None);

    // Preserve legacy `mustardy trim in.mp4 out.mp4` when 2 args are passed and
    // the second doesn't exist yet as an input file.
    if cmd.inputs.len() == 2
        && cmd.output.is_none()
        && cmd.out_dir.is_none()
        && !cmd.in_place
        && !Path::new(&cmd.inputs[1]).is_file()
    {
        cmd.output = Some(cmd.inputs.pop().unwrap());
    }

    if cmd.inputs.len() > 1 && cmd.output.is_some() {
        return Err(CoreError::coded(
            "INVALID_ARGS",
            "cannot use -o/--output with multiple input files — use --out-dir or --in-place",
        ));
    }
    if cmd.inputs.len() > 1 && cmd.plan.is_some() {
        return Err(CoreError::coded(
            "INVALID_ARGS",
            "cannot use --plan with multiple input files",
        ));
    }

    let is_batch = cmd.inputs.len() > 1;
    let mut batch_results: Vec<serde_json::Value> = Vec::new();

    for (idx, input) in cmd.inputs.iter().enumerate() {
        let t0 = Instant::now();
        let mut progress = ProgressReporter::new(cmd.progress.into(), quiet);
        if is_batch {
            progress.info(
                "batch",
                &format!("[{}/{}] {}", idx + 1, cmd.inputs.len(), input),
            );
        }

        let (mut plan, cuts, meta) = if let Some(plan_path) = &cmd.plan {
            let loaded = CutPlan::load(plan_path)?;
            let meta = ffmpeg::probe(input)?;
            let cuts = loaded.to_changes();
            let rebuilt = CutPlan::from_analysis(
                input.clone(),
                None,
                cmd.dry_run,
                &meta,
                loaded.audio,
                settings.to_plan_settings(),
                loaded.silences,
                &cuts,
            );
            (rebuilt, cuts, meta)
        } else {
            analyze_input(input, &cmd.detect, &settings, cmd.dry_run, &mut progress)?
        };

        if let Some(save_path) = &cmd.save_plan {
            plan.save(save_path)?;
            progress.info("plan", &format!("saved plan to {}", save_path.display()));
        }

        let fmt = resolve_format(cmd.format, cmd.output.as_deref());
        if cmd.in_place && fmt != OutputFormat::Media {
            return Err(CoreError::coded(
                "INVALID_ARGS",
                "--in-place cannot be used with timeline formats (fcpxml/edl)",
            ));
        }

        let target_output = if cmd.in_place {
            input.clone()
        } else if let Some(out) = &cmd.output {
            out.clone()
        } else if let Some(dir) = &cmd.out_dir {
            std::fs::create_dir_all(dir)?;
            output_in_dir(input, dir, meta.has_video, fmt)
        } else {
            default_output_for_format(input, meta.has_video, fmt)
        };

        if !cmd.dry_run && !cmd.in_place && same_path(&target_output, input) {
            return Err(CoreError::coded(
                "INVALID_ARGS",
                "output would overwrite the input — pass -o with a different path or use --in-place",
            ));
        }

        if cmd.skip_existing && !cmd.in_place && !cmd.dry_run && Path::new(&target_output).is_file()
        {
            progress.info("skip", &format!("skipping existing {target_output}"));
            if cmd.json {
                batch_results.push(serde_json::json!({
                    "ok": true,
                    "input": input,
                    "output": target_output,
                    "skipped": true,
                }));
            }
            continue;
        }

        if cmd.dry_run {
            if cmd.json {
                let mut val = serde_json::to_value(&plan)?;
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("ok".into(), serde_json::Value::Bool(true));
                    obj.insert(
                        "elapsedMs".into(),
                        serde_json::Value::from(t0.elapsed().as_millis() as u64),
                    );
                }
                if is_batch {
                    batch_results.push(val);
                } else {
                    println!("{}", serde_json::to_string_pretty(&val)?);
                }
            } else {
                progress.info(
                    "dry-run",
                    &format!(
                        "dry run — {} pause(s), {} cut(s), would remove {} ({} → {})",
                        plan.silences,
                        plan.cuts.len(),
                        fmt_secs(plan.removed),
                        fmt_secs(plan.duration),
                        fmt_secs(plan.trimmed_duration),
                    ),
                );
                if !quiet {
                    if let Some(a) = &plan.audio {
                        eprintln!(
                            "  audio: speech {:.1} dBFS, noise {:.1} dBFS, threshold {:.1} dBFS",
                            a.speech_db, a.noise_floor_db, a.threshold_db
                        );
                    }
                    for c in &plan.cuts {
                        eprintln!("  {} – {}  {}", fmt_secs(c.start), fmt_secs(c.end), c.label);
                    }
                }
            }
            continue;
        }

        if cuts.is_empty() && !settings.normalize {
            progress.info(
                "unchanged",
                &format!("no pauses found in {} — nothing to trim", meta.name),
            );
            if cmd.json {
                let mut val = serde_json::to_value(&plan)?;
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("ok".into(), serde_json::Value::Bool(true));
                    obj.insert("unchanged".into(), serde_json::Value::Bool(true));
                    obj.insert(
                        "elapsedMs".into(),
                        serde_json::Value::from(t0.elapsed().as_millis() as u64),
                    );
                }
                if is_batch {
                    batch_results.push(val);
                } else {
                    println!("{}", serde_json::to_string_pretty(&val)?);
                }
            }
            continue;
        }

        if fmt != OutputFormat::Media {
            let keeps = if cuts.is_empty() {
                vec![ffmpeg::Seg {
                    start: 0.0,
                    end: meta.duration,
                }]
            } else {
                ffmpeg::kept_segments(meta.duration, &cuts)
            };
            match fmt {
                OutputFormat::Fcpxml => {
                    nle::write_fcpxml(input, &meta, &keeps, Path::new(&target_output))?;
                }
                OutputFormat::Edl => {
                    nle::write_edl(input, &meta, &keeps, Path::new(&target_output))?;
                }
                OutputFormat::Media => unreachable!(),
            }
        } else {
            // For --in-place, write to a sibling staged file and atomically replace.
            let write_target = if cmd.in_place {
                let p = Path::new(input);
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
                p.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(format!(
                        ".mustardy-inplace-{}-{}.{ext}",
                        std::process::id(),
                        idx
                    ))
                    .to_string_lossy()
                    .into_owned()
            } else {
                target_output.clone()
            };

            let payload = ExportPayload {
                input: input.clone(),
                duration: meta.duration,
                changes: cuts,
                output: write_target.clone(),
                normalize: settings.normalize,
                normalize_amount: settings.normalize_amount,
            };
            ffmpeg::export_project_with_options(&payload, &settings.export, |p| {
                progress.update("export", p.progress, &p.label);
            })?;

            if cmd.in_place {
                if cmd.backup {
                    let bak = format!("{input}.bak");
                    std::fs::copy(input, &bak)?;
                }
                std::fs::rename(&write_target, input)?;
            }
        }

        plan.output = Some(target_output.clone());
        let elapsed_ms = t0.elapsed().as_millis() as u64;

        if cmd.json {
            let mut val = serde_json::to_value(&plan)?;
            if let Some(obj) = val.as_object_mut() {
                obj.insert("ok".into(), serde_json::Value::Bool(true));
                obj.insert("elapsedMs".into(), serde_json::Value::from(elapsed_ms));
            }
            if is_batch {
                batch_results.push(val);
            } else {
                println!("{}", serde_json::to_string_pretty(&val)?);
            }
        } else {
            progress.info(
                "done",
                &format!(
                    "done in {:.1}s — {} → {} ({} → {}, removed {})",
                    elapsed_ms as f64 / 1000.0,
                    input,
                    target_output,
                    fmt_secs(plan.duration),
                    fmt_secs(plan.trimmed_duration),
                    fmt_secs(plan.removed)
                ),
            );
        }
    }

    if is_batch && cmd.json {
        let out = serde_json::json!({
            "ok": true,
            "count": batch_results.len(),
            "results": batch_results,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    }

    Ok(())
}

fn run_scan(cmd: ScanCmd, cfg: &UserConfig, quiet: bool) -> CoreResult<()> {
    let settings = resolve_settings(&cmd.detect, None, cfg, cmd.jobs);
    let mut progress = ProgressReporter::new(cmd.progress.into(), quiet);
    let (plan, _, _) = analyze_input(&cmd.input, &cmd.detect, &settings, true, &mut progress)?;

    if let Some(out_path) = &cmd.output {
        plan.save(out_path)?;
        progress.info(
            "saved",
            &format!(
                "wrote plan ({} cut(s), removes {}) → {}",
                plan.cuts.len(),
                fmt_secs(plan.removed),
                out_path.display()
            ),
        );
    }

    if cmd.json || cmd.output.is_none() {
        if cmd.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            progress.info(
                "summary",
                &format!(
                    "{} pause(s), {} cut(s), would remove {} ({} → {})",
                    plan.silences,
                    plan.cuts.len(),
                    fmt_secs(plan.removed),
                    fmt_secs(plan.duration),
                    fmt_secs(plan.trimmed_duration),
                ),
            );
            if !quiet {
                if let Some(a) = &plan.audio {
                    eprintln!(
                        "  audio: speech {:.1} dBFS, noise {:.1} dBFS, threshold {:.1} dBFS",
                        a.speech_db, a.noise_floor_db, a.threshold_db
                    );
                }
                for c in &plan.cuts {
                    eprintln!("  {} – {}  {}", fmt_secs(c.start), fmt_secs(c.end), c.label);
                }
            }
        }
    }
    Ok(())
}

fn run_apply(cmd: ApplyCmd, cfg: &UserConfig, quiet: bool) -> CoreResult<()> {
    let t0 = Instant::now();
    let mut plan = CutPlan::load(&cmd.plan)?;
    let input = cmd.input.unwrap_or_else(|| plan.input.clone());
    let meta = ffmpeg::probe(&input)?;

    let dummy_detect = DetectionArgs {
        preset: None,
        drop: None,
        min: None,
        pad: None,
        pad_before: None,
        pad_after: None,
        transition_margin: None,
        keep_edges: false,
        no_visual: true,
        max_visual_ranges: None,
        manual_cuts: Vec::new(),
        srt: None,
        cut_words: Vec::new(),
    };
    let mut settings = resolve_settings(&dummy_detect, Some(&cmd.encode), cfg, None);
    // Respect plan's normalize settings unless overridden on CLI.
    if !cmd.encode.normalize && !cmd.encode.no_normalize {
        if let Some(ps) = &plan.settings {
            settings.normalize = ps.normalize;
            if cmd.encode.normalize_amount.is_none() {
                settings.normalize_amount = ps.normalize_amount;
            }
        }
    }

    let fmt = resolve_format(cmd.format, cmd.output.as_deref());
    let output = cmd
        .output
        .or_else(|| plan.output.clone())
        .unwrap_or_else(|| default_output_for_format(&input, meta.has_video, fmt));

    if same_path(&output, &input) {
        return Err(CoreError::coded(
            "INVALID_ARGS",
            "output would overwrite the input — pass -o with a different path",
        ));
    }

    let cuts = plan.to_changes();
    let keeps = if cuts.is_empty() {
        vec![ffmpeg::Seg {
            start: 0.0,
            end: meta.duration,
        }]
    } else {
        ffmpeg::kept_segments(meta.duration, &cuts)
    };
    let trimmed: f64 = keeps.iter().map(|s| s.end - s.start).sum();
    let removed = (meta.duration - trimmed).max(0.0);

    let mut progress = ProgressReporter::new(cmd.progress.into(), quiet);
    progress.info(
        "apply",
        &format!(
            "applying {} cut(s) from {} to {}",
            cuts.len(),
            cmd.plan.display(),
            input
        ),
    );

    match fmt {
        OutputFormat::Fcpxml => {
            nle::write_fcpxml(&input, &meta, &keeps, Path::new(&output))?;
        }
        OutputFormat::Edl => {
            nle::write_edl(&input, &meta, &keeps, Path::new(&output))?;
        }
        OutputFormat::Media => {
            let payload = ExportPayload {
                input: input.clone(),
                duration: meta.duration,
                changes: cuts,
                output: output.clone(),
                normalize: settings.normalize,
                normalize_amount: settings.normalize_amount,
            };
            ffmpeg::export_project_with_options(&payload, &settings.export, |p| {
                progress.update("export", p.progress, &p.label);
            })?;
        }
    }

    plan.input = input.clone();
    plan.output = Some(output.clone());
    plan.dry_run = false;
    plan.trimmed_duration = round3(trimmed);
    plan.removed = round3(removed);
    let elapsed_ms = t0.elapsed().as_millis() as u64;

    if cmd.json {
        let mut val = serde_json::to_value(&plan)?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert("ok".into(), serde_json::Value::Bool(true));
            obj.insert("elapsedMs".into(), serde_json::Value::from(elapsed_ms));
        }
        println!("{}", serde_json::to_string_pretty(&val)?);
    } else {
        progress.info(
            "done",
            &format!(
                "done in {:.1}s — {} → {} ({} → {}, removed {})",
                elapsed_ms as f64 / 1000.0,
                input,
                output,
                fmt_secs(meta.duration),
                fmt_secs(trimmed),
                fmt_secs(removed)
            ),
        );
    }
    Ok(())
}

fn run_doctor(cmd: DoctorCmd, cfg_path: Option<PathBuf>) -> CoreResult<()> {
    let ffmpeg_bin = ffmpeg::resolve_bin("ffmpeg");
    let ffprobe_bin = ffmpeg::resolve_bin("ffprobe");
    let ffmpeg_ver = ffmpeg::binary_version("ffmpeg");
    let ffprobe_ver = ffmpeg::binary_version("ffprobe");
    let listed_hw = ffmpeg::listed_hw_encoders();
    let working_hw = if ffmpeg_ver.is_ok() {
        ffmpeg::detect_working_hw_encoder()
    } else {
        None
    };
    let ok = ffmpeg_ver.is_ok() && ffprobe_ver.is_ok();

    if cmd.json {
        let out = serde_json::json!({
            "ok": ok,
            "version": env!("CARGO_PKG_VERSION"),
            "ffmpeg": {
                "path": ffmpeg_bin,
                "version": ffmpeg_ver.as_ref().ok(),
                "error": ffmpeg_ver.as_ref().err().map(|e| e.to_string()),
            },
            "ffprobe": {
                "path": ffprobe_bin,
                "version": ffprobe_ver.as_ref().ok(),
                "error": ffprobe_ver.as_ref().err().map(|e| e.to_string()),
            },
            "hwEncodersListed": listed_hw,
            "hwEncoderWorking": working_hw,
            "configFile": cfg_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("mustardy {} doctor", env!("CARGO_PKG_VERSION"));
        match &ffmpeg_ver {
            Ok(v) => println!("  ffmpeg:   OK  ({ffmpeg_bin}) — {v}"),
            Err(e) => println!("  ffmpeg:   MISSING ({ffmpeg_bin}) — {e}"),
        }
        match &ffprobe_ver {
            Ok(v) => println!("  ffprobe:  OK  ({ffprobe_bin}) — {v}"),
            Err(e) => println!("  ffprobe:  MISSING ({ffprobe_bin}) — {e}"),
        }
        println!(
            "  hw enc:   {} (compiled: {})",
            working_hw.as_deref().unwrap_or("none detected, using libx264"),
            if listed_hw.is_empty() {
                "none".to_string()
            } else {
                listed_hw.join(", ")
            }
        );
        println!(
            "  config:   {}",
            cfg_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none (using built-in defaults)".to_string())
        );
    }

    if ok {
        Ok(())
    } else {
        Err(CoreError::coded(
            "FFMPEG_MISSING",
            "ffmpeg and ffprobe are required on PATH (or via MUSTARDY_FFMPEG / MUSTARDY_FFPROBE)",
        ))
    }
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
    Err(CoreError::msg(
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
            Err(CoreError::msg(format!(
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

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let full: Vec<&str> = std::iter::once("mustardy")
            .chain(args.iter().copied())
            .collect();
        Cli::try_parse_from(full)
    }

    #[test]
    fn defaults_match_the_documented_settings() {
        let cli = parse(&["trim", "talk.mp4"]).unwrap();
        let Command::Trim(t) = cli.command else {
            panic!("expected trim");
        };
        let s = resolve_settings(&t.detect, Some(&t.encode), &UserConfig::default(), None);
        assert_eq!(t.inputs, vec!["talk.mp4"]);
        assert_eq!(s.drop_db, 30.0);
        assert_eq!(s.min_dur, 0.9);
        assert!(s.normalize);
        assert!(s.visual);
        assert!(s.trim_edges);
    }

    #[test]
    fn accepts_flags_presets_and_legacy_aliases() {
        let cli = parse(&[
            "trim",
            "in.mp4",
            "out.mp4",
            "--drop",
            "24dB",
            "--min=0.5s",
            "--no-normalise",
            "--pad",
            "0.12",
            "--cut",
            "0:00-0:10",
            "--json",
        ])
        .unwrap();
        let Command::Trim(t) = cli.command else {
            panic!("expected trim");
        };
        let s = resolve_settings(&t.detect, Some(&t.encode), &UserConfig::default(), None);
        assert_eq!(s.drop_db, 24.0);
        assert_eq!(s.min_dur, 0.5);
        assert!(!s.normalize);
        assert_eq!(s.pad_before, 0.12);
        assert_eq!(s.pad_after, 0.12);
        assert_eq!(t.detect.manual_cuts, vec!["0:00-0:10"]);
        assert!(t.json);
    }

    #[test]
    fn preset_tight_changes_pacing_defaults() {
        let cli = parse(&["trim", "in.mp4", "--preset", "tight"]).unwrap();
        let Command::Trim(t) = cli.command else {
            panic!("expected trim");
        };
        let s = resolve_settings(&t.detect, Some(&t.encode), &UserConfig::default(), None);
        assert_eq!(s.drop_db, 24.0);
        assert_eq!(s.min_dur, 0.45);
        assert_eq!(s.pad_before, 0.10);
        assert_eq!(s.pad_after, 0.08);
    }

    #[test]
    fn rejects_nan_and_unknown_flags() {
        assert!(parse(&["trim", "in.mp4", "--nope"]).is_err());
        assert!(parse(&["trim", "in.mp4", "--normalize-amount", "NaN"]).is_err());
        assert!(parse(&["trim", "in.mp4", "--drop", "inf"]).is_err());
    }

    #[test]
    fn parses_scan_apply_doctor_and_completions() {
        assert!(matches!(
            parse(&["scan", "talk.mp4", "--json"]).unwrap().command,
            Command::Scan(_)
        ));
        assert!(matches!(
            parse(&["apply", "plan.json", "-o", "out.mp4"])
                .unwrap()
                .command,
            Command::Apply(_)
        ));
        assert!(matches!(
            parse(&["doctor", "--json"]).unwrap().command,
            Command::Doctor(_)
        ));
        assert!(matches!(
            parse(&["completions", "zsh"]).unwrap().command,
            Command::Completions { .. }
        ));
    }

    #[test]
    fn default_output_preserves_audio_extension() {
        assert_eq!(
            default_output("/tmp/a/talk.mp4", true),
            "/tmp/a/talk-trimmed.mp4"
        );
        assert_eq!(default_output("talk.mov", true), "talk-trimmed.mp4");
        assert_eq!(default_output("pod.wav", false), "pod-trimmed.wav");
        assert_eq!(default_output("voice.mp3", false), "voice-trimmed.mp3");
    }
}
