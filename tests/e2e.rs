use std::path::{Path, PathBuf};
use std::process::Command;

use mustardy_core::ffmpeg::{self, resolve_bin};
use mustardy_core::silence::{self, SilenceCutOptions};
use mustardy_core::visual;
use mustardy_core::{Change, ExportOptions, ExportPayload};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("repo root")
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-scratch")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mustardy")
}

#[test]
fn e2e_demo_mp4_single_pass_trim_and_verify() {
    let demo = repo_root().join("demo.mp4");
    if !demo.is_file() {
        eprintln!("skipping e2e_demo_mp4: demo.mp4 not present");
        return;
    }
    let dir = scratch_dir("demo-trim");
    let out = dir.join("demo-trimmed.mp4");

    let meta = ffmpeg::probe(&demo.to_string_lossy()).expect("probe demo.mp4");
    assert!(meta.has_video);
    assert!(meta.has_audio);
    assert!((meta.duration - 14.0).abs() < 0.5);

    let env = ffmpeg::audio_envelope(&demo.to_string_lossy()).expect("envelope");
    assert!(
        env.dbs.len() >= 680 && env.dbs.len() <= 720,
        "expected ~700 windows, got {}",
        env.dbs.len()
    );

    let analysis = silence::analyze_envelope(&env, 30.0, 0.9);
    assert_eq!(analysis.ranges.len(), 2);
    assert!(!analysis.stats.low_contrast);

    let vres = visual::visual_change_times_parallel(
        &demo.to_string_lossy(),
        &analysis.ranges,
        2,
        visual::DEFAULT_MAX_RANGES,
        |_, _| {},
    )
    .expect("visual scan");
    assert_eq!(vres.times.len(), 2);

    let cuts = silence::silence_cuts_with_options(
        &analysis.ranges,
        &vres.times,
        &SilenceCutOptions {
            duration: Some(meta.duration),
            ..Default::default()
        },
    );
    let keeps = ffmpeg::kept_segments(meta.duration, &cuts);
    let expected: f64 = keeps.iter().map(|s| s.end - s.start).sum();

    let payload = ExportPayload {
        input: demo.to_string_lossy().into_owned(),
        duration: meta.duration,
        changes: cuts,
        output: out.to_string_lossy().into_owned(),
        normalize: true,
        normalize_amount: 0.7,
    };

    let mut progress_seen = Vec::new();
    ffmpeg::export_project_with_options(&payload, &ExportOptions::default(), |p| {
        progress_seen.push(p.progress);
    })
    .expect("export demo.mp4");

    assert!(out.is_file(), "output file missing");
    assert_eq!(progress_seen.last().copied(), Some(100));

    let out_meta = ffmpeg::probe(&out.to_string_lossy()).expect("probe output");
    assert!(out_meta.has_video);
    assert!(out_meta.has_audio);
    assert!(
        (out_meta.duration - expected).abs() < 0.3,
        "expected ~{expected:.2}s, got {:.2}s",
        out_meta.duration
    );

    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".mustardy-tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "leaked temp dirs: {leftovers:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn e2e_audio_only_wav_trim() {
    let dir = scratch_dir("audio-only");
    let src = dir.join("podcast.wav");
    let out = dir.join("podcast-trimmed.wav");

    let status = Command::new(resolve_bin("ffmpeg"))
        .args([
            "-y",
            "-hide_banner",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1.5:sample_rate=16000",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=16000:cl=mono:d=2.0",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=550:duration=1.5:sample_rate=16000",
            "-filter_complex",
            "[0:a][1:a][2:a]concat=n=3:v=0:a=1[a]",
            "-map",
            "[a]",
            &src.to_string_lossy(),
        ])
        .status()
        .expect("generate wav");
    assert!(status.success());

    let meta = ffmpeg::probe(&src.to_string_lossy()).expect("probe wav");
    assert!(!meta.has_video);
    assert!(meta.has_audio);
    assert!((meta.duration - 5.0).abs() < 0.2);

    let env = ffmpeg::audio_envelope(&src.to_string_lossy()).expect("wav envelope");
    let ranges = silence::silences_from_envelope(&env, 24.0, 0.5);
    assert_eq!(ranges.len(), 1);

    let cuts = silence::silence_cuts_with_options(
        &ranges,
        &[],
        &SilenceCutOptions {
            duration: Some(meta.duration),
            ..Default::default()
        },
    );

    let payload = ExportPayload {
        input: src.to_string_lossy().into_owned(),
        duration: meta.duration,
        changes: cuts,
        output: out.to_string_lossy().into_owned(),
        normalize: true,
        normalize_amount: 0.7,
    };
    ffmpeg::export_project(&payload, |_| {}).expect("export audio-only wav");

    let out_meta = ffmpeg::probe(&out.to_string_lossy()).expect("probe out wav");
    assert!(!out_meta.has_video);
    assert!(out_meta.has_audio);
    assert!(
        out_meta.duration > 2.8 && out_meta.duration < 3.8,
        "expected ~3.36s trimmed wav, got {:.2}s",
        out_meta.duration
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn e2e_chunked_parallel_export_over_50_segments() {
    let dir = scratch_dir("chunked-55");
    let src = dir.join("long.mp4");
    let out = dir.join("long-trimmed.mp4");

    let status = Command::new(resolve_bin("ffmpeg"))
        .args([
            "-y",
            "-hide_banner",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=128x72:d=26:r=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=26:sample_rate=48000",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            &src.to_string_lossy(),
        ])
        .status()
        .expect("generate chunked test mp4");
    assert!(status.success());

    let mut cuts = Vec::new();
    for i in 0..52 {
        let start = (i as f64) * 0.48 + 0.25;
        let end = start + 0.20;
        cuts.push(Change {
            id: format!("c-{i}"),
            kind: "cut".into(),
            start,
            end,
            status: "accepted".into(),
            label: "cut".into(),
            rationale: "".into(),
            origin: Some("silence".into()),
            text: None,
            style: None,
            pan: None,
            rate: None,
        });
    }

    let keeps = ffmpeg::kept_segments(26.0, &cuts);
    assert!(keeps.len() > 48, "expected >48 keeps, got {}", keeps.len());
    let expected: f64 = keeps.iter().map(|s| s.end - s.start).sum();

    let payload = ExportPayload {
        input: src.to_string_lossy().into_owned(),
        duration: 26.0,
        changes: cuts,
        output: out.to_string_lossy().into_owned(),
        normalize: true,
        normalize_amount: 0.7,
    };
    ffmpeg::export_project_with_options(
        &payload,
        &ExportOptions {
            preset: "ultrafast".into(),
            jobs: 2,
            ..Default::default()
        },
        |_| {},
    )
    .expect("chunked parallel export");

    let out_meta = ffmpeg::probe(&out.to_string_lossy()).expect("probe chunked output");
    assert!(out_meta.has_video && out_meta.has_audio);
    assert!(
        (out_meta.duration - expected).abs() < 1.0,
        "expected ~{expected:.2}s, got {:.2}s",
        out_meta.duration
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn e2e_cli_scan_apply_nle_and_doctor() {
    let demo = repo_root().join("demo.mp4");
    if !demo.is_file() {
        return;
    }
    let dir = scratch_dir("cli-e2e");
    let plan = dir.join("plan.json");
    let fcpxml = dir.join("demo.fcpxml");
    let edl = dir.join("demo.edl");
    let out_dir = dir.join("batch");

    // 1. doctor --json
    let doc = Command::new(cli_bin())
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor");
    assert!(doc.status.success());
    let doc_json: serde_json::Value = serde_json::from_slice(&doc.stdout).expect("doctor json");
    assert_eq!(doc_json["ok"], true);

    // 2. scan -> plan.json
    let scan = Command::new(cli_bin())
        .args([
            "scan",
            &demo.to_string_lossy(),
            "-o",
            &plan.to_string_lossy(),
            "--json",
            "-q",
        ])
        .output()
        .expect("run scan");
    assert!(scan.status.success());
    let scan_json: serde_json::Value = serde_json::from_slice(&scan.stdout).expect("scan json");
    assert_eq!(scan_json["schemaVersion"], 1);
    assert_eq!(scan_json["cuts"].as_array().unwrap().len(), 2);
    assert_eq!(scan_json["keeps"].as_array().unwrap().len(), 3);

    // 3. apply plan -> FCPXML (zero re-encode, auto-detected from .fcpxml extension)
    let apply_xml = Command::new(cli_bin())
        .args([
            "apply",
            &plan.to_string_lossy(),
            "-o",
            &fcpxml.to_string_lossy(),
            "--json",
            "-q",
        ])
        .output()
        .expect("apply fcpxml");
    assert!(apply_xml.status.success());
    let xml_body = std::fs::read_to_string(&fcpxml).expect("read fcpxml");
    assert!(xml_body.contains("<fcpxml version=\"1.10\">"));
    assert_eq!(xml_body.matches("<asset-clip ").count(), 3);

    // 4. apply plan -> EDL (auto-detected from .edl extension)
    let apply_edl = Command::new(cli_bin())
        .args([
            "apply",
            &plan.to_string_lossy(),
            "-o",
            &edl.to_string_lossy(),
            "--json",
            "-q",
        ])
        .output()
        .expect("apply edl");
    assert!(apply_edl.status.success());
    let edl_body = std::fs::read_to_string(&edl).expect("read edl");
    assert!(edl_body.contains("FCM: NON-DROP FRAME"));
    assert!(edl_body.contains("003  AX"));

    // 5. trim --out-dir + --skip-existing (using --format edl for instant batch check)
    let first = Command::new(cli_bin())
        .args([
            "trim",
            &demo.to_string_lossy(),
            "--out-dir",
            &out_dir.to_string_lossy(),
            "--format",
            "edl",
            "--no-visual",
            "-q",
        ])
        .output()
        .expect("batch first");
    assert!(first.status.success());
    assert!(out_dir.join("demo-trimmed.edl").is_file());

    let second = Command::new(cli_bin())
        .args([
            "trim",
            &demo.to_string_lossy(),
            "--out-dir",
            &out_dir.to_string_lossy(),
            "--format",
            "edl",
            "--skip-existing",
            "--no-visual",
            "--json",
            "-q",
        ])
        .output()
        .expect("batch skip");
    assert!(second.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}
