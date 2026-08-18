//! Remote providers: Kimi (Moonshot) and Ollama. Port of electron/ai.cjs.

use serde::Serialize;
use serde_json::json;

use crate::{ChatMessage, CoreError, CoreResult};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AiSettings {
    #[serde(rename = "ollamaUrl", default = "default_ollama")]
    pub ollama_url: String,
    #[serde(rename = "qwenModel", default = "default_qwen")]
    pub qwen_model: String,
    #[serde(rename = "kimiBaseUrl", default = "default_kimi_base")]
    pub kimi_base_url: String,
    #[serde(rename = "kimiModel", default = "default_kimi_model")]
    pub kimi_model: String,
    #[serde(rename = "kimiKey", default)]
    pub kimi_key: String,
}

fn default_ollama() -> String { "http://127.0.0.1:11434".into() }
fn default_qwen() -> String { "qwen2.5:7b".into() }
fn default_kimi_base() -> String { "https://api.moonshot.ai/v1".into() }
fn default_kimi_model() -> String { "moonshot-v1-32k-vision-preview".into() }

#[derive(Debug, Serialize)]
pub struct ChatOut {
    pub text: String,
    pub provider: String,
    pub model: String,
}

fn post_json(url: &str, headers: &[(&str, &str)], body: serde_json::Value) -> CoreResult<serde_json::Value> {
    let mut req = ureq::post(url);
    for &(k, v) in headers {
        req = req.header(k, v);
    }
    let mut resp = req.send_json(&body).map_err(|e| match e {
        ureq::Error::StatusCode(code) => {
            CoreError::msg(format!("{url} returned {code}"))
        }
        other => CoreError::Http(other),
    })?;
    let text = resp.body_mut().read_to_string()?;
    Ok(serde_json::from_str(&text)?)
}

pub fn chat(provider: &str, settings: Option<AiSettings>, messages: Vec<ChatMessage>, system: String) -> CoreResult<ChatOut> {
    let cfg = settings.unwrap_or(AiSettings {
        ollama_url: default_ollama(),
        qwen_model: default_qwen(),
        kimi_base_url: default_kimi_base(),
        kimi_model: default_kimi_model(),
        kimi_key: String::new(),
    });
    match provider {
        "kimi" => chat_kimi(&cfg, messages, system),
        "ollama" => chat_ollama(&cfg, messages, system),
        other => Err(CoreError::msg(format!("unknown provider {other}"))),
    }
}

fn chat_ollama(cfg: &AiSettings, messages: Vec<ChatMessage>, system: String) -> CoreResult<ChatOut> {
    let url = format!("{}/api/chat", cfg.ollama_url.trim_end_matches('/'));
    let mut msgs = vec![json!({"role": "system", "content": system})];
    for m in &messages {
        msgs.push(json!({"role": m.role, "content": m.content}));
    }
    let data = post_json(
        &url,
        &[],
        json!({
            "model": cfg.qwen_model,
            "stream": false,
            "format": "json",
            "messages": msgs,
            "options": { "temperature": 0.4 }
        }),
    )?;
    Ok(ChatOut {
        text: data["message"]["content"].as_str().unwrap_or("").to_string(),
        provider: "ollama".into(),
        model: cfg.qwen_model.clone(),
    })
}

fn chat_kimi(cfg: &AiSettings, messages: Vec<ChatMessage>, system: String) -> CoreResult<ChatOut> {
    let key = if cfg.kimi_key.is_empty() {
        std::env::var("KIMI_API_KEY")
            .or_else(|_| std::env::var("MOONSHOT_API_KEY"))
            .map_err(|_| CoreError::msg("Add a Kimi / Moonshot API key in Settings."))?
    } else {
        cfg.kimi_key.clone()
    };
    let url = format!("{}/chat/completions", cfg.kimi_base_url.trim_end_matches('/'));
    let mut msgs = vec![json!({"role": "system", "content": system})];
    for m in &messages {
        msgs.push(json!({"role": m.role, "content": m.content}));
    }
    let data = post_json(
        &url,
        &[("Authorization", &format!("Bearer {key}"))],
        json!({
            "model": cfg.kimi_model,
            "temperature": 0.4,
            "response_format": { "type": "json_object" },
            "messages": msgs,
        }),
    )?;
    Ok(ChatOut {
        text: data["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string(),
        provider: "kimi".into(),
        model: cfg.kimi_model.clone(),
    })
}

#[derive(Debug, Serialize)]
pub struct ModelsOut {
    pub models: Vec<String>,
    pub online: bool,
}

pub fn list_models(provider: &str, settings: Option<AiSettings>) -> CoreResult<ModelsOut> {
    let cfg = settings.unwrap_or(AiSettings {
        ollama_url: default_ollama(),
        qwen_model: default_qwen(),
        kimi_base_url: default_kimi_base(),
        kimi_model: default_kimi_model(),
        kimi_key: String::new(),
    });
    match provider {
        "kimi" => Ok(ModelsOut {
            models: vec![cfg.kimi_model.clone()],
            online: !cfg.kimi_key.is_empty(),
        }),
        "ollama" => {
            let url = format!("{}/api/tags", cfg.ollama_url.trim_end_matches('/'));
            match ureq::get(&url).call() {
                Ok(mut resp) => {
                    let data: serde_json::Value = serde_json::from_str(&resp.body_mut().read_to_string()?)?;
                    let models = data["models"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    Ok(ModelsOut { models, online: true })
                }
                Err(_) => Ok(ModelsOut { models: vec![], online: false }),
            }
        }
        _ => Ok(ModelsOut { models: vec!["local-eyes".into()], online: true }),
    }
}
