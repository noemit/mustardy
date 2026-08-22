//! Headless smoke test for the native engines.
//!
//!   MUSTARDY_FFMPEG=/path/ffmpeg MUSTARDY_FFPROBE=/path/ffprobe \
//!     cargo run --release --bin smoke -- all ../demo.mp4
//!
//! Subcommands: models | ffmpeg <video> | eyes <img.jpg>… | ears <video> | brain "<prompt>" | brain-refine <video> | all <video>

use mustardy_core::{brain, ears, eyes, ffmpeg, models};

const GEMMA_PROMPT: &str = "You are Mustardy, a punchy video editor.
Pick tools. Return ONLY JSON: {\"changes\":[{\"type\":\"cut\",\"start\":0,\"end\":1,\"label\":\"\",\"rationale\":\"\",\"text\":\"\",\"style\":\"youtube\",\"pan\":{\"kind\":\"zoom-in\"},\"rate\":0.5}]}

Tools: cut, overlay, pan, punch, slow, fast, shake, freeze, flash, textcard, tilt, impact, bounce, glitch, spotlight.

Rules:
- Times are source seconds. Stay inside the video.
- 3-8 changes. Stronger > more.
- text / textcard / impact need short ALL-CAPS copy, max 6 words.";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("models");
    let t0 = std::time::Instant::now();
    let res = match cmd {
        "models" => cmd_models(),
        "ffmpeg" => cmd_ffmpeg(args.get(2).expect("video path")),
        "eyes" => cmd_eyes(&args[2..]),
        "ears" => cmd_ears(args.get(2).expect("video path")),
        "brain" => cmd_brain(args.get(2).map(String::as_str).unwrap_or("Cut the silences and add a title.")),
        "brain-refine" => cmd_brain_refine(args.get(2).expect("video path")),
        "backend-race" => cmd_backend_race(),
        "brain-debug" => cmd_brain_debug(),
        "all" => cmd_all(args.get(2).expect("video path")),
        other => Err(format!("unknown subcommand {other}")),
    };
    match res {
        Ok(()) => eprintln!("\n[smoke] ok in {:.1?}", t0.elapsed()),
        Err(e) => {
            eprintln!("\n[smoke] FAILED: {e}");
            std::process::exit(1);
        }
    }
}

type R = Result<(), String>;

fn cmd_models() -> R {
    for s in models::status() {
        println!(
            "{:12} {:44} {}",
            s.id,
            s.file,
            if s.present { format!("present ({} MB)", s.bytes / 1_000_000) } else { "MISSING".into() }
        );
    }
    Ok(())
}

fn cmd_ffmpeg(video: &str) -> R {
    let meta = ffmpeg::probe(video).map_err(|e| e.to_string())?;
    println!(
        "probe: {:.2}s {}x{} @ {:.1}fps audio={} size={}MB",
        meta.duration,
        meta.width,
        meta.height,
        meta.fps,
        meta.has_audio,
        meta.size / 1_000_000
    );
    let sil = ffmpeg::detect_silence(video, None, None).map_err(|e| e.to_string())?;
    println!("silences: {:?}", sil.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>());
    Ok(())
}

fn cmd_eyes(images: &[String]) -> R {
    let frames: Vec<(f64, Vec<u8>)> = images
        .iter()
        .enumerate()
        .map(|(i, p)| std::fs::read(p).map(|b| (i as f64, b)))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let t = std::time::Instant::now();
    let caps = eyes::caption_frames(&frames, 32);
    for c in &caps {
        println!("{:>5.1}s  {}", c.t, c.text);
    }
    eprintln!("[eyes] {} captions in {:.1?}", caps.len(), t.elapsed());
    if caps.is_empty() {
        return Err("no captions produced".into());
    }
    Ok(())
}

fn cmd_ears_expect(video: &str, expect_words: bool) -> R {
    let t = std::time::Instant::now();
    let pcm = ffmpeg::extract_pcm_16k(video).map_err(|e| e.to_string())?;
    eprintln!("[ears] pcm: {} samples ({:.1}s) in {:.1?}", pcm.len(), pcm.len() as f32 / 16000.0, t.elapsed());
    let t = std::time::Instant::now();
    let tr = ears::transcribe(&pcm, None).map_err(|e| e.to_string())?;
    println!("text: {}", tr.text);
    println!("words ({}):", tr.words.len());
    for w in tr.words.iter().take(24) {
        println!("  {:>6.2}-{:>6.2}  {}", w.t, w.end, w.text);
    }
    eprintln!("[ears] transcribed in {:.1?}", t.elapsed());
    if expect_words && tr.words.is_empty() {
        return Err("no words produced".into());
    }
    Ok(())
}

fn cmd_ears(video: &str) -> R {
    cmd_ears_expect(video, true)
}

fn cmd_brain(prompt: &str) -> R {
    let t = std::time::Instant::now();
    let out = brain::plan(
        GEMMA_PROMPT,
        &format!(
            "Video 42.0s 1280x720.\nSilences: [[3.0, 4.5], [10.2, 11.8]]\nFrames: 1s a person typing | 20s a cat on the desk | 39s a wave\nTranscript: um so yeah this is the demo\nUser: {prompt}"
        ),
        420,
        3,
        None,
    )
    .map_err(|e| e.to_string())?;
    match out {
        Some(json) => {
            println!("brain plan:\n{json}");
            eprintln!("[brain] picked tools in {:.1?}", t.elapsed());
            Ok(())
        }
        None => Err("brain picked no tool after retries".into()),
    }
}

fn cmd_brain_refine(video: &str) -> R {
    let meta = ffmpeg::probe(video).map_err(|e| e.to_string())?;
    let d = meta.duration;
    // Pretend the brain proposed a deliberately imprecise cut; refinement
    // (empty silence list, so nothing is exempt) should snap its boundaries.
    let t = std::time::Instant::now();
    let out = brain::plan(
        GEMMA_PROMPT,
        &format!(
            "Video {d:.1}s {}x{}.\nSilences: []\nFrames: (none yet)\nTranscript: none\nUser: Cut the boring part around the middle and add a title.",
            meta.width, meta.height
        ),
        420,
        3,
        Some(&brain::RefineCtx {
            path: video.to_string(),
            silences: vec![],
        }),
    )
    .map_err(|e| e.to_string())?;
    match out {
        Some(json) => {
            println!("refined plan:\n{json}");
            eprintln!("[brain+refine] done in {:.1?}", t.elapsed());
            Ok(())
        }
        None => Err("brain picked no tool after retries".into()),
    }
}

/// Regression test: the app warms eyes and brain on parallel threads, and
/// llama.cpp allows only one backend init per process. Hammer backend() from
/// 8 threads; all must succeed and agree on the same backend.
fn cmd_backend_race() -> R {
    let mut handles = Vec::new();
    for _ in 0..8 {
        handles.push(std::thread::spawn(|| {
            mustardy_core::llm::backend()
                .map(|b| b as *const _ as usize)
                .map_err(|e| e.to_string())
        }));
    }
    let mut ptrs = std::collections::HashSet::new();
    for h in handles {
        let p = h.join().map_err(|_| "thread panicked".to_string())??;
        ptrs.insert(p);
    }
    if ptrs.len() != 1 {
        return Err(format!("{} distinct backends", ptrs.len()));
    }
    println!("8 threads → one backend");
    Ok(())
}

fn cmd_brain_debug() -> R {    let msgs = vec![
        ("system".to_string(), GEMMA_PROMPT.to_string()),
        ("user".to_string(), "Video 42.0s 1280x720.\nSilences: [[3.0, 4.5], [10.2, 11.8]]\nFrames: 1s a person typing | 20s a cat on the desk | 39s a wave\nTranscript: um so yeah this is the demo\nUser: Cut the silences, zoom in on the cat, and put a title card at the start.".to_string()),
    ];
    let (rendered, out) = brain::debug_chat(&msgs, 120).map_err(|e| e.to_string())?;
    println!("=== rendered prompt ===\n{rendered}\n=== end ===\n=== output ===\n{out}\n=== end ===");
    Ok(())
}

fn cmd_all(video: &str) -> R {
    cmd_ffmpeg(video)?;
    // frames at 1/6, 3/6, 5/6 of the video
    let meta = ffmpeg::probe(video).map_err(|e| e.to_string())?;
    let times: Vec<f64> = [0.17, 0.5, 0.83].iter().map(|f| f * meta.duration).collect();
    let frames = ffmpeg::extract_frames(video, &times, 384).map_err(|e| e.to_string())?;
    eprintln!("[all] extracted {} frames", frames.len());
    let tmp = std::env::temp_dir().join("mustardy-smoke");
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let paths: Vec<String> = frames
        .iter()
        .enumerate()
        .map(|(i, (_, bytes))| {
            let p = tmp.join(format!("frame-{i}.jpg"));
            std::fs::write(&p, bytes).map(|_| p.to_string_lossy().into_owned())
        })
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    cmd_eyes(&paths)?;
    cmd_ears_expect(video, false)?; // demo clip may legitimately be silent
    cmd_brain("Cut the silences, zoom on the cat, smash a title on the wave.")?;
    Ok(())
}
