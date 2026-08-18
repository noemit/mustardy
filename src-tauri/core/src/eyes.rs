//! Eyes: frame captioning with SmolVLM (GGUF) through llama.cpp's mtmd API.

use std::ffi::CString;
use std::sync::{Mutex, OnceLock};

use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText, mtmd_default_marker};

use crate::llm;
use crate::models::{self, EYES_MMPROJ, EYES_MODEL};
use crate::{Caption, CoreError, CoreResult};

const CAPTION_PROMPT: &str =
    "Describe this video frame in one short sentence. Focus on the action and any visible text.";

struct Eyes {
    model: LlamaModel,
    mmproj_path: String,
}

static EYES: OnceLock<Mutex<Option<Eyes>>> = OnceLock::new();

fn cell() -> &'static Mutex<Option<Eyes>> {
    EYES.get_or_init(|| Mutex::new(None))
}

pub fn loaded() -> bool {
    cell().lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn load() -> CoreResult<()> {
    let mut guard = cell().lock().map_err(|_| CoreError::msg("eyes lock poisoned"))?;
    if guard.is_some() {
        return Ok(());
    }
    let model_path = models::find(&EYES_MODEL)
        .ok_or_else(|| CoreError::msg(format!("missing {} — run the model download", EYES_MODEL.file)))?;
    let mmproj_path = models::find(&EYES_MMPROJ)
        .ok_or_else(|| CoreError::msg(format!("missing {} — run the model download", EYES_MMPROJ.file)))?;
    let model = llm::load_model(&model_path)?;
    *guard = Some(Eyes {
        model,
        mmproj_path: mmproj_path.to_string_lossy().into_owned(),
    });
    crate::log::line("eyes loaded (SmolVLM)");
    Ok(())
}

/// Caption one frame. `image` holds encoded JPEG/PNG bytes.
pub fn caption_one(image: &[u8], max_tokens: usize) -> CoreResult<String> {
    load()?;
    let guard = cell().lock().map_err(|_| CoreError::msg("eyes lock poisoned"))?;
    let eyes = guard.as_ref().ok_or_else(|| CoreError::msg("eyes not loaded"))?;

    let params = MtmdContextParams {
        use_gpu: true,
        print_timings: false,
        n_threads: llm::n_threads(),
        media_marker: CString::new(mtmd_default_marker()).map_err(|e| CoreError::msg(e.to_string()))?,
        image_min_tokens: -1,
        image_max_tokens: -1,
    };
    let mtmd = MtmdContext::init_from_file(&eyes.mmproj_path, &eyes.model, &params)
        .map_err(|e| CoreError::msg(format!("mtmd init: {e}")))?;
    if !mtmd.support_vision() {
        return Err(CoreError::msg("mmproj has no vision support"));
    }

    let bitmap = MtmdBitmap::from_buffer(&mtmd, image, false)
        .map_err(|e| CoreError::msg(format!("decode image: {e}")))?;

    let prompt = llm::render_chat(
        &eyes.model,
        &[("user".to_string(), format!("{}\n{}", mtmd_default_marker(), CAPTION_PROMPT))],
    )?;

    let chunks = mtmd
        .tokenize(
            MtmdInputText {
                text: prompt,
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )
        .map_err(|e| CoreError::msg(format!("mtmd tokenize: {e}")))?;

    let mut ctx = llm::new_ctx(&eyes.model, 2048, llm::n_threads())?;
    let n_past = chunks
        .eval_chunks(&mtmd, &ctx, 0, 0, 512, true)
        .map_err(|e| CoreError::msg(format!("mtmd eval: {e}")))?;

    let mut sampler = llm::brain_sampler();
    let text = llm::generate(&eyes.model, &mut ctx, n_past, max_tokens, &mut sampler)?;
    Ok(text.trim().to_string())
}

pub fn caption_frames(frames: &[(f64, Vec<u8>)], max_tokens: usize) -> Vec<Caption> {
    let mut out = Vec::new();
    for (t, bytes) in frames {
        match caption_one(bytes, max_tokens) {
            Ok(text) if !text.is_empty() => out.push(Caption { t: *t, text }),
            _ => {}
        }
    }
    out
}
