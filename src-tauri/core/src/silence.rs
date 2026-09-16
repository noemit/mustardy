//! Silence detection shared by the headless CLI.
//!
//! This mirrors `src/lib/silence.ts` (`envelopeFromSamples` /
//! `silencesFromEnvelope`) and `src/lib/edits.ts` (`silenceCuts`) so a CLI trim
//! matches what the editor would cut, given the same drop / min-pause.

use crate::{AudioEnvelope, Change, SilenceRange};

/// dB below talking before a gap counts as a pause. Higher = keep quieter speech.
pub const DEFAULT_SILENCE_DROP: f64 = 30.0;
/// Shortest stretch worth removing.
pub const DEFAULT_SILENCE_MIN: f64 = 0.9;
const MIN_DROP: f64 = 8.0;
const MAX_DROP: f64 = 35.0;
const HYST_DB: f64 = 4.0;
const NOISE_HEADROOM_DB: f64 = 3.0;

/// RMS windows over PCM, in dBFS. One scan; thresholds re-read this.
pub fn envelope_from_pcm(data: &[f32], sample_rate: u32) -> AudioEnvelope {
    let sr = sample_rate.max(1) as f64;
    let hop_samples = ((sr * 0.02).round() as usize).max(1);
    let hop = hop_samples as f64 / sr;
    let mut dbs = Vec::new();
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

/// Pause = a stretch near room tone, not "anything quieter than a loud word."
/// Talking level is the 80th percentile; a gap must fall `drop_db` below that
/// (and stay a bit above the noise floor so the threshold can't dive into hiss).
pub fn silences_from_envelope(
    env: &AudioEnvelope,
    drop_db: f64,
    min_dur: f64,
) -> Vec<SilenceRange> {
    let (hop, dbs) = (&env.hop, &env.dbs);
    if dbs.is_empty() || !(*hop > 0.0) {
        return Vec::new();
    }
    let min = min_dur.max(0.0);
    let drop = drop_db.clamp(MIN_DROP, MAX_DROP);
    let mut sorted: Vec<f64> = dbs.iter().map(|&x| x as f64).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise = percentile(&sorted, 12.0);
    let speech = percentile(&sorted, 80.0);
    let contrast = speech - noise;
    let thr = if contrast < 8.0 {
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
    merged
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

/// Build silence trims. When a pause hides a visual change (scene cut, slide
/// switch), the trim splits around it: the static sides go, the transition
/// stays. `visuals` is parallel to `ranges` (empty when no scan ran).
pub fn silence_cuts(
    ranges: &[SilenceRange],
    visuals: &[Vec<f64>],
    pad: f64,
    transition_margin: f64,
) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    for (ri, r) in ranges.iter().enumerate() {
        let span = r.end - r.start;
        let edge = pad.min((span - 0.02).max(0.0) * 0.3);
        let mut splits: Vec<f64> = visuals
            .get(ri)
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|&t| t > r.start + edge && t < r.end - edge)
                    .collect()
            })
            .unwrap_or_default();
        splits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut bounds = Vec::with_capacity(splits.len() + 2);
        bounds.push(r.start);
        bounds.extend_from_slice(&splits);
        bounds.push(r.end);

        for i in 0..bounds.len() - 1 {
            let start = if i == 0 {
                bounds[i] + edge
            } else {
                bounds[i] + transition_margin
            };
            let end = if i + 1 == bounds.len() - 1 {
                bounds[i + 1] - edge
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
}
