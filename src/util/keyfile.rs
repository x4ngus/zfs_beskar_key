// ============================================================================
// src/util/keyfile.rs – helpers for reading/writing USB key material
// ============================================================================

use anyhow::{anyhow, Context, Result};
use std::fs::{self, File, Permissions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEncoding {
    Raw,
    Hex,
}

#[derive(Debug)]
pub struct KeyMaterialDisk {
    pub raw: Zeroizing<Vec<u8>>,
    pub encoding: KeyEncoding,
}

/// Read key material from disk, auto-detecting whether it is raw bytes or legacy hex.
pub fn read_key_material(path: &Path) -> Result<KeyMaterialDisk> {
    let data = fs::read(path).with_context(|| format!("read key file {}", path.display()))?;
    if data.len() == 32 {
        return Ok(KeyMaterialDisk {
            raw: Zeroizing::new(data),
            encoding: KeyEncoding::Raw,
        });
    }

    let text = String::from_utf8_lossy(&data);
    let cleaned: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() == 64 {
        let decoded = Zeroizing::new(
            hex::decode(&cleaned)
                .with_context(|| format!("decode hex key material at {}", path.display()))?,
        );
        return Ok(KeyMaterialDisk {
            raw: decoded,
            encoding: KeyEncoding::Hex,
        });
    }

    Err(anyhow!(
        "Key file {} malformed (expected 32 raw bytes or 64 hex chars).",
        path.display()
    ))
}

/// Ensure the on-disk key file contains raw bytes; legacy hex files are rewritten in-place.
pub fn ensure_raw_key_file(path: &Path) -> Result<KeyMaterialDisk> {
    let mut key = read_key_material(path)?;
    if key.encoding == KeyEncoding::Hex {
        rewrite_key_file(path, &key.raw)?;
        key.encoding = KeyEncoding::Raw;
    }
    Ok(key)
}

/// Rewrite the key file with the provided raw bytes and secure permissions.
pub fn rewrite_key_file(path: &Path, raw: &[u8]) -> Result<()> {
    let mut file =
        File::create(path).with_context(|| format!("rewrite key file {}", path.display()))?;
    file.write_all(raw)?;
    file.sync_all().ok();
    fs::set_permissions(path, Permissions::from_mode(0o400))
        .with_context(|| format!("set permissions on {}", path.display()))?;
    Ok(())
}
