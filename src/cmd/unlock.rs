// ============================================================================
// src/cmd/unlock.rs – Secure unlock workflow with adaptive lockout
// ============================================================================

use crate::config::ConfigFile;
use crate::ui::{Pace, Timing, UX};
use crate::util::audit::audit_log;
use crate::util::lockout::Lockout;
use crate::zfs::Zfs;
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::Duration;
use zeroize::Zeroizing;

// ----------------------------------------------------------------------------
// Public entrypoint
// ----------------------------------------------------------------------------

pub fn run_unlock(ui: &UX, timing: &Timing, cfg: &ConfigFile, dataset: &str) -> Result<()> {
    ui.banner();
    ui.info(&format!(
        "Starting unlock sequence for dataset: {}",
        dataset
    ));
    timing.pace(Pace::Info);

    // ------------------------------------------------------------------------
    // Step 1: Prepare ZFS interface and verify dataset state
    // ------------------------------------------------------------------------
    let zfs = if let Some(path) = &cfg.policy.zfs_path {
        Zfs::with_path(path, Duration::from_secs(cfg.crypto.timeout_secs))?
    } else {
        Zfs::discover(Duration::from_secs(cfg.crypto.timeout_secs))?
    };

    if zfs.is_unlocked(dataset)? {
        ui.success("Dataset already unlocked. No action taken.");
        audit_log("UNLOCK_SKIP", &format!("{} already unlocked", dataset));
        return Ok(());
    }

    // ------------------------------------------------------------------------
    // Step 2: Identify encryption root
    // ------------------------------------------------------------------------
    let enc_root = match zfs.encryption_root(dataset) {
        Ok(root) => {
            if root != dataset {
                ui.info(&format!(
                    "Dataset {} inherits encryption from root {}.",
                    dataset, root
                ));
            } else {
                ui.info(&format!("Encryption root identified as {}.", root));
            }
            root
        }
        Err(_) => {
            ui.warn("Unable to determine encryption root. Proceeding with given dataset.");
            dataset.to_string()
        }
    };
    audit_log(
        "UNLOCK_ROOT",
        &format!("Target encryption root: {}", enc_root),
    );

    // ------------------------------------------------------------------------
    // Step 3: Read USB key and verify checksum
    // ------------------------------------------------------------------------
    let key_path = Path::new(&cfg.usb.key_hex_path);
    if !key_path.exists() {
        return Err(anyhow!("Key file not found: {}", key_path.display()));
    }

    let key_data = fs::read_to_string(key_path)
        .with_context(|| format!("failed to read {}", key_path.display()))?;
    let cleaned: String = key_data.chars().filter(|c| c.is_ascii_hexdigit()).collect();

    if cleaned.len() != 64 {
        return Err(anyhow!(
            "Key file malformed (expected 64 hex chars, found {})",
            cleaned.len()
        ));
    }

    let raw_bytes = Zeroizing::new(hex::decode(&cleaned)?);

    if let Some(expected) = &cfg.usb.expected_sha256 {
        let mut hasher = Sha256::new();
        hasher.update(&*raw_bytes);
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            ui.error("USB key checksum mismatch.");
            ui.warn(&format!("Expected: {}\nFound:    {}", expected, actual));
            audit_log("UNLOCK_FAIL", "Checksum mismatch on USB key");
            return Err(anyhow!("USB key checksum mismatch"));
        }
        ui.success("USB key checksum verified (SHA-256 match).");
        audit_log("UNLOCK_CHECKSUM", "Checksum verified successfully");
    } else {
        ui.warn("No expected SHA-256 checksum found in config.usb.expected_sha256.");
        audit_log("UNLOCK_CHECKSUM_SKIP", "Checksum skipped; field not set");
    }

    // ------------------------------------------------------------------------
    // Step 4: Attempt unlock (with adaptive lockout control)
    // ------------------------------------------------------------------------
    const MAX_ATTEMPTS: usize = 3;
    let mut lockout = Lockout::new();

    for attempt in 1..=MAX_ATTEMPTS {
        ui.info(&format!(
            "Attempt {}/{} to unlock encryption root {}...",
            attempt, MAX_ATTEMPTS, enc_root
        ));
        timing.pace(Pace::Info);

        match zfs.load_key(&enc_root, &raw_bytes) {
            Ok(_) => {
                ui.success(&format!(
                    "Key accepted. Encryption root {} unlocked successfully.",
                    enc_root
                ));
                audit_log("UNLOCK_OK", &format!("Unlocked {}", enc_root));
                lockout.reset(ui, timing);
                return Ok(());
            }
            Err(e) => {
                ui.error(&format!("Unlock failed for {}: {}", enc_root, e));
                audit_log(
                    "UNLOCK_ATTEMPT_FAIL",
                    &format!("Attempt {} failed for {}: {}", attempt, enc_root, e),
                );

                if attempt < MAX_ATTEMPTS {
                    lockout.register_failure(ui, timing);
                    lockout.wait_if_needed(ui, timing);
                }
            }
        }
    }

    // ------------------------------------------------------------------------
    // Step 5: Exhausted retries
    // ------------------------------------------------------------------------
    audit_log(
        "UNLOCK_ABORT",
        &format!(
            "Maximum unlock attempts ({}) reached for {}",
            MAX_ATTEMPTS, enc_root
        ),
    );
    ui.error("Unlock failed after maximum retry attempts.");
    Err(anyhow!(
        "Unlock failed after {} attempts for {}",
        MAX_ATTEMPTS,
        enc_root
    ))
}
