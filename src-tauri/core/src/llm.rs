//! Shared llama.cpp plumbing for the eyes (SmolVLM) and brain (SmolLM2).

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::{send_logs_to_tracing, LogOptions};

use crate::{CoreError, CoreResult};

pub fn backend() -> CoreResult<&'static LlamaBackend> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    static INIT_LOCK: Mutex<()> = Mutex::new(());
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    // Eyes and brain warm up on parallel threads; llama.cpp allows exactly one
    // backend init per process, so serialize the initialization.
    let _g = INIT_LOCK
        .lock()
        .map_err(|_| CoreError::msg("backend lock poisoned"))?;
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let _ = send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
    let b = LlamaBackend::init().map_err(|e| CoreError::msg(format!("llama backend init: {e}")))?;
    Ok(BACKEND.get_or_init(|| b))
}

pub fn load_model(path: &Path) -> CoreResult<LlamaModel> {
    let params = LlamaModelParams::default().with_n_gpu_layers(1000);
    LlamaModel::load_from_file(backend()?, path, &params)
        .map_err(|e| CoreError::msg(format!("load {}: {e}", path.display())))
}

pub fn new_ctx(model: &LlamaModel, n_ctx: u32, threads: i32) -> CoreResult<llama_cpp_2::context::LlamaContext<'_>> {
    let params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        // chat_once_with decodes the whole prompt as ONE batch; llama.cpp's
        // default n_batch is 2048 and it hard-aborts (GGML_ASSERT → SIGABRT)
        // when the prompt batch exceeds it. Size the batch to the context so
        // any prompt that passes the length guard can always be decoded.
        .with_n_batch(n_ctx)
        .with_n_threads(threads)
        .with_n_threads_batch(threads);
    model
        .new_context(backend()?, params)
        .map_err(|e| CoreError::msg(format!("llama context: {e}")))
}

pub fn n_threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4)
}

/// Render a chat conversation with the model's built-in template.
pub fn render_chat(model: &LlamaModel, messages: &[(String, String)]) -> CoreResult<String> {
    let tmpl = model
        .chat_template(None)
        .map_err(|e| CoreError::msg(format!("chat template: {e}")))?;
    let msgs: Vec<LlamaChatMessage> = messages
        .iter()
        .map(|(role, content)| LlamaChatMessage::new(role.clone(), content.clone()))
        .collect::<Result<_, _>>()
        .map_err(|e| CoreError::msg(format!("chat message: {e}")))?;
    model
        .apply_chat_template(&tmpl, &msgs, true)
        .map_err(|e| CoreError::msg(format!("apply chat template: {e}")))
}

/// Sample tokens from the position after the prompt until EOG / max_tokens.
///
/// `n_past` must be the number of tokens already decoded in `ctx`, with logits
/// available at the last position.
pub fn generate(
    model: &LlamaModel,
    ctx: &mut llama_cpp_2::context::LlamaContext,
    n_past_start: i32,
    max_tokens: usize,
    sampler: &mut LlamaSampler,
) -> CoreResult<String> {
    let mut batch = LlamaBatch::new(1, 1);
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut out = String::new();
    let mut n_cur = n_past_start;
    for _ in 0..max_tokens {
        let token = sampler.sample(ctx, -1);
        sampler.accept(token);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, true, None)
            .map_err(|e| CoreError::msg(format!("detokenize: {e}")))?;
        out.push_str(&piece);
        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| CoreError::msg(format!("batch add: {e}")))?;
        n_cur += 1;
        ctx.decode(&mut batch)
            .map_err(|e| CoreError::msg(format!("decode: {e}")))?;
    }
    Ok(out)
}

/// Default sampler for the planning brain: light sampling, mostly deterministic.
pub fn brain_sampler() -> LlamaSampler {
    if std::env::var("BRAIN_SAMPLE").is_ok() {
        return LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::temp(0.25),
            LlamaSampler::dist(42),
        ]);
    }
    LlamaSampler::chain_simple([
        LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
        LlamaSampler::greedy(),
    ])
}

/// Decode a whole prompt (already rendered) into a fresh context and generate.
pub fn chat_once(
    model: &LlamaModel,
    messages: &[(String, String)],
    n_ctx: u32,
    max_tokens: usize,
) -> CoreResult<String> {
    chat_once_with(model, messages, n_ctx, max_tokens, brain_sampler())
}

/// Same as [`chat_once`] but with a caller-provided sampler.
pub fn chat_once_with(
    model: &LlamaModel,
    messages: &[(String, String)],
    n_ctx: u32,
    max_tokens: usize,
    mut sampler: LlamaSampler,
) -> CoreResult<String> {
    let prompt = render_chat(model, messages)?;
    let mut ctx = new_ctx(model, n_ctx, n_threads())?;
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| CoreError::msg(format!("tokenize: {e}")))?;
    // Leave real room for the response: reserving only a fixed 64-token
    // sliver let long prompts + max_tokens run past n_ctx mid-generation,
    // which aborts decode and burns an attempt.
    let gen_reserve = (max_tokens as u32).min(n_ctx) + 16;
    if tokens.len() as u32 >= n_ctx.saturating_sub(gen_reserve) {
        return Err(CoreError::msg("prompt too long for context"));
    }
    crate::log::line(&format!("brain prompt: {} token(s), ctx {n_ctx}", tokens.len()));
    let mut batch = LlamaBatch::new(tokens.len().max(512), 1);
    let last = tokens.len() - 1;
    for (i, token) in tokens.iter().enumerate() {
        batch
            .add(*token, i as i32, &[0], i == last)
            .map_err(|e| CoreError::msg(format!("batch add: {e}")))?;
    }
    ctx.decode(&mut batch)
        .map_err(|e| CoreError::msg(format!("decode prompt: {e}")))?;
    generate(model, &mut ctx, tokens.len() as i32, max_tokens, &mut sampler)
}
