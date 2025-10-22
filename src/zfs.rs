// ============================================================================
// src/zfs.rs – safe wrappers for ZFS key operations
// ============================================================================

use crate::cmd::{Cmd, OutputData};
use anyhow::{anyhow, Context, Result};
use std::collections::HashSet;
use std::path::Path;
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
            let stderr = out.stderr.trim();
            if stderr.contains("Key already loaded") {
                return Ok(());
            }
            return Err(anyhow!("zfs load-key failed: {}", stderr));
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

    /// Change the dataset key by pointing ZFS at a temporary key file.
    pub fn change_key_from_file(&self, dataset: &str, key_path: &Path) -> Result<()> {
        let keylocation = format!("keylocation=file://{}", key_path.display());
        let out = self.run(
            &[
                "change-key",
                "-o",
                "keyformat=raw",
                "-o",
                &keylocation,
                dataset,
            ],
            None,
        )?;
        if out.status != 0 {
            return Err(anyhow!("zfs change-key failed: {}", out.stderr));
        }
        Ok(())
    }

    /// Set an arbitrary property on a dataset (used for keylocation/keyformat resets).
    pub fn set_property(&self, dataset: &str, property: &str, value: &str) -> Result<()> {
        let assignment = format!("{}={}", property, value);
        let out = self.run(&["set", &assignment, dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!("zfs set {} failed: {}", assignment, out.stderr));
        }
        Ok(())
    }

    /// Query a single dataset property and return the trimmed value.
    pub fn get_property(&self, dataset: &str, property: &str) -> Result<String> {
        let out = self.run(&["get", "-H", "-o", "value", property, dataset], None)?;
        if out.status != 0 {
            return Err(anyhow!(
                "zfs get {} failed: {}",
                property,
                out.stderr.trim()
            ));
        }
        Ok(out.stdout.trim().to_string())
    }

    /// Attempt to load keys for the encryption root and any descendants sharing it.
    /// Returns the list of datasets confirmed unlocked (root is always first).
    pub fn load_key_tree(&self, root: &str, key: &[u8]) -> Result<Vec<String>> {
        self.load_key(root, key)?;

        let mut unlocked = vec![root.to_string()];

        let pending_scan = self.locked_descendants(root)?;
        if pending_scan.iter().any(|ds| ds == root) {
            return Err(anyhow!(
                "Encryption root {} still reports a sealed keystatus after load-key",
                root
            ));
        }

        let pending: Vec<String> = pending_scan.into_iter().filter(|ds| ds != root).collect();

        if !pending.is_empty() {
            for ds in pending.clone() {
                self.load_key(&ds, key)?;
                unlocked.push(ds);
            }
        }

        let stubborn_scan = self.locked_descendants(root)?;
        if stubborn_scan.iter().any(|ds| ds == root) {
            return Err(anyhow!(
                "Encryption root {} unexpectedly sealed after descendant retries",
                root
            ));
        }

        let stubborn: Vec<String> = stubborn_scan.into_iter().filter(|ds| ds != root).collect();
        if !stubborn.is_empty() {
            return Err(anyhow!(
                "Datasets inheriting {} remain sealed after retries: {}",
                root,
                stubborn.join(", ")
            ));
        }

        Ok(unlocked)
    }

    /// Return datasets under `root` that still report a sealed keystatus.
    pub fn locked_descendants(&self, root: &str) -> Result<Vec<String>> {
        let list = self.run(
            &["list", "-H", "-r", "-o", "name,encryptionroot", root],
            None,
        )?;
        if list.status != 0 {
            return Err(anyhow!(
                "zfs list encryption roots failed for {}: {}",
                root,
                list.stderr.trim()
            ));
        }
        let mut same_root = HashSet::new();
        for line in list.stdout.lines() {
            let mut parts = line.split('\t');
            if let (Some(name), Some(encryption_root)) = (parts.next(), parts.next()) {
                if encryption_root == root {
                    same_root.insert(name.to_string());
                }
            }
        }
        let status = self.run(
            &["get", "-H", "-r", "-o", "name,value", "keystatus", root],
            None,
        )?;
        if status.status != 0 {
            return Err(anyhow!(
                "zfs get keystatus failed for {}: {}",
                root,
                status.stderr.trim()
            ));
        }
        let mut locked = Vec::new();
        for line in status.stdout.lines() {
            let mut parts = line.split('\t');
            if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                let trimmed = value.trim();
                if same_root.contains(name)
                    && trimmed != "available"
                    && !trimmed.is_empty()
                    && trimmed != "-"
                    && trimmed != "none"
                {
                    locked.push(name.to_string());
                }
            }
        }
        Ok(locked)
    }

    /// Discover dataset by mountpoint (e.g., "/"). Returns the first match.
    pub fn dataset_with_mountpoint(&self, mountpoint: &str) -> Result<Option<String>> {
        let out = self.run(
            &["list", "-H", "-o", "name,mountpoint", "-t", "filesystem"],
            None,
        )?;
        if out.status != 0 {
            return Err(anyhow!(
                "zfs list mountpoints failed: {}",
                out.stderr.trim()
            ));
        }
        for line in out.stdout.lines() {
            let mut parts = line.split('\t');
            if let (Some(name), Some(mp)) = (parts.next(), parts.next()) {
                if mp == mountpoint {
                    return Ok(Some(name.to_string()));
                }
            }
        }
        Ok(None)
    }
}
