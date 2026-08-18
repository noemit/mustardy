//! Visual-change scan inside silence ranges — pure pixel diff on tiny raw
//! frames, no model. Where the picture changes mid-pause (scene cut, slide
//! switch, screen share kicks in), the change moment is reported so the UI
//! can trim the static sides of the pause and keep the transition. Cutting
//! through a visual change is what makes silence trims feel jumpy.

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
/// Pathological files can report hundreds of pauses — cap the total work.
const MAX_RANGES: usize = 400;

/// For each silence range, the times where the picture materially changes.
/// Range-aligned: `out[i]` holds the transition points inside `ranges[i]`
/// (empty when the pause is visually static — or the frames couldn't be read).
pub fn visual_change_times(input: &str, ranges: &[SilenceRange]) -> CoreResult<Vec<Vec<f64>>> {
    let mut out: Vec<Vec<f64>> = Vec::with_capacity(ranges.len());
    let mut changed = 0usize;
    for r in ranges.iter().take(MAX_RANGES) {
        let points = scan_range(input, r).unwrap_or_default();
        if !points.is_empty() {
            changed += 1;
        }
        out.push(points);
    }
    crate::log::line(&format!(
        "visual scan: {}/{} quiet stretch(es) change on screen",
        changed,
        ranges.len()
    ));
    Ok(out)
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

    let mut points = Vec::new();
    for (i, pair) in frames.windows(2).enumerate() {
        if changed_frac(&pair[0], &pair[1]) > CHANGE_FRAC {
            // Midpoint of the pair: frame i ≈ start + i/fps.
            points.push(((r.start + (i as f64 + 1.0) / fps) * 100.0).round() / 100.0);
        }
    }
    Ok(points)
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
