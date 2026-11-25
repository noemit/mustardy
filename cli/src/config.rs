//! Optional TOML configuration (`./.mustardy.toml` or `~/.config/mustardy/config.toml`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    pub preset: Option<String>,
    pub drop: Option<f64>,
    pub min: Option<f64>,
    pub pad: Option<f64>,
    pub pad_before: Option<f64>,
    pub pad_after: Option<f64>,
    pub transition_margin: Option<f64>,
    pub trim_edges: Option<bool>,
    pub normalize: Option<bool>,
    pub normalize_amount: Option<f64>,
    pub visual: Option<bool>,
    pub max_visual_ranges: Option<usize>,
    pub hw: Option<bool>,
    pub codec: Option<String>,
    pub crf: Option<u8>,
    pub encode_preset: Option<String>,
    pub crossfade_ms: Option<f64>,
    pub jobs: Option<usize>,
}

/// Candidate config file locations in precedence order (highest first).
pub fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(PathBuf::from(".mustardy.toml"));
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            paths.push(PathBuf::from(xdg).join("mustardy").join("config.toml"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            paths.push(
                PathBuf::from(&home)
                    .join(".config")
                    .join("mustardy")
                    .join("config.toml"),
            );
        }
    }
    paths
}

/// Load the first existing config file, returning `(config, Some(path))`.
pub fn load_config() -> (UserConfig, Option<PathBuf>) {
    for path in config_search_paths() {
        if path.is_file() {
            match std::fs::read_to_string(&path) {
                Ok(text) => match toml::from_str::<UserConfig>(&text) {
                    Ok(cfg) => {
                        crate::log::debug(&format!("loaded config: {}", path.display()));
                        return (cfg, Some(path));
                    }
                    Err(e) => {
                        eprintln!(
                            "mustardy: warning — could not parse {}: {e}",
                            path.display()
                        );
                    }
                },
                Err(e) => {
                    eprintln!("mustardy: warning — could not read {}: {e}", path.display());
                }
            }
        }
    }
    (UserConfig::default(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_toml_config() {
        let cfg: UserConfig = toml::from_str(
            r#"
            preset = "tight"
            drop = 26.0
            hw = true
            crf = 20
            "#,
        )
        .unwrap();
        assert_eq!(cfg.preset.as_deref(), Some("tight"));
        assert_eq!(cfg.drop, Some(26.0));
        assert_eq!(cfg.hw, Some(true));
        assert_eq!(cfg.crf, Some(20));
        assert_eq!(cfg.min, None);
    }
}
