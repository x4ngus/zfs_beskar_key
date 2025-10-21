// ============================================================================
// src/zfs.rs – safe wrappers for ZFS key operations
// ============================================================================

use crate::cmd::{Cmd, OutputData};
use anyhow::{anyhow, Context, Result};
use std::time::Duration;

/// Safe ZFS command wrapper. All calls go through the allow-listed `cmd` layer.
pub struct Zfs {
    path: String,
    timeout: Duration,
}

impl Zfs {
    /// Auto-discover `zfs` binary from common system locations.
    pub fn discover(timeout: Duration) -> Result<Self> {
        let candidates = [
            "/sbin/zfs",
            "/usr/sbin/zfs",
            "/usr/local/sbin/zfs",
            "/bin/zfs",
        ];

        for c in &candidates {
            if std::path::Path::new(c).exists() {
                return Ok(Self {
                    path: c.to_string(),
                    timeout,
                });
            }
        }
        Err(anyhow!("zfs binary not found in {:?}", candidates))
    }

    /// Use an explicit binary path (for policy-controlled environments).
    pub fn with_path(path: &str, timeout: Duration) -> Result<Self> {
        if !std::path::Path::new(path).exists() {
            return Err(anyhow!("zfs binary not found at {}", path));
        }
        Ok(Self {
            path: path.to_string(),
            timeout,
        })
    }

    /// Internal runner for all ZFS sub-commands.
    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<OutputData> {
        let cmd = Cmd::new_allowlisted(&self.path, self.timeout)?;
        cmd.run(args, input)
    }

    /// Returns true if dataset encryption is enabled.
    pub fn is_encrypted(&self, dataset: &str) -> Result<bool> {
        let out = self.run(&["get", "-H", "-o", "value", "encryption", dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs get encryption failed: {}", out.stderr));
        }
        let v = out.stdout.trim();
        Ok(v != "off" && !v.is_empty())
    }

    /// Returns true if dataset key is loaded.
    pub fn is_unlocked(&self, dataset: &str) -> Result<bool> {
        let out = self.run(&["get", "-H", "-o", "value", "keystatus", dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs get keystatus failed: {}", out.stderr));
        }
        Ok(out.stdout.trim() == "available")
    }

    /// Loads a key into ZFS using stdin (never shell-escaped).
    pub fn load_key(&self, dataset: &str, key: &[u8]) -> Result<()> {
        let out = self
            .run(&["load-key", "-L", "prompt", dataset], Some(key))
            .context("zfs load-key")?;
        if out.status != 0 {
            return Err(anyhow!("zfs load-key failed: {}", out.stderr));
        }
        Ok(())
    }

    /// Unloads a key from ZFS, sealing the dataset.
    pub fn unload_key(&self, dataset: &str) -> Result<()> {
        let out = self.run(&["unload-key", dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs unload-key failed: {}", out.stderr));
        }
        Ok(())
    }

    /// Returns the encryption root for a dataset.
    pub fn encryption_root(&self, dataset: &str) -> Result<String> {
        let out = self.run(
            &["get", "-H", "-o", "value", "encryptionroot", dataset],
            None,
        )?;
        if out.status != 0 {
            return Err(anyhow!("zfs get encryptionroot failed: {}", out.stderr));
        }
        Ok(out.stdout.trim().to_string())
    }
}
