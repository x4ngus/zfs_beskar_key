// ============================================================================
// src/config.rs – strict config loader (aligned with CLI UX system)
// ============================================================================

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ----------------------------------------------------------------------------
// Policy Section
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// List of managed datasets (e.g., ["rpool/ROOT"])
    pub datasets: Vec<String>,

    /// Optional explicit path to `zfs` binary
    #[serde(default)]
    pub zfs_path: Option<String>,

    /// Optional explicit path to `zfs_beskar_key` binary
    #[serde(default)]
    pub binary_path: Option<String>,

    /// Allow root context execution (advanced users)
    #[serde(default)]
    pub allow_root: bool,
}

// ----------------------------------------------------------------------------
// Crypto Section
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoCfg {
    /// Timeout (seconds) for zfs operations
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

// ----------------------------------------------------------------------------
// USB Section
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usb {
    /// Path to USB key file (binary 32-byte key, usually /run/beskar/key.hex)
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

// ----------------------------------------------------------------------------
// Fallback Section
// ----------------------------------------------------------------------------

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

// ----------------------------------------------------------------------------
// Main Config Object
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    pub policy: Policy,
    #[serde(default)]
    pub crypto: CryptoCfg,
    #[serde(default)]
    pub usb: Usb,
    #[serde(default)]
    pub fallback: Fallback,

    /// Internal path reference for better error messages (not serialized)
    #[serde(skip)]
    pub path: PathBuf,
}

impl ConfigFile {
    /// Load a TOML or YAML config from disk.
    pub fn load<P: AsRef<Path>>(p: P) -> Result<Self> {
        let path_ref = p.as_ref();
        let s = fs::read_to_string(path_ref)
            .with_context(|| format!("read config: {}", path_ref.display()))?;

        let mut cfg: Self = if path_ref
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("toml"))
            .unwrap_or(false)
        {
            toml::from_str(&s).context("toml parse")?
        } else {
            serde_yaml::from_str(&s).context("yaml parse")?
        };

        cfg.path = path_ref.to_path_buf();
        Ok(cfg)
    }
}
