// ============================================================================
// src/util/binary.rs – Helpers to locate the running zfs_beskar_key binary
// ============================================================================

use crate::config::ConfigFile;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Resolve the path to the zfs_beskar_key binary, preferring the configured
/// value when present, otherwise falling back to the running executable and
/// finally to the default installation prefix.
pub fn determine_binary_path(cfg: Option<&ConfigFile>) -> Result<PathBuf> {
    if let Some(cfg) = cfg {
        if let Some(path) = cfg
            .policy
            .binary_path
            .as_ref()
            .and_then(|p| sanitize_path(Path::new(p)))
        {
            return Ok(path);
        }
    }

    let current = std::env::current_exe().context("determine current executable path")?;
    if let Some(path) = sanitize_path(&current) {
        return Ok(path);
    }

    let fallback = Path::new("/usr/local/bin/zfs_beskar_key");
    if let Some(path) = sanitize_path(fallback) {
        return Ok(path);
    }

    Err(anyhow!(
        "unable to resolve zfs_beskar_key binary path; set policy.binary_path in config"
    ))
}

fn sanitize_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }

    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => match fs::canonicalize(path) {
            Ok(canonical) => Some(canonical),
            Err(_) => Some(path.to_path_buf()),
        },
        _ => None,
    }
}
