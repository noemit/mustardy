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

/// Drop the loaded model + mmproj to free memory (reloads on next use).
pub fn unload() {
    if let Ok(mut guard) = cell().lock() {
        *guard = None;
    }
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
    let mut caps = caption_frames(&[(0.0, image.to_vec())], max_tokens);
    caps.pop()
        .map(|c| c.text)
        .ok_or_else(|| CoreError::msg("caption failed"))
}

/// Caption a batch of frames. The mtmd projector context is initialized ONCE
/// for the whole batch — `init_from_file` re-reads and re-weights the mmproj
/// on every call, which used to dominate refine time frame by frame.
pub fn caption_frames(frames: &[(f64, Vec<u8>)], max_tokens: usize) -> Vec<Caption> {
    let mut out = Vec::new();
    if frames.is_empty() || load().is_err() {
        return out;
    }
    let Ok(guard) = cell().lock() else { return out };
    let Some(eyes) = guard.as_ref() else { return out };

    let Ok(media_marker) = CString::new(mtmd_default_marker()) else {
        return out;
    };
    let params = MtmdContextParams {
        use_gpu: true,
        print_timings: false,
        n_threads: llm::n_threads(),
        media_marker,
        image_min_tokens: -1,
        image_max_tokens: -1,
    };
    let Ok(mtmd) = MtmdContext::init_from_file(&eyes.mmproj_path, &eyes.model, &params) else {
        return out;
    };
    if !mtmd.support_vision() {
        return out;
    }
    let Ok(prompt) = llm::render_chat(
        &eyes.model,
        &[("user".to_string(), format!("{}\n{}", mtmd_default_marker(), CAPTION_PROMPT))],
    ) else {
        return out;
    };

    for (t, bytes) in frames {
        match caption_with(&mtmd, &eyes.model, &prompt, bytes, max_tokens) {
            Ok(text) if !text.is_empty() => out.push(Caption { t: *t, text }),
            _ => {}
        }
    }
    out
}

/// One caption against an already-initialized mtmd context.
fn caption_with(
    mtmd: &MtmdContext,
    model: &LlamaModel,
    prompt: &str,
    image: &[u8],
    max_tokens: usize,
) -> CoreResult<String> {
    let bitmap =
        MtmdBitmap::from_buffer(mtmd, image, false).map_err(|e| CoreError::msg(format!("decode image: {e}")))?;
    let chunks = mtmd
        .tokenize(
            MtmdInputText {
                text: prompt.to_string(),
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )
        .map_err(|e| CoreError::msg(format!("mtmd tokenize: {e}")))?;
    let mut ctx = llm::new_ctx(model, 2048, llm::n_threads())?;
    let n_past = chunks
        .eval_chunks(mtmd, &ctx, 0, 0, 512, true)
        .map_err(|e| CoreError::msg(format!("mtmd eval: {e}")))?;
    let mut sampler = llm::brain_sampler();
    let text = llm::generate(model, &mut ctx, n_past, max_tokens, &mut sampler)?;
    Ok(text.trim().to_string())
}
