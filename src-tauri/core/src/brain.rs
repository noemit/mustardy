//! Brain: edit planning with SmolLM2 (GGUF) through llama.cpp.
//!
//! The brain's only job is to *choose tools*: it must emit a JSON object with
//! a non-empty "changes" array. We retry — first greedy, then sampled at
//! rising temperature — until it does or attempts run out.

use std::sync::{Mutex, OnceLock};

use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;

use crate::llm;
use crate::models::{self, BRAIN_MODEL};
use crate::{ffmpeg, CoreError, CoreResult};

static BRAIN: OnceLock<Mutex<Option<LlamaModel>>> = OnceLock::new();

fn cell() -> &'static Mutex<Option<LlamaModel>> {
    BRAIN.get_or_init(|| Mutex::new(None))
}

pub fn loaded() -> bool {
    cell().lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn load() -> CoreResult<()> {
    let mut guard = cell().lock().map_err(|_| CoreError::msg("brain lock poisoned"))?;
    if guard.is_some() {
        return Ok(());
    }
    // Optional override: absolute path or a filename inside a models root.
    if let Ok(custom) = std::env::var("MUSTARDY_BRAIN") {
        let p = models::resolve_name(&custom)
            .ok_or_else(|| CoreError::msg(format!("MUSTARDY_BRAIN not found: {custom}")))?;
        *guard = Some(llm::load_model(&p)?);
        return Ok(());
    }
    let path = models::find(&BRAIN_MODEL)
        .ok_or_else(|| CoreError::msg(format!("missing {} — run the model download", BRAIN_MODEL.file)))?;
    *guard = Some(llm::load_model(&path)?);
    crate::log::line("brain loaded (SmolLM2)");
    Ok(())
}

fn sampler_for(attempt: usize) -> LlamaSampler {
    if attempt == 0 {
        // Deterministic first try.
        return LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            LlamaSampler::greedy(),
        ]);
    }
    // Later attempts: sample with rising temperature to escape deterministic failure.
    let temp = 0.5 + 0.15 * attempt as f32;
    LlamaSampler::chain_simple([
        LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
        LlamaSampler::top_k(40),
        LlamaSampler::top_p(0.95, 1),
        LlamaSampler::temp(temp),
        LlamaSampler::dist(attempt as u32 * 7919 + 1),
    ])
}

/// Strip ``` fences and extract the outermost JSON object.
fn extract_json(raw: &str) -> Option<&str> {
    let body = if let Some(m) = raw.find("```") {
        let after = &raw[m + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        match after.find("```") {
            Some(end) => &after[..end],
            None => after,
        }
    } else {
        raw
    };
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    (end > start).then(|| &body[start..=end])
}

/// True when the text parses as JSON with a non-empty "changes" array.
fn picks_tool(text: &str) -> Option<String> {
    let body = extract_json(text)?;
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let changes = v.get("changes")?.as_array()?;
    if changes.is_empty() {
        return None;
    }
    Some(body.to_string())
}

/// Generate until the model picks at least one tool, or `attempts` run out.
/// Returns the extracted JSON object string (the frontend normalizes it).
/// When `refine` is given, cut boundaries are then snapped to the exact
/// moment with the eyes (see refine_cuts).
pub fn plan(
    system: &str,
    user: &str,
    max_tokens: usize,
    attempts: usize,
    refine: Option<&RefineCtx>,
) -> CoreResult<Option<String>> {
    load()?;
    let guard = cell().lock().map_err(|_| CoreError::msg("brain lock poisoned"))?;
    let model = guard.as_ref().ok_or_else(|| CoreError::msg("brain not loaded"))?;
    let msgs = vec![
        ("system".to_string(), system.to_string()),
        ("user".to_string(), user.to_string()),
    ];
    for attempt in 0..attempts.max(1) {
        let out = llm::chat_once_with(model, &msgs, 8192, max_tokens, sampler_for(attempt))?;
        if std::env::var("MUSTARDY_DEBUG").is_ok() {
            crate::log::line(&format!("brain attempt {attempt}: {out}"));
        }
        if let Some(json) = picks_tool(&out) {
            let json = match refine {
                Some(ctx) => refine_cuts(model, &json, ctx).unwrap_or(json),
                None => json,
            };
            return Ok(Some(json));
        }
    }
    Ok(None)
}

/// For the smoke test: render the prompt and generate once, returning both.
pub fn debug_chat(messages: &[(String, String)], max_tokens: usize) -> CoreResult<(String, String)> {
    load()?;
    let guard = cell().lock().map_err(|_| CoreError::msg("brain lock poisoned"))?;
    let model = guard.as_ref().ok_or_else(|| CoreError::msg("brain not loaded"))?;
    let rendered = llm::render_chat(model, messages)?;
    let out = llm::chat_once_with(model, messages, 8192, max_tokens, sampler_for(0))?;
    Ok((rendered, out))
}

// ---------- precision refinement ----------

/// Context the eyes need to snap cut points to the exact moment.
pub struct RefineCtx {
    pub path: String,
    /// Detected silences — cuts landing on these are already audio-precise
    /// and are left alone (the eyes can't see silence).
    pub silences: Vec<(f64, f64)>,
}

const REFINE_RADIUS: f64 = 0.6; // look this far to either side of a cut point
const MAX_BOUNDARIES: usize = 6;

struct Boundary {
    change: usize,
    edge: &'static str, // "start" | "end"
    t: f64,
    label: String,
    candidates: Vec<f64>,
}

/// The eyes' second job: given a plan's cut points, caption frames around
/// each one and let the brain snap the cut to the frame where the moment
/// actually starts/ends. Best-effort — any failure keeps the original JSON.
fn refine_cuts(model: &LlamaModel, json: &str, ctx: &RefineCtx) -> Option<String> {
    if models::find(&models::EYES_MODEL).is_none() || models::find(&models::EYES_MMPROJ).is_none() {
        return None;
    }
    let mut doc: serde_json::Value = serde_json::from_str(json).ok()?;
    let duration = ffmpeg::probe(&ctx.path).ok()?.duration;

    // Collect cut boundaries, skipping ones already pinned to a silence edge.
    let mut boundaries: Vec<Boundary> = Vec::new();
    for (ci, ch) in doc.get("changes")?.as_array()?.iter().enumerate() {
        if ch.get("type")?.as_str() != Some("cut") {
            continue;
        }
        for edge in ["start", "end"] {
            let Some(t) = ch.get(edge).and_then(|v| v.as_f64()) else { continue };
            let on_silence = ctx
                .silences
                .iter()
                .any(|&(s, e)| (t - s).abs() <= 0.35 || (t - e).abs() <= 0.35);
            let dup = boundaries.iter().any(|b: &Boundary| (b.t - t).abs() <= 0.2);
            if !on_silence && !dup {
                let label = ch
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("cut")
                    .to_string();
                boundaries.push(Boundary {
                    change: ci,
                    edge,
                    t,
                    label,
                    candidates: [t - REFINE_RADIUS, t, t + REFINE_RADIUS]
                        .into_iter()
                        .map(|c| c.clamp(0.05, (duration - 0.1).max(0.05)))
                        .collect(),
                });
            }
        }
    }
    boundaries.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    boundaries.truncate(MAX_BOUNDARIES);
    if boundaries.is_empty() {
        return None;
    }

    // Caption the candidates (3 frames per boundary).
    let mut caption_lines: Vec<String> = Vec::new();
    for (id, b) in boundaries.iter().enumerate() {
        let frames = ffmpeg::extract_frames(&ctx.path, &b.candidates, 384).ok()?;
        let caps = crate::eyes::caption_frames(&frames, 20);
        if caps.len() < 2 {
            continue;
        }
        let seen: Vec<String> = caps.iter().map(|c| format!("{:.2}s \"{}\"", c.t, c.text)).collect();
        caption_lines.push(format!(
            "{}. {} of cut \"{}\" near {:.2}s — candidates: {}",
            id + 1,
            b.edge.to_uppercase(),
            b.label,
            b.t,
            seen.join(" | ")
        ));
    }
    if caption_lines.is_empty() {
        return None;
    }

    let user = format!(
        "Snap each cut point to the candidate time where the described moment actually happens.\n{}\nReply ONLY JSON: {{\"snaps\":[{{\"id\":1,\"t\":12.34}}]}}",
        caption_lines.join("\n")
    );
    let msgs = vec![
        (
            "system".to_string(),
            "You snap video cut points to the exact moment. Reply ONLY JSON.".to_string(),
        ),
        ("user".to_string(), user),
    ];
    let out = llm::chat_once_with(model, &msgs, 2048, 200, sampler_for(0)).ok()?;
    let body = extract_json(&out)?;
    let snaps: serde_json::Value = serde_json::from_str(body).ok()?;
    let snaps = snaps.get("snaps")?.as_array()?;

    let changes = doc.get_mut("changes")?.as_array_mut()?;
    let originals: Vec<(f64, f64)> = changes
        .iter()
        .map(|c| {
            (
                c.get("start").and_then(|v| v.as_f64()).unwrap_or(0.0),
                c.get("end").and_then(|v| v.as_f64()).unwrap_or(0.0),
            )
        })
        .collect();
    let mut applied = 0;
    for snap in snaps {
        let id = snap.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let t = snap.get("t").and_then(|v| v.as_f64()).unwrap_or(f64::NAN);
        if id == 0 || id > boundaries.len() || !t.is_finite() {
            continue;
        }
        let b = &boundaries[id - 1];
        // Snap to the nearest offered candidate; never drift past the radius.
        let Some(&best) = b
            .candidates
            .iter()
            .min_by(|a, c| (**a - t).abs().partial_cmp(&(**c - t).abs()).unwrap_or(std::cmp::Ordering::Equal))
        else {
            continue;
        };
        if (best - b.t).abs() > REFINE_RADIUS + 0.01 {
            continue;
        }
        if let Some(ch) = changes.get_mut(b.change) {
            ch[b.edge] = serde_json::Value::from((best * 100.0).round() / 100.0);
            applied += 1;
            crate::log::line(&format!(
                "refine: cut #{} {} {:.2}s → {:.2}s (eyes)",
                b.change + 1,
                b.edge,
                b.t,
                best
            ));
        }
    }
    if applied == 0 {
        return None;
    }
    // Revert any change whose times stopped making sense.
    for (i, ch) in changes.iter_mut().enumerate() {
        let (s, e) = (
            ch.get("start").and_then(|v| v.as_f64()).unwrap_or(0.0),
            ch.get("end").and_then(|v| v.as_f64()).unwrap_or(0.0),
        );
        if e - s < 0.1 && ch.get("type").and_then(|v| v.as_str()) == Some("cut") {
            ch["start"] = serde_json::Value::from(originals[i].0);
            ch["end"] = serde_json::Value::from(originals[i].1);
        }
    }
    serde_json::to_string(&doc).ok()
}
