use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembraneConfig {
    #[serde(default = "default_gate_url")]
    pub gate_url: String,
    #[serde(default = "default_relay_url")]
    pub relay_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_ttl_secs")]
    pub ttl_secs: i64,
    #[serde(default = "default_delta_t_secs")]
    pub delta_t_secs: u64,
}

fn default_gate_url() -> String {
    "http://127.0.0.1:8787".into()
}

fn default_relay_url() -> String {
    "ws://127.0.0.1:7777".into()
}

fn default_model() -> String {
    "qwen2.5-0.5b-instruct".into()
}

fn default_ttl_secs() -> i64 {
    3600
}

fn default_delta_t_secs() -> u64 {
    300
}

impl Default for MembraneConfig {
    fn default() -> Self {
        Self {
            gate_url: default_gate_url(),
            relay_url: default_relay_url(),
            model: default_model(),
            ttl_secs: default_ttl_secs(),
            delta_t_secs: default_delta_t_secs(),
        }
    }
}

impl MembraneConfig {
    pub fn load() -> Result<Self> {
        if let Some(path) = config_path() {
            if path.exists() {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                return Ok(serde_yaml::from_str(&text)?);
            }
        }
        Ok(Self::default())
    }

    pub fn data_dir() -> Result<PathBuf> {
        let base = dirs_home()?.join(".local/share/membrane");
        std::fs::create_dir_all(&base)?;
        Ok(base)
    }

    pub fn sessions_dir() -> Result<PathBuf> {
        let dir = Self::data_dir()?.join("sessions");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn active_iac_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("active-iac.json"))
    }
}

pub fn config_path() -> Option<PathBuf> {
    dirs_home().ok().map(|h| h.join(".config/membrane/config.yaml"))
}

fn dirs_home() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .context("HOME not set")
}

pub fn write_example_config(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cfg = MembraneConfig::default();
    let yaml = serde_yaml::to_string(&cfg)?;
    std::fs::write(path, yaml)?;
    Ok(())
}
