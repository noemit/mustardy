//! Silence detection and cut planning shared by the CLI and desktop shell.
//!
//! Mirrors `src/lib/silence.ts` (`envelopeFromSamples` / `silencesFromEnvelope`)
//! and `src/lib/edits.ts` (`silenceCuts`) while also returning audio diagnostics
//! (`AudioStats`) and supporting asymmetric padding + clean edge trimming.

use crate::{AudioEnvelope, AudioStats, Change, SilenceRange, round3};

/// dB below talking before a gap counts as a pause. Higher = keep quieter speech.
pub const DEFAULT_SILENCE_DROP: f64 = 30.0;
/// Shortest stretch worth removing.
pub const DEFAULT_SILENCE_MIN: f64 = 0.9;
/// Default breathing room kept on each side of a cut.
pub const DEFAULT_PAD_S: f64 = 0.18;
/// Default margin kept on each side of a mid-pause visual transition.
pub const DEFAULT_TRANSITION_MARGIN_S: f64 = 0.20;

pub const MIN_DROP: f64 = 8.0;
pub const MAX_DROP: f64 = 35.0;
const HYST_DB: f64 = 4.0;
const NOISE_HEADROOM_DB: f64 = 3.0;
const EDGE_EPS_S: f64 = 0.04;

/// Output of analyzing an audio envelope.
#[derive(Debug, Clone)]
pub struct EnvelopeAnalysis {
    pub ranges: Vec<SilenceRange>,
    pub stats: AudioStats,
}

/// Parameters controlling how detected silences become cut segments.
#[derive(Debug, Clone, Copy)]
pub struct SilenceCutOptions {
    /// Breathing room kept after speech ends (at the start of a pause).
    pub pad_before: f64,
    /// Breathing room kept before speech resumes (at the end of a pause).
    pub pad_after: f64,
    /// Protected margin on either side of a mid-pause visual transition.
    pub transition_margin: f64,
    /// When true, leading silence at 0:00 and trailing silence at EOF are cut
    /// all the way to the boundary without leaving dead air on the outer edge.
    pub trim_edges: bool,
    /// Total duration of the media (used with `trim_edges` to detect EOF silence).
    pub duration: Option<f64>,
}

impl Default for SilenceCutOptions {
    fn default() -> Self {
        Self {
            pad_before: DEFAULT_PAD_S,
            pad_after: DEFAULT_PAD_S,
            transition_margin: DEFAULT_TRANSITION_MARGIN_S,
            trim_edges: true,
            duration: None,
        }
    }
}

/// RMS windows over PCM, in dBFS. One scan; thresholds re-read this.
pub fn envelope_from_pcm(data: &[f32], sample_rate: u32) -> AudioEnvelope {
    let sr = sample_rate.max(1) as f64;
    let hop_samples = ((sr * 0.02).round() as usize).max(1);
    let hop = hop_samples as f64 / sr;
    let mut dbs = Vec::with_capacity(data.len().div_ceil(hop_samples));
    let mut i = 0usize;
    while i < data.len() {
        let end = (i + hop_samples).min(data.len());
        let mut sum = 0.0f64;
        for &x in &data[i..end] {
            sum += (x as f64) * (x as f64);
        }
        let rms = (sum / (end - i).max(1) as f64).sqrt();
        dbs.push((20.0 * rms.max(1e-9).log10()) as f32);
        i += hop_samples;
    }
    AudioEnvelope { hop, dbs }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return -80.0;
    }
    let raw = ((p / 100.0) * (sorted.len() - 1) as f64).round();
    let i = (raw.max(0.0) as usize).min(sorted.len() - 1);
    sorted[i]
}

/// Analyze an audio envelope into silence ranges plus audio level diagnostics.
///
/// Pause = a stretch near room tone, not "anything quieter than a loud word."
/// Talking level is the 80th percentile; a gap must fall `drop_db` below that
/// (and stay a bit above the noise floor so the threshold can't dive into hiss).
pub fn analyze_envelope(
    env: &AudioEnvelope,
    drop_db: f64,
    min_dur: f64,
) -> EnvelopeAnalysis {
    let (hop, dbs) = (&env.hop, &env.dbs);
    if dbs.is_empty() || !(*hop > 0.0) {
        return EnvelopeAnalysis {
            ranges: Vec::new(),
            stats: AudioStats {
                speech_db: -80.0,
                noise_floor_db: -80.0,
                threshold_db: -80.0,
                contrast_db: 0.0,
                low_contrast: true,
            },
        };
    }
    let min = min_dur.max(0.0);
    let drop = drop_db.clamp(MIN_DROP, MAX_DROP);
    let mut sorted: Vec<f64> = dbs.iter().map(|&x| x as f64).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise = percentile(&sorted, 12.0);
    let speech = percentile(&sorted, 80.0);
    let contrast = speech - noise;
    let low_contrast = contrast < 8.0;
    let thr = if low_contrast {
        speech - drop
    } else {
        (noise + NOISE_HEADROOM_DB).max(speech - drop)
    };

    let mut ranges: Vec<SilenceRange> = Vec::new();
    let mut start: Option<f64> = None;
    let mut in_pause = false;
    for (i, &db) in dbs.iter().enumerate() {
        let t = i as f64 * hop;
        let db = db as f64;
        if !in_pause {
            if db < thr {
                in_pause = true;
                start = Some(t);
            }
        } else if db > thr + HYST_DB {
            if let Some(st) = start.take() {
                if t - st >= min {
                    ranges.push(SilenceRange { start: st, end: t });
                }
            }
            in_pause = false;
        }
    }
    if in_pause {
        if let Some(st) = start {
            let end = dbs.len() as f64 * hop;
            if end - st >= min {
                ranges.push(SilenceRange { start: st, end });
            }
        }
    }

    let gap = 0.05f64.min(min);
    let mut merged: Vec<SilenceRange> = Vec::new();
    for r in ranges {
        if let Some(last) = merged.last_mut() {
            if r.start - last.end <= gap {
                last.end = r.end;
                continue;
            }
        }
        merged.push(r);
    }

    EnvelopeAnalysis {
        ranges: merged,
        stats: AudioStats {
            speech_db: round3(speech),
            noise_floor_db: round3(noise),
            threshold_db: round3(thr),
            contrast_db: round3(contrast),
            low_contrast,
        },
    }
}

/// Convenience wrapper returning only the silence ranges.
pub fn silences_from_envelope(
    env: &AudioEnvelope,
    drop_db: f64,
    min_dur: f64,
) -> Vec<SilenceRange> {
    analyze_envelope(env, drop_db, min_dur).ranges
}

fn format_time(seconds: f64) -> String {
    let s = if !seconds.is_finite() || seconds < 0.0 {
        0.0
    } else {
        seconds
    };
    let m = (s / 60.0).floor() as i64;
    let sec = (s % 60.0).floor() as i64;
    let f = ((s % 1.0) * 10.0).floor() as i64;
    format!("{m}:{sec:02}.{f}")
}

/// Build silence trims with configurable padding and edge trimming.
pub fn silence_cuts_with_options(
    ranges: &[SilenceRange],
    visuals: &[Vec<f64>],
    opts: &SilenceCutOptions,
) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let pad_before = opts.pad_before.max(0.0);
    let pad_after = opts.pad_after.max(0.0);
    let transition_margin = opts.transition_margin.max(0.0);

    for (ri, r) in ranges.iter().enumerate() {
        let span = r.end - r.start;
        if span <= 0.02 {
            continue;
        }
        let is_leading = opts.trim_edges && r.start <= EDGE_EPS_S;
        let is_trailing = opts
            .duration
            .is_some_and(|dur| opts.trim_edges && r.end >= dur - EDGE_EPS_S);

        // Shrink pads proportionally on short pauses so they can still be cut cleanly.
        let mut edge_start = if is_leading {
            0.0
        } else {
            pad_before.min((span - 0.02).max(0.0) * 0.3)
        };
        let mut edge_end = if is_trailing {
            0.0
        } else {
            pad_after.min((span - 0.02).max(0.0) * 0.3)
        };
        let total_pad = edge_start + edge_end;
        let max_pad = (span - 0.02).max(0.0) * 0.6;
        if total_pad > max_pad && total_pad > 0.0 {
            let scale = max_pad / total_pad;
            edge_start *= scale;
            edge_end *= scale;
        }

        let range_start = if is_leading { 0.0 } else { r.start };
        let range_end = if is_trailing {
            opts.duration.unwrap_or(r.end)
        } else {
            r.end
        };

        let mut splits: Vec<f64> = visuals
            .get(ri)
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|&t| t > range_start + edge_start && t < range_end - edge_end)
                    .collect()
            })
            .unwrap_or_default();
        splits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut bounds = Vec::with_capacity(splits.len() + 2);
        bounds.push(range_start);
        bounds.extend_from_slice(&splits);
        bounds.push(range_end);

        for i in 0..bounds.len() - 1 {
            let start = if i == 0 {
                bounds[i] + edge_start
            } else {
                bounds[i] + transition_margin
            };
            let end = if i + 1 == bounds.len() - 1 {
                bounds[i + 1] - edge_end
            } else {
                bounds[i + 1] - transition_margin
            };
            let dur = end - start;
            if dur < 0.02 {
                continue;
            }
            let dur_label = if dur < 1.0 {
                format!("{dur:.2}s")
            } else {
                format!("{dur:.1}s")
            };
            let (label, rationale) = if splits.is_empty() {
                (
                    format!("Trim {dur_label} silence"),
                    "Dead air — keep a short breath on either side.".to_string(),
                )
            } else {
                let times = splits
                    .iter()
                    .map(|&t| format_time(t))
                    .collect::<Vec<_>>()
                    .join(", ");
                (
                    format!("Trim {dur_label} silence — kept the visual change at {times}"),
                    "Dead air, but the picture changes mid-pause — that moment stays.".to_string(),
                )
            };
            out.push(Change {
                id: format!("cut-{}", out.len() + 1),
                kind: "cut".to_string(),
                start,
                end,
                status: "accepted".to_string(),
                label,
                rationale,
                origin: Some("silence".to_string()),
                text: None,
                style: None,
                pan: None,
                rate: None,
            });
        }
    }
    out
}

/// Build silence trims (symmetric padding, preserves exact desktop-app behavior).
pub fn silence_cuts(
    ranges: &[SilenceRange],
    visuals: &[Vec<f64>],
    pad: f64,
    transition_margin: f64,
) -> Vec<Change> {
    silence_cuts_with_options(
        ranges,
        visuals,
        &SilenceCutOptions {
            pad_before: pad,
            pad_after: pad,
            transition_margin,
            trim_edges: false,
            duration: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f64, amp: f32, sr: u32) -> Vec<f32> {
        vec![amp; (seconds * sr as f64) as usize]
    }

    #[test]
    fn flags_a_quiet_stretch_well_below_talking() {
        let sr = 16000;
        let mut data = tone(1.0, 0.2, sr);
        data.extend(tone(1.0, 0.0001, sr));
        let env = envelope_from_pcm(&data, sr);
        let ranges = silences_from_envelope(&env, 16.0, 0.2);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].start > 0.8);
        assert!(ranges[0].end > 1.5);
    }

    #[test]
    fn keeps_quieter_speech_above_room_tone() {
        let sr = 16000;
        let mut data = tone(3.0, 0.2, sr);
        data.extend(tone(0.5, 0.04, sr));
        data.extend(tone(0.5, 0.0001, sr));
        let env = envelope_from_pcm(&data, sr);
        let ranges = silences_from_envelope(&env, 18.0, 0.2);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].start > 3.3);
    }

    #[test]
    fn larger_drop_finds_less_silence() {
        let sr = 16000;
        let mut data = tone(1.0, 0.15, sr);
        data.extend(tone(0.6, 0.003, sr));
        let env = envelope_from_pcm(&data, sr);
        let tight = silences_from_envelope(&env, 24.0, 0.15);
        let loose = silences_from_envelope(&env, 10.0, 0.15);
        let span = |rs: &[SilenceRange]| rs.iter().map(|r| r.end - r.start).sum::<f64>();
        assert!(span(&loose) >= span(&tight));
    }

    #[test]
    fn pads_plain_silences_and_labels_the_trim() {
        let cuts = silence_cuts(
            &[SilenceRange {
                start: 10.0,
                end: 15.0,
            }],
            &[],
            0.18,
            0.2,
        );
        assert_eq!(cuts.len(), 1);
        assert!((cuts[0].start - 10.18).abs() < 1e-9);
        assert!((cuts[0].end - 14.82).abs() < 1e-9);
        assert!(cuts[0].label.contains("4.6s"));
        assert!(!cuts[0].rationale.contains("picture"));
    }

    #[test]
    fn splits_around_a_mid_pause_visual_change() {
        let cuts = silence_cuts(
            &[SilenceRange {
                start: 10.0,
                end: 15.0,
            }],
            &[vec![12.0]],
            0.18,
            0.2,
        );
        assert_eq!(cuts.len(), 2);
        assert!((cuts[0].end - 11.8).abs() < 1e-9);
        assert!((cuts[1].start - 12.2).abs() < 1e-9);
        assert!(cuts[1].rationale.contains("picture changes"));
    }

    #[test]
    fn auto_accepts_silence_trims() {
        let cuts = silence_cuts(
            &[SilenceRange {
                start: 10.0,
                end: 15.0,
            }],
            &[],
            0.18,
            0.2,
        );
        assert_eq!(cuts[0].status, "accepted");
        assert_eq!(cuts[0].origin.as_deref(), Some("silence"));
    }

    #[test]
    fn trims_leading_and_trailing_edges_cleanly() {
        let cuts = silence_cuts_with_options(
            &[
                SilenceRange {
                    start: 0.0,
                    end: 2.0,
                },
                SilenceRange {
                    start: 8.0,
                    end: 10.0,
                },
            ],
            &[],
            &SilenceCutOptions {
                pad_before: 0.15,
                pad_after: 0.10,
                transition_margin: 0.2,
                trim_edges: true,
                duration: Some(10.0),
            },
        );
        assert_eq!(cuts.len(), 2);
        assert!((cuts[0].start - 0.0).abs() < 1e-9);
        assert!((cuts[0].end - 1.90).abs() < 1e-9);
        assert!((cuts[1].start - 8.15).abs() < 1e-9);
        assert!((cuts[1].end - 10.0).abs() < 1e-9);
    }

    #[test]
    fn analyzes_audio_stats_for_low_and_high_contrast() {
        let sr = 16000;
        let flat = tone(2.0, 0.1, sr);
        let flat_res = analyze_envelope(&envelope_from_pcm(&flat, sr), 24.0, 0.3);
        assert!(flat_res.stats.low_contrast);
        assert!(flat_res.ranges.is_empty());

        let mut speech_and_pause = tone(2.0, 0.2, sr);
        speech_and_pause.extend(tone(1.5, 0.0002, sr));
        let res = analyze_envelope(&envelope_from_pcm(&speech_and_pause, sr), 24.0, 0.5);
        assert!(!res.stats.low_contrast);
        assert!(res.stats.speech_db > res.stats.noise_floor_db + 20.0);
        assert_eq!(res.ranges.len(), 1);
    }
}
