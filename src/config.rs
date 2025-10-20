// ============================================================================
// src/config.rs – strict config loader
// ============================================================================

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub datasets: Vec<String>,
    #[serde(default)]
    pub zfs_path: Option<String>,
    #[serde(default)]
    pub allow_root: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoCfg {
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    10
}

impl Default for CryptoCfg {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub policy: Policy,
    #[serde(default)]
    pub crypto: CryptoCfg,
}

impl Config {
    pub fn load<P: AsRef<Path>>(p: P) -> Result<Self> {
        let s = fs::read_to_string(&p)
            .with_context(|| format!("read config: {}", p.as_ref().display()))?;
        let cfg: Self = if p.as_ref().extension().and_then(|e| e.to_str()) == Some("toml") {
            toml::from_str(&s).context("toml parse")?
        } else {
            serde_yaml::from_str(&s).context("yaml parse")?
        };
        Ok(cfg)
    }
}
