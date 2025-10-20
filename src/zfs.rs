// ============================================================================
// src/zfs.rs – safe wrappers for ZFS key operations
// ============================================================================

use anyhow::{anyhow, Context, Result};
use std::time::Duration;

/// Safe ZFS command wrapper. All calls go through the allowlisted `cmd` layer.
pub struct Zfs {
    cmd: crate::cmd::Cmd,
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

        let mut last_err: Option<anyhow::Error> = None;
        for c in &candidates {
            match crate::cmd::Cmd::new_allowlisted(c, timeout) {
                Ok(cmd) => return Ok(Self { cmd }),
                Err(e) => last_err = Some(e),
            }
        }

        Err(anyhow!("zfs binary not found: {:?}", last_err))
    }

    /// Returns ZFS keyformat for dataset (e.g., "passphrase", "hex", "raw", "none")
    pub fn keyformat(&self, dataset: &str) -> Result<String> {
        let out = self
            .cmd
            .run(&["get", "-H", "-o", "value", "keyformat", dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs get keyformat failed: {}", out.stderr));
        }
        Ok(out.stdout.trim().to_string())
    }

    /// Use an explicit binary path (for policy-controlled environments).
    pub fn with_path(path: &str, timeout: Duration) -> Result<Self> {
        Ok(Self {
            cmd: crate::cmd::Cmd::new_allowlisted(path, timeout)?,
        })
    }

    /// Returns true if dataset encryption is enabled.
    pub fn is_encrypted(&self, dataset: &str) -> Result<bool> {
        let out = self
            .cmd
            .run(&["get", "-H", "-o", "value", "encryption", dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs get failed: {}", out.stderr));
        }
        let v = out.stdout.trim();
        Ok(v != "off" && !v.is_empty())
    }

    /// Returns true if dataset key is already loaded.
    pub fn is_unlocked(&self, dataset: &str) -> Result<bool> {
        let out = self
            .cmd
            .run(&["get", "-H", "-o", "value", "keystatus", dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs get keystatus failed: {}", out.stderr));
        }
        Ok(out.stdout.trim() == "available")
    }

    /// Loads a key into ZFS using stdin (never shell-escaped).
    pub fn load_key(&self, dataset: &str, key: &[u8]) -> Result<()> {
        let mut key_nl = Vec::with_capacity(key.len() + 1);
        key_nl.extend_from_slice(key);
        key_nl.push(b'\n');

        let out = self
            .cmd
            .run(&["load-key", "-L", "prompt", dataset], Some(&key_nl))
            .context("zfs load-key")?;

        if out.status != 0 {
            return Err(anyhow!("zfs load-key failed: {}", out.stderr));
        }
        Ok(())
    }

    /// Unloads a key from ZFS, sealing the dataset.
    pub fn unload_key(&self, dataset: &str) -> Result<()> {
        let out = self.cmd.run(&["unload-key", dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs unload-key failed: {}", out.stderr));
        }
        Ok(())
    }
}
