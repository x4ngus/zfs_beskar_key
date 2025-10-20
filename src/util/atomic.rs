// ============================================================================
// src//atomic.rs – Durable, permissioned atomic writes (key + config)
// ============================================================================

use anyhow::{bail, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Return the parent directory path or error with context.
fn parent_dir(path: &Path) -> Result<PathBuf> {
    path.parent()
        .map(|p| p.to_path_buf())
        .context("Target path has no parent directory")
}

/// Fsync a directory to persist metadata (like rename).
fn fsync_dir(dir: &Path) -> Result<()> {
    let f = File::open(dir).with_context(|| format!("Open dir for fsync: {dir:?}"))?;
    f.sync_all()
        .with_context(|| format!("Fsync dir failed: {dir:?}"))?;
    Ok(())
}

/// Reject writes if target is a symlink (avoid TOCTOU surprises at the destination).
fn reject_symlink_target(path: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            bail!("Refusing to write to symlink: {}", path.display());
        }
    }
    Ok(())
}

/// Core atomic write: writes bytes to a temp file in the same directory,
/// fsyncs the file, renames into place, then fsyncs the parent directory.
/// Applies exact POSIX mode (ignores umask).
pub fn atomic_write_bytes(path: &Path, bytes: &[u8], mode: u32, force: bool) -> Result<()> {
    reject_symlink_target(path)?;

    let dir = parent_dir(path)?;
    // Ensure directory exists (caller should create with correct perms earlier if needed)
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .with_context(|| format!("Create parent directory failed: {dir:?}"))?;
    }

    // If not forcing, abort when file already exists
    if !force && path.exists() {
        bail!(
            "File already exists (use --force to overwrite): {}",
            path.display()
        );
    }

    // Create a temp file in the same directory, with strict permissions
    let mut tmp_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .context("Target path missing file name")?;
    tmp_name.push(".tmp-XXXXXX");

    // Use a unique temp name (low-collision approach)
    let mut tmp = dir.join(tmp_name);
    for _ in 0..8 {
        let rand = nanoid::nanoid!(8);
        tmp.set_file_name(format!(
            "{}.tmp-{}",
            path.file_name().unwrap().to_string_lossy(),
            rand
        ));
        if !tmp.exists() {
            break;
        }
    }

    // Open temp file with explicit mode
    let mut f = OpenOptions::new()
        .create_new(true) // fail if exists
        .write(true)
        .mode(mode) // exact mode (0400, 0600, etc.)
        .open(&tmp)
        .with_context(|| format!("Open temp file failed: {tmp:?}"))?;

    // Write data without cloning
    f.write_all(bytes).context("Write to temp file failed")?;
    f.sync_all().context("Fsync temp file failed")?;

    // Atomically replace destination
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "Atomic rename failed ({} -> {})",
            tmp.display(),
            path.display()
        )
    })?;

    // Ensure final permissions (defense-in-depth)
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("Set permissions failed for {}", path.display()))?;

    // Persist directory entry
    fsync_dir(&dir)?;

    Ok(())
}

/// Atomic write of TOML-serializable config with 0600 permissions.
pub fn atomic_write_toml<T: serde::Serialize>(path: &Path, value: &T, force: bool) -> Result<()> {
    let s = toml::to_string_pretty(value).context("Serialize TOML failed")?;
    atomic_write_bytes(path, s.as_bytes(), 0o600, force)
}

/// Convenience: atomic write for key material (binary) with 0400 permissions.
pub fn atomic_write_key(path: &Path, key: &[u8], force: bool) -> Result<()> {
    atomic_write_bytes(path, key, 0o400, force)
}
