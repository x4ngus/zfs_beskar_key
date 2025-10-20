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
pub struct Usb {
    /// Path to USB key file (usually /run/beskar/key.hex)
    #[serde(default = "default_usb_key_path")]
    pub key_hex_path: String,

    /// Optional SHA-256 checksum for integrity verification
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

fn default_usb_key_path() -> String {
    "/run/beskar/key.hex".to_string()
}

impl Default for Usb {
    fn default() -> Self {
        Self {
            key_hex_path: default_usb_key_path(),
            expected_sha256: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fallback {
    /// Enable fallback to passphrase (or hex) when USB step fails
    #[serde(default)]
    pub enabled: bool,
    /// Use systemd-ask-password when non-interactive (boot)
    #[serde(default)]
    pub askpass: bool,
    /// Optional explicit path to systemd-ask-password (allowlisted)
    #[serde(default)]
    pub askpass_path: Option<String>,
}

impl Default for Fallback {
    fn default() -> Self {
        Self {
            enabled: true,
            askpass: true,
            askpass_path: Some("/usr/bin/systemd-ask-password".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub policy: Policy,
    #[serde(default)]
    pub crypto: CryptoCfg,
    #[serde(default)]
    pub usb: Usb,
    #[serde(default)]
    pub fallback: Fallback,
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
