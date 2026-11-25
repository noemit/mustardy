//! Zero-reencode NLE timeline exporters:
//! - CMX 3600 EDL (`.edl`) — DaVinci Resolve, Adobe Premiere Pro, Avid, Kdenlive
//! - Final Cut Pro XML v1.10 (`.fcpxml`) — Final Cut Pro, DaVinci Resolve, Premiere Pro

use std::path::Path;

use crate::ffmpeg::Seg;
use crate::{CoreResult, VideoMeta};

/// Rational timebase `(frame_num, frame_den)` matching common video frame rates.
/// e.g. 29.97 -> (1001, 30000), 30 -> (1, 30), 23.976 -> (1001, 24000), 60 -> (1, 60).
fn rational_frame_duration(fps: f64) -> (u64, u64) {
    let f = if fps.is_finite() && fps > 0.5 { fps } else { 30.0 };
    let presets: [(f64, (u64, u64)); 7] = [
        (23.976, (1001, 24000)),
        (24.0, (1, 24)),
        (25.0, (1, 25)),
        (29.97, (1001, 30000)),
        (30.0, (1, 30)),
        (50.0, (1, 50)),
        (59.94, (1001, 60000)),
    ];
    for (target, rat) in presets {
        if (f - target).abs() < 0.03 {
            return rat;
        }
    }
    if (f - 60.0).abs() < 0.05 {
        return (1, 60);
    }
    let rounded = f.round().max(1.0) as u64;
    (1, rounded)
}

fn fps_int(fps: f64) -> u64 {
    if fps.is_finite() && fps > 0.5 {
        fps.round().max(1.0) as u64
    } else {
        30
    }
}

/// Format seconds as non-drop-frame SMPTE timecode `HH:MM:SS:FF`.
fn smpte_ndf(seconds: f64, fps: u64) -> String {
    let total_frames = (seconds.max(0.0) * fps as f64).round() as u64;
    let ff = total_frames % fps;
    let total_secs = total_frames / fps;
    let ss = total_secs % 60;
    let mm = (total_secs / 60) % 60;
    let hh = total_secs / 3600;
    format!("{hh:02}:{mm:02}:{ss:02}:{ff:02}")
}

/// Generate a CMX 3600 EDL representing the kept segments of `input`.
pub fn render_edl(input: &str, meta: &VideoMeta, keeps: &[Seg]) -> String {
    let fps = fps_int(meta.fps);
    let title = Path::new(input)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mustardy".to_string());
    let clip_name = Path::new(input)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string());
    let track = if meta.has_video && meta.has_audio {
        "AA/V"
    } else if meta.has_video {
        "V   "
    } else {
        "AA  "
    };

    let mut out = String::new();
    out.push_str(&format!("TITLE: {title}\n"));
    out.push_str("FCM: NON-DROP FRAME\n\n");

    let mut rec_cursor = 0.0f64;
    for (idx, seg) in keeps.iter().enumerate() {
        let dur = (seg.end - seg.start).max(0.0);
        if dur <= 0.0 {
            continue;
        }
        let num = idx + 1;
        let src_in = smpte_ndf(seg.start, fps);
        let src_out = smpte_ndf(seg.end, fps);
        let rec_in = smpte_ndf(rec_cursor, fps);
        let rec_out = smpte_ndf(rec_cursor + dur, fps);
        out.push_str(&format!(
            "{num:03}  AX       {track}  C        {src_in} {src_out} {rec_in} {rec_out}\n"
        ));
        out.push_str(&format!("* FROM CLIP NAME: {clip_name}\n\n"));
        rec_cursor += dur;
    }
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn file_url(path: &str) -> String {
    let abs = std::fs::canonicalize(Path::new(path)).unwrap_or_else(|_| Path::new(path).to_path_buf());
    let raw = abs.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(raw.len() + 16);
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b':' => encoded.push(b as char),
            _ => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// Convert seconds into frame-aligned FCPXML rational time string (e.g. `10010/30000s`).
fn fcpxml_time(seconds: f64, frame_num: u64, frame_den: u64) -> String {
    let fps = frame_den as f64 / frame_num as f64;
    let frames = (seconds.max(0.0) * fps).round() as u64;
    if frames == 0 {
        return "0s".to_string();
    }
    let num = frames * frame_num;
    if num % frame_den == 0 {
        format!("{}s", num / frame_den)
    } else {
        format!("{num}/{frame_den}s")
    }
}

/// Generate a Final Cut Pro XML v1.10 (`.fcpxml`) timeline for the kept segments.
pub fn render_fcpxml(input: &str, meta: &VideoMeta, keeps: &[Seg]) -> String {
    let (frame_num, frame_den) = rational_frame_duration(meta.fps);
    let width = if meta.width > 0 { meta.width } else { 1920 };
    let height = if meta.height > 0 { meta.height } else { 1080 };
    let sample_rate = meta.sample_rate.unwrap_or(48000);
    let channels = meta.channels.unwrap_or(2);
    let clip_name = Path::new(input)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mustardy".to_string());
    let src_url = file_url(input);
    let asset_dur = fcpxml_time(meta.duration, frame_num, frame_den);

    let total_kept: f64 = keeps.iter().map(|s| (s.end - s.start).max(0.0)).sum();
    let seq_dur = fcpxml_time(total_kept, frame_num, frame_den);

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<!DOCTYPE fcpxml>\n");
    out.push_str("<fcpxml version=\"1.10\">\n");
    out.push_str("  <resources>\n");
    out.push_str(&format!(
        "    <format id=\"r1\" name=\"FFVideoFormat{}p{}\" frameDuration=\"{frame_num}/{frame_den}s\" width=\"{width}\" height=\"{height}\"/>\n",
        height,
        fps_int(meta.fps)
    ));
    out.push_str(&format!(
        "    <asset id=\"r2\" name=\"{}\" start=\"0s\" duration=\"{asset_dur}\" hasVideo=\"{}\" format=\"r1\" hasAudio=\"{}\" audioSources=\"1\" audioChannels=\"{channels}\" audioRate=\"{sample_rate}\">\n",
        xml_escape(&clip_name),
        if meta.has_video { 1 } else { 0 },
        if meta.has_audio { 1 } else { 0 },
    ));
    out.push_str(&format!(
        "      <media-rep kind=\"original-media\" src=\"{}\"/>\n",
        xml_escape(&src_url)
    ));
    out.push_str("    </asset>\n");
    out.push_str("  </resources>\n");
    out.push_str("  <library>\n");
    out.push_str("    <event name=\"Mustardy\">\n");
    out.push_str(&format!(
        "      <project name=\"{} (trimmed)\">\n",
        xml_escape(&clip_name)
    ));
    out.push_str(&format!(
        "        <sequence format=\"r1\" duration=\"{seq_dur}\" tcStart=\"0s\" tcFormat=\"NDF\" audioLayout=\"{}\" audioRate=\"{}k\">\n",
        if channels == 1 { "mono" } else { "stereo" },
        sample_rate / 1000
    ));
    out.push_str("          <spine>\n");

    let mut offset_frames = 0u64;
    let fps = frame_den as f64 / frame_num as f64;
    for seg in keeps {
        let start_f = (seg.start.max(0.0) * fps).round() as u64;
        let end_f = (seg.end.max(0.0) * fps).round() as u64;
        if end_f <= start_f {
            continue;
        }
        let dur_f = end_f - start_f;
        let offset_t = frames_to_fcpxml(offset_frames, frame_num, frame_den);
        let start_t = frames_to_fcpxml(start_f, frame_num, frame_den);
        let dur_t = frames_to_fcpxml(dur_f, frame_num, frame_den);
        out.push_str(&format!(
            "            <asset-clip ref=\"r2\" offset=\"{offset_t}\" name=\"{}\" start=\"{start_t}\" duration=\"{dur_t}\"/>\n",
            xml_escape(&clip_name)
        ));
        offset_frames += dur_f;
    }

    out.push_str("          </spine>\n");
    out.push_str("        </sequence>\n");
    out.push_str("      </project>\n");
    out.push_str("    </event>\n");
    out.push_str("  </library>\n");
    out.push_str("</fcpxml>\n");
    out
}

fn frames_to_fcpxml(frames: u64, frame_num: u64, frame_den: u64) -> String {
    if frames == 0 {
        return "0s".to_string();
    }
    let num = frames * frame_num;
    if num % frame_den == 0 {
        format!("{}s", num / frame_den)
    } else {
        format!("{num}/{frame_den}s")
    }
}

pub fn write_edl(input: &str, meta: &VideoMeta, keeps: &[Seg], out_path: &Path) -> CoreResult<()> {
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(out_path, render_edl(input, meta, keeps))?;
    Ok(())
}

pub fn write_fcpxml(
    input: &str,
    meta: &VideoMeta,
    keeps: &[Seg],
    out_path: &Path,
) -> CoreResult<()> {
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(out_path, render_fcpxml(input, meta, keeps))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_meta() -> VideoMeta {
        VideoMeta {
            duration: 14.0,
            width: 1920,
            height: 1080,
            fps: 30.0,
            has_audio: true,
            has_video: true,
            start_time: 0.0,
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            sample_rate: Some(48000),
            channels: Some(2),
            name: "talk.mp4".into(),
            size: 1000,
        }
    }

    #[test]
    fn renders_valid_cmx3600_edl() {
        let keeps = vec![
            Seg { start: 0.0, end: 2.5 },
            Seg { start: 5.0, end: 10.0 },
        ];
        let edl = render_edl("talk.mp4", &sample_meta(), &keeps);
        assert!(edl.contains("TITLE: talk"));
        assert!(edl.contains("FCM: NON-DROP FRAME"));
        assert!(edl.contains("001  AX       AA/V  C        00:00:00:00 00:00:02:15 00:00:00:00 00:00:02:15"));
        assert!(edl.contains("002  AX       AA/V  C        00:00:05:00 00:00:10:00 00:00:02:15 00:00:07:15"));
        assert!(edl.contains("* FROM CLIP NAME: talk.mp4"));
    }

    #[test]
    fn renders_valid_fcpxml_1_10() {
        let keeps = vec![
            Seg { start: 0.0, end: 2.0 },
            Seg { start: 5.0, end: 9.0 },
        ];
        let xml = render_fcpxml("talk.mp4", &sample_meta(), &keeps);
        assert!(xml.contains("<!DOCTYPE fcpxml>"));
        assert!(xml.contains("<fcpxml version=\"1.10\">"));
        assert!(xml.contains("frameDuration=\"1/30s\""));
        assert!(xml.contains("offset=\"0s\""));
        assert!(xml.contains("offset=\"2s\""));
        assert!(xml.contains("start=\"5s\" duration=\"4s\""));
    }
}
