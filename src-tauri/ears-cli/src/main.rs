//! mustardy-ears <model.ggml> <pcm.f32le>
//!
//! Reads 16 kHz mono f32le PCM from a file, transcribes with whisper.cpp,
//! writes {"text": "...", "words": [{"t", "end", "text"}...]} to stdout.
//!
//! Runs as a separate process on purpose: whisper.cpp and llama.cpp both
//! vendor ggml, and linking both into one binary collides their symbols.

use serde::Serialize;
use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters,
};

#[derive(Serialize)]
struct Word {
    t: f64,
    end: f64,
    text: String,
}

#[derive(Serialize)]
struct Transcript {
    text: String,
    words: Vec<Word>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (model, pcm_path) = (args.get(1), args.get(2));
    let (Some(model), Some(pcm_path)) = (model, pcm_path) else {
        eprintln!("usage: mustardy-ears <model.ggml> <pcm.f32le>");
        std::process::exit(2);
    };
    match run(model, pcm_path) {
        Ok(t) => {
            println!("{}", serde_json::to_string(&t).expect("json"));
        }
        Err(e) => {
            eprintln!("mustardy-ears: {e}");
            std::process::exit(1);
        }
    }
}

fn dtw_preset(model: &str) -> DtwModelPreset {
    let n = model.to_ascii_lowercase();
    if n.contains("small") {
        DtwModelPreset::SmallEn
    } else if n.contains("base") {
        DtwModelPreset::BaseEn
    } else {
        DtwModelPreset::TinyEn
    }
}

fn run(model: &str, pcm_path: &str) -> Result<Transcript, String> {
    let bytes = std::fs::read(pcm_path).map_err(|e| format!("read {pcm_path}: {e}"))?;
    let mut pcm = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        pcm.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.dtw_parameters(DtwParameters {
        mode: DtwMode::ModelPreset {
            model_preset: dtw_preset(model),
        },
        ..Default::default()
    });
    let ctx = WhisperContext::new_with_params(model, ctx_params).map_err(|e| format!("load model: {e}"))?;
    let mut state = ctx.create_state().map_err(|e| format!("state: {e}"))?;

    let threads = std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4);
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_n_threads(threads);
    params.set_token_timestamps(true);
    params.set_split_on_word(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state.full(params, &pcm).map_err(|e| format!("decode: {e}"))?;

    let mut text = String::new();
    let mut words: Vec<Word> = Vec::new();
    for seg in state.as_iter() {
        text.push_str(&seg.to_str_lossy().unwrap_or_default());
        let mut cur: Option<Word> = None;
        for i in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(i) else { continue };
            let piece = tok.to_str_lossy().unwrap_or_default().into_owned();
            if piece.starts_with('[') || piece.starts_with("<|") {
                continue;
            }
            let data = tok.token_data();
            let (t0, t1) = (data.t0 as f64 / 100.0, data.t1 as f64 / 100.0);
            if piece.starts_with(' ') || piece.starts_with('\'') || cur.is_none() {
                if let Some(w) = cur.take() {
                    words.push(w);
                }
                cur = Some(Word {
                    t: t0,
                    end: t1.max(t0),
                    text: piece.trim_start().to_string(),
                });
            } else if let Some(w) = cur.as_mut() {
                w.text.push_str(&piece);
                w.end = t1.max(w.end);
            }
        }
        if let Some(w) = cur.take() {
            words.push(w);
        }
    }

    Ok(Transcript {
        text: text.trim().to_string(),
        words: words.into_iter().filter(|w| !w.text.is_empty()).collect(),
    })
}
