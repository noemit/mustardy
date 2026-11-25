//! Reusable cut plans (`scan` -> edit -> `apply`), manual time-range parsing,
//! and deterministic `.srt` subtitle word-cut helpers.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ffmpeg::{self, Seg};
use crate::{round3, AudioStats, Change, CoreError, CoreResult, VideoMeta};

pub const PLAN_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCut {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSettings {
    #[serde(rename = "dropDb")]
    pub drop_db: f64,
    #[serde(rename = "minPause")]
    pub min_pause: f64,
    #[serde(rename = "padBefore")]
    pub pad_before: f64,
    #[serde(rename = "padAfter")]
    pub pad_after: f64,
    #[serde(rename = "transitionMargin")]
    pub transition_margin: f64,
    #[serde(rename = "trimEdges")]
    pub trim_edges: bool,
    pub visual: bool,
    pub normalize: bool,
    #[serde(rename = "normalizeAmount")]
    pub normalize_amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutPlan {
    #[serde(rename = "schemaVersion", default = "default_schema_version")]
    pub schema_version: u32,
    pub input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(rename = "dryRun", default)]
    pub dry_run: bool,
    pub duration: f64,
    #[serde(rename = "trimmedDuration")]
    pub trimmed_duration: f64,
    pub removed: f64,
    pub silences: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<PlanSettings>,
    pub cuts: Vec<PlanCut>,
    #[serde(default)]
    pub keeps: Vec<Seg>,
}

fn default_schema_version() -> u32 {
    PLAN_SCHEMA_VERSION
}

impl CutPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn from_analysis(
        input: String,
        output: Option<String>,
        dry_run: bool,
        meta: &VideoMeta,
        audio: Option<AudioStats>,
        settings: PlanSettings,
        silences: usize,
        cuts: &[Change],
    ) -> Self {
        let keeps = if cuts.is_empty() {
            vec![Seg {
                start: 0.0,
                end: round3(meta.duration),
            }]
        } else {
            ffmpeg::kept_segments(meta.duration, cuts)
                .into_iter()
                .map(|s| Seg {
                    start: round3(s.start),
                    end: round3(s.end),
                })
                .collect()
        };
        let trimmed: f64 = keeps.iter().map(|s| s.end - s.start).sum();
        let removed = (meta.duration - trimmed).max(0.0);

        let plan_cuts = cuts
            .iter()
            .map(|c| PlanCut {
                start: round3(c.start),
                end: round3(c.end),
                label: c.label.clone(),
                origin: c.origin.clone(),
            })
            .collect();

        Self {
            schema_version: PLAN_SCHEMA_VERSION,
            input,
            output,
            dry_run,
            duration: round3(meta.duration),
            trimmed_duration: round3(trimmed),
            removed: round3(removed),
            silences,
            video: Some(meta.clone()),
            audio,
            settings: Some(settings),
            cuts: plan_cuts,
            keeps,
        }
    }

    pub fn to_changes(&self) -> Vec<Change> {
        self.cuts
            .iter()
            .enumerate()
            .map(|(i, c)| Change {
                id: format!("plan-cut-{}", i + 1),
                kind: "cut".to_string(),
                start: c.start,
                end: c.end,
                status: "accepted".to_string(),
                label: if c.label.is_empty() {
                    format!("Cut {:.2}s–{:.2}s", c.start, c.end)
                } else {
                    c.label.clone()
                },
                rationale: "Loaded from plan.".to_string(),
                origin: c.origin.clone().or_else(|| Some("plan".to_string())),
                text: None,
                style: None,
                pan: None,
                rate: None,
            })
            .collect()
    }

    pub fn load(path: &Path) -> CoreResult<Self> {
        let text = std::fs::read_to_string(path)?;
        let plan: Self = serde_json::from_str(&text)
            .map_err(|e| CoreError::coded("INVALID_PLAN", format!("invalid plan JSON: {e}")))?;
        Ok(plan)
    }

    pub fn save(&self, path: &Path) -> CoreResult<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let body = serde_json::to_string_pretty(self)?;
        std::fs::write(path, body)?;
        Ok(())
    }
}

/// Parse a timestamp like `"1:05.5"`, `"00:01:05.250"`, `"90s"`, `"12.5"`, or `"start"`/`"end"`.
pub fn parse_timestamp(raw: &str, duration: f64) -> CoreResult<f64> {
    let t = raw.trim().to_ascii_lowercase();
    if t == "start" || t == "begin" {
        return Ok(0.0);
    }
    if t == "end" || t == "eof" {
        return Ok(duration);
    }
    if t.contains(':') {
        let parts: Vec<&str> = t.split(':').collect();
        let secs = match parts.as_slice() {
            [m, s] => {
                let mm: f64 = m
                    .parse()
                    .map_err(|_| CoreError::coded("INVALID_ARGS", format!("bad time '{raw}'")))?;
                let ss: f64 = s
                    .parse()
                    .map_err(|_| CoreError::coded("INVALID_ARGS", format!("bad time '{raw}'")))?;
                mm * 60.0 + ss
            }
            [h, m, s] => {
                let hh: f64 = h
                    .parse()
                    .map_err(|_| CoreError::coded("INVALID_ARGS", format!("bad time '{raw}'")))?;
                let mm: f64 = m
                    .parse()
                    .map_err(|_| CoreError::coded("INVALID_ARGS", format!("bad time '{raw}'")))?;
                let ss: f64 = s
                    .parse()
                    .map_err(|_| CoreError::coded("INVALID_ARGS", format!("bad time '{raw}'")))?;
                hh * 3600.0 + mm * 60.0 + ss
            }
            _ => {
                return Err(CoreError::coded(
                    "INVALID_ARGS",
                    format!("bad timestamp '{raw}' (use mm:ss, hh:mm:ss, or seconds)"),
                ));
            }
        };
        if !secs.is_finite() || secs < 0.0 {
            return Err(CoreError::coded(
                "INVALID_ARGS",
                format!("timestamp must be non-negative, got '{raw}'"),
            ));
        }
        return Ok(secs.min(duration));
    }
    let stripped = t.trim_end_matches('s').trim();
    let val: f64 = stripped
        .parse()
        .map_err(|_| CoreError::coded("INVALID_ARGS", format!("bad timestamp '{raw}'")))?;
    if !val.is_finite() || val < 0.0 {
        return Err(CoreError::coded(
            "INVALID_ARGS",
            format!("timestamp must be non-negative, got '{raw}'"),
        ));
    }
    Ok(val.min(duration))
}

/// Parse a manual cut specification like `"0:00-0:12"`, `"14:20-end"`, `"5s..10s"`.
pub fn parse_cut_range(spec: &str, duration: f64) -> CoreResult<Change> {
    let (a, b) = spec
        .split_once("..")
        .or_else(|| spec.split_once('-'))
        .ok_or_else(|| {
            CoreError::coded(
                "INVALID_ARGS",
                format!("--cut expects START-END (e.g. 0:00-0:12 or 14:20-end), got '{spec}'"),
            )
        })?;
    let start = parse_timestamp(a, duration)?;
    let end = parse_timestamp(b, duration)?;
    if end <= start + 0.02 {
        return Err(CoreError::coded(
            "INVALID_ARGS",
            format!("--cut range '{spec}' has empty or inverted span ({start:.2}s..{end:.2}s)"),
        ));
    }
    Ok(Change {
        id: format!("manual-{:.2}-{:.2}", start, end),
        kind: "cut".to_string(),
        start,
        end,
        status: "accepted".to_string(),
        label: format!("Manual cut {spec}"),
        rationale: "Specified via --cut.".to_string(),
        origin: Some("manual".to_string()),
        text: None,
        style: None,
        pan: None,
        rate: None,
    })
}

/// Parse an `.srt` subtitle file and return cuts for cues matching any of the
/// given words/phrases (case-insensitive whole-word or substring phrase match).
pub fn cuts_from_srt(srt_path: &Path, targets: &[String], duration: f64) -> CoreResult<Vec<Change>> {
    let text = std::fs::read_to_string(srt_path)?;
    let needles: Vec<String> = targets
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if needles.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let lines: Vec<&str> = block.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if lines.len() < 2 {
            continue;
        }
        let Some(time_idx) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((s_raw, e_raw)) = lines[time_idx].split_once("-->") else {
            continue;
        };
        let Ok(start) = parse_srt_time(s_raw) else {
            continue;
        };
        let Ok(end) = parse_srt_time(e_raw) else {
            continue;
        };
        let cue_text = lines[time_idx + 1..].join(" ").to_ascii_lowercase();
        let tokens: Vec<&str> = cue_text
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|t| !t.is_empty())
            .collect();

        let matched = needles.iter().find(|needle| {
            if needle.contains(' ') {
                cue_text.contains(needle.as_str())
            } else {
                tokens.contains(&needle.as_str())
            }
        });
        if let Some(word) = matched {
            let s = start.clamp(0.0, duration);
            let e = end.clamp(0.0, duration);
            if e - s >= 0.05 {
                out.push(Change {
                    id: format!("srt-{}", out.len() + 1),
                    kind: "cut".to_string(),
                    start: s,
                    end: e,
                    status: "accepted".to_string(),
                    label: format!("Cut transcript match '{word}'"),
                    rationale: format!("Matched SRT cue: {}", lines[time_idx + 1..].join(" ")),
                    origin: Some("srt".to_string()),
                    text: None,
                    style: None,
                    pan: None,
                    rate: None,
                });
            }
        }
    }
    Ok(out)
}

fn parse_srt_time(raw: &str) -> Result<f64, ()> {
    let cleaned = raw.trim().replace(',', ".");
    let parts: Vec<&str> = cleaned.split(':').collect();
    if parts.len() != 3 {
        return Err(());
    }
    let h: f64 = parts[0].parse().map_err(|_| ())?;
    let m: f64 = parts[1].parse().map_err(|_| ())?;
    let s: f64 = parts[2].parse().map_err(|_| ())?;
    Ok(h * 3600.0 + m * 60.0 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps_and_cut_ranges() {
        assert!((parse_timestamp("1:05.5", 200.0).unwrap() - 65.5).abs() < 1e-9);
        assert!((parse_timestamp("00:02:00", 200.0).unwrap() - 120.0).abs() < 1e-9);
        assert!((parse_timestamp("45s", 200.0).unwrap() - 45.0).abs() < 1e-9);
        assert!((parse_timestamp("end", 200.0).unwrap() - 200.0).abs() < 1e-9);

        let cut = parse_cut_range("0:00-0:12", 100.0).unwrap();
        assert!((cut.start - 0.0).abs() < 1e-9);
        assert!((cut.end - 12.0).abs() < 1e-9);

        let tail = parse_cut_range("1:20-end", 100.0).unwrap();
        assert!((tail.start - 80.0).abs() < 1e-9);
        assert!((tail.end - 100.0).abs() < 1e-9);
    }

    #[test]
    fn srt_word_and_phrase_matching() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-scratch");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("test-sub.srt");
        std::fs::write(
            &p,
            "1\n00:00:01,000 --> 00:00:01,600\nUm\n\n2\n00:00:02,000 --> 00:00:03,500\nHello world\n\n3\n00:00:04,000 --> 00:00:05,200\nplease cut that\n",
        )
        .unwrap();

        let cuts = cuts_from_srt(&p, &["um".into(), "cut that".into()], 10.0).unwrap();
        assert_eq!(cuts.len(), 2);
        assert!((cuts[0].start - 1.0).abs() < 1e-6);
        assert!((cuts[1].start - 4.0).abs() < 1e-6);
        let _ = std::fs::remove_file(&p);
    }
}
