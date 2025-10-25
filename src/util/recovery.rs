// ============================================================================
// src/util/recovery.rs – encode/decode helpers for recovery keys
// ============================================================================

use anyhow::{anyhow, Result};
use data_encoding::BASE32_NOPAD;
use zeroize::Zeroizing;

pub fn encode_recovery_code(raw: &[u8]) -> String {
    BASE32_NOPAD.encode(raw).to_uppercase()
}

pub fn decode_recovery_code(input: &str) -> Result<Zeroizing<Vec<u8>>> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let bytes = BASE32_NOPAD
        .decode(cleaned.as_bytes())
        .map_err(|e| anyhow!("Recovery key invalid: {}", e))?;
    if bytes.len() != 32 {
        return Err(anyhow!(
            "Recovery key decoded to {} bytes (expected 32).",
            bytes.len()
        ));
    }
    Ok(Zeroizing::new(bytes))
}
