use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub aur_helper: AurHelperPreference,
    pub editor: Option<String>,
    pub show_arch_news: bool,
    pub snapshot_before_upgrade: bool,
    pub dependency_depth: usize,
    pub aur_rpc_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            aur_helper: AurHelperPreference::Auto,
            editor: env::var("VISUAL").ok().or_else(|| env::var("EDITOR").ok()),
            show_arch_news: true,
            snapshot_before_upgrade: false,
            dependency_depth: 5,
            aur_rpc_url: "https://aur.archlinux.org/rpc/".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AurHelperPreference {
    #[default]
    Auto,
    Paru,
    Yay,
    None,
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<(Self, PathBuf)> {
        let path = path.unwrap_or_else(config_path);
        if !path.exists() {
            return Ok((Self::default(), path));
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let mut config: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse config {}", path.display()))?;
        config.dependency_depth = config.dependency_depth.clamp(1, 20);
        Ok((config, path))
    }
}

pub fn config_path() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("arch-maint/config.toml")
}

pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("arch-maint")
}

pub fn cache_dir() -> PathBuf {
    xdg_cache_home().join("arch-maint")
}

pub fn xdg_cache_home() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache")
}

fn xdg_dir(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable).map(PathBuf::from).unwrap_or_else(|| {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(fallback)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_config_with_defaults() {
        let config: Config = toml::from_str(
            r#"
                aur_helper = "paru"
                dependency_depth = 7
            "#,
        )
        .expect("config should parse");
        assert_eq!(config.aur_helper, AurHelperPreference::Paru);
        assert_eq!(config.dependency_depth, 7);
        assert!(config.show_arch_news);
    }
}
