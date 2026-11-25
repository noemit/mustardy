//! Visual-change scan inside silence ranges — pure pixel diff on tiny raw
//! frames, no model. Where the picture changes mid-pause (scene cut, slide
//! switch, fade/dissolve, screen share kicks in), the change moment is
//! reported so trims split around it instead of cutting through it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::ffmpeg;
use crate::{CoreResult, SilenceRange};

/// Frames this small make the diff about structure, not sensor noise.
const DIFF_W: u32 = 64;
const DIFF_H: u32 = 36;
/// A pixel counts as changed when its mean RGB shift passes this (0–255).
const PIXEL_SHIFT: i32 = 40;
/// …and a frame pair counts as a visual change when this fraction of pixels
/// changed. Scene/slide cuts move 40–95%; a talking head leaning stays < 20%.
const CHANGE_FRAC: f64 = 0.28;
/// Shorter pauses can't spare a protected gap and still leave trimmable
/// sides, so there's nothing to scan.
const MIN_SCAN_DUR: f64 = 0.6;
/// Long pauses don't need denser sampling than this.
const MAX_FRAMES_PER_RANGE: usize = 24;
/// Default cap on total pauses scanned on pathological files.
pub const DEFAULT_MAX_RANGES: usize = 1000;

/// Result of a parallel visual-change scan.
#[derive(Debug, Clone)]
pub struct VisualScanResult {
    /// Transition timestamps per input silence range (range-aligned).
    pub times: Vec<Vec<f64>>,
    /// How many ranges were scanned.
    pub scanned: usize,
    /// How many ranges were skipped due to `max_ranges` cap.
    pub truncated: usize,
    /// How many ranges had at least one visual change.
    pub changed_ranges: usize,
}

/// Parallel visual-change scan across up to `jobs` worker threads (0 = auto).
pub fn visual_change_times_parallel<F>(
    input: &str,
    ranges: &[SilenceRange],
    jobs: usize,
    max_ranges: usize,
    on_progress: F,
) -> CoreResult<VisualScanResult>
where
    F: Fn(usize, usize) + Sync,
{
    let total = ranges.len();
    let limit = if max_ranges == 0 {
        total
    } else {
        total.min(max_ranges)
    };
    let truncated = total.saturating_sub(limit);
    let mut out = vec![Vec::new(); total];

    if limit == 0 {
        return Ok(VisualScanResult {
            times: out,
            scanned: 0,
            truncated,
            changed_ranges: 0,
        });
    }

    let workers = if jobs == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 6)
    } else {
        jobs.clamp(1, 16)
    }
    .min(limit);

    let next_idx = AtomicUsize::new(0);
    let done_count = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Vec<f64>)>> = Mutex::new(Vec::with_capacity(limit));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next_idx = &next_idx;
            let done_count = &done_count;
            let results = &results;
            let on_progress = &on_progress;
            scope.spawn(move || loop {
                let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                if idx >= limit {
                    break;
                }
                let points = scan_range(input, &ranges[idx]).unwrap_or_default();
                if let Ok(mut guard) = results.lock() {
                    guard.push((idx, points));
                }
                let finished = done_count.fetch_add(1, Ordering::Relaxed) + 1;
                on_progress(finished, limit);
            });
        }
    });

    let mut changed_ranges = 0usize;
    if let Ok(mut guard) = results.into_inner() {
        for (idx, points) in guard.drain(..) {
            if !points.is_empty() {
                changed_ranges += 1;
            }
            out[idx] = points;
        }
    }

    crate::log::debug(&format!(
        "visual scan ({workers} worker(s)): {changed_ranges}/{limit} quiet stretch(es) change on screen{}",
        if truncated > 0 {
            format!(" ({truncated} skipped past cap)")
        } else {
            String::new()
        }
    ));

    Ok(VisualScanResult {
        times: out,
        scanned: limit,
        truncated,
        changed_ranges,
    })
}

/// Backwards-compatible wrapper used by the desktop shell.
pub fn visual_change_times(input: &str, ranges: &[SilenceRange]) -> CoreResult<Vec<Vec<f64>>> {
    Ok(
        visual_change_times_parallel(input, ranges, 0, DEFAULT_MAX_RANGES, |_, _| {})?
            .times,
    )
}

fn scan_range(input: &str, r: &SilenceRange) -> CoreResult<Vec<f64>> {
    let dur = r.end - r.start;
    if dur < MIN_SCAN_DUR {
        return Ok(Vec::new());
    }
    // ~4 samples per second of pause, capped: a 0.8 s pause gets ~4 frames,
    // a 3 s pause gets 12, anything long plateaus at MAX_FRAMES_PER_RANGE.
    let n = ((dur / 0.25).round() as usize).clamp(4, MAX_FRAMES_PER_RANGE);
    let fps = n as f64 / dur;
    let frames = ffmpeg::extract_range_frames(input, r.start, r.end, fps, DIFF_W, DIFF_H)?;
    Ok(detect_change_points(&frames, r.start, fps))
}

/// Detect both abrupt scene cuts (frame-to-frame) and gradual transitions
/// (dissolves/fades relative to the last stable anchor frame).
fn detect_change_points(frames: &[Vec<u8>], start: f64, fps: f64) -> Vec<f64> {
    if frames.len() < 2 || fps <= 0.0 {
        return Vec::new();
    }
    let mut points = Vec::new();
    let mut anchor = &frames[0];
    for i in 1..frames.len() {
        let step_diff = changed_frac(&frames[i - 1], &frames[i]);
        let anchor_diff = changed_frac(anchor, &frames[i]);
        if step_diff > CHANGE_FRAC || anchor_diff > CHANGE_FRAC {
            // Midpoint between frame i-1 and frame i.
            let t = ((start + (i as f64) / fps) * 100.0).round() / 100.0;
            points.push(t);
            anchor = &frames[i];
        }
    }
    points
}

/// Fraction of pixels whose mean per-channel shift exceeds PIXEL_SHIFT.
fn changed_frac(a: &[u8], b: &[u8]) -> f64 {
    let px = a.len() / 3;
    if px == 0 || b.len() / 3 != px {
        return 0.0;
    }
    let mut changed = 0usize;
    for i in 0..px {
        let d = (a[i * 3] as i32 - b[i * 3] as i32).abs()
            + (a[i * 3 + 1] as i32 - b[i * 3 + 1] as i32).abs()
            + (a[i * 3 + 2] as i32 - b[i * 3 + 2] as i32).abs();
        if d / 3 > PIXEL_SHIFT {
            changed += 1;
        }
    }
    changed as f64 / px as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_frame(val: u8) -> Vec<u8> {
        vec![val; (DIFF_W * DIFF_H * 3) as usize]
    }

    #[test]
    fn static_frames_produce_no_changes() {
        let frames = vec![solid_frame(50); 6];
        assert!(detect_change_points(&frames, 10.0, 4.0).is_empty());
    }

    #[test]
    fn hard_scene_cut_detected_once() {
        let frames = vec![
            solid_frame(20),
            solid_frame(20),
            solid_frame(200),
            solid_frame(200),
        ];
        let pts = detect_change_points(&frames, 10.0, 4.0);
        assert_eq!(pts.len(), 1);
        assert!((pts[0] - 10.5).abs() < 1e-6);
    }

    #[test]
    fn gradual_fade_detected_via_anchor_frame() {
        // Each step moves by 25 (< PIXEL_SHIFT=40), so pairwise diff would miss it,
        // but total drift from anchor (20 -> 70 -> 95) crosses PIXEL_SHIFT cleanly.
        let frames = vec![
            solid_frame(20),
            solid_frame(45),
            solid_frame(70),
            solid_frame(95),
        ];
        let pts = detect_change_points(&frames, 5.0, 4.0);
        assert!(!pts.is_empty());
    }
}
