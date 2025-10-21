// ============================================================================
// src/cmd/unlock.rs – Secure unlock workflow with adaptive lockout
// ============================================================================

use crate::cmd::Cmd;
use crate::config::ConfigFile;
use crate::ui::{Pace, Timing, UX};
use crate::util::audit::audit_log;
use crate::util::lockout::Lockout;
use crate::zfs::Zfs;
use anyhow::{anyhow, Context, Result};
use dialoguer::Password;
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
        "Initiating unlock sequence for dataset {}.",
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
        ui.success("Dataset already stands open; no further strikes required.");
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
                    "Dataset {} draws its ward from encryption root {}.",
                    dataset, root
                ));
            } else {
                ui.info(&format!("Encryption root stands as {}.", root));
            }
            root
        }
        Err(_) => {
            ui.warn("Unable to trace the encryption root; proceeding directly against the dataset.");
            dataset.to_string()
        }
    };
    audit_log(
        "UNLOCK_ROOT",
        &format!("Target encryption root: {}", enc_root),
    );

    // ------------------------------------------------------------------------
    // Step 3: Attempt unlock (with USB-first path and fallback)
    // ------------------------------------------------------------------------
    const MAX_ATTEMPTS: usize = 3;
    let mut lockout = Lockout::new();

    for attempt in 1..=MAX_ATTEMPTS {
        ui.info(&format!(
            "Attempt {}/{} to unlock {}...",
            attempt, MAX_ATTEMPTS, enc_root
        ));
        timing.pace(Pace::Info);

        let (key_material, origin) =
            match obtain_key_material(ui, timing, cfg, &enc_root, attempt == 1) {
                Ok(pair) => pair,
                Err(err) => {
                    ui.error(&format!("Unable to obtain key material ({}).", err));
                    audit_log("UNLOCK_KEY_FETCH_FAIL", &err.to_string());
                    return Err(err);
                }
            };

        match zfs.load_key(&enc_root, &key_material) {
            Ok(_) => {
                ui.success(&format!(
                    "Key accepted. Encryption root {} now stands unlocked.",
                    enc_root
                ));
                if matches!(origin, KeyOrigin::Passphrase) {
                    ui.note("Fallback passphrase accepted. Replace or rebuild the beskar key at the earliest opportunity.");
                }
                audit_log("UNLOCK_OK", &format!("Unlocked {}", enc_root));
                lockout.reset(ui, timing);
                return Ok(());
            }
            Err(e) => {
                ui.error(&format!("Unlock attempt on {} failed ({}).", enc_root, e));
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
    ui.error("Unlock failed after exhausting the maximum retry attempts.");
    Err(anyhow!(
        "Unlock failed after {} attempts for {}",
        MAX_ATTEMPTS,
        enc_root
    ))
}

enum KeyOrigin {
    Usb,
    Passphrase,
}

fn obtain_key_material(
    ui: &UX,
    timing: &Timing,
    cfg: &ConfigFile,
    enc_root: &str,
    first_attempt: bool,
) -> Result<(Zeroizing<Vec<u8>>, KeyOrigin)> {
    match load_usb_key_material(ui, cfg) {
        Ok(bytes) => {
            if first_attempt {
                audit_log("UNLOCK_SOURCE", "Using USB key material");
            }
            Ok((bytes, KeyOrigin::Usb))
        }
        Err(usb_err) => {
            audit_log("UNLOCK_USB_UNAVAILABLE", &format!("reason={}", usb_err));

            if !cfg.fallback.enabled {
                return Err(usb_err);
            }

            ui.warn(&format!(
                "USB key unavailable ({}); invoking the fallback passphrase ritual.",
                usb_err
            ));
            timing.pace(Pace::Prompt);

            let passphrase =
                prompt_fallback_passphrase(ui, timing, cfg, enc_root).map_err(|fallback_err| {
                    anyhow!(
                        "USB key unavailable ({}) and fallback failed ({})",
                        usb_err,
                        fallback_err
                    )
                })?;

            audit_log("UNLOCK_FALLBACK_USED", "Fallback passphrase requested");
            Ok((passphrase, KeyOrigin::Passphrase))
        }
    }
}

fn load_usb_key_material(ui: &UX, cfg: &ConfigFile) -> Result<Zeroizing<Vec<u8>>> {
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
            return Err(anyhow!(
                "USB key checksum mismatch (expected {}, found {})",
                expected,
                actual
            ));
        }
        ui.success("USB key checksum verified (SHA-256 match).");
        audit_log("UNLOCK_CHECKSUM", "Checksum verified successfully");
    } else {
        ui.warn(
            "No reference SHA-256 recorded in config.usb.expected_sha256 — authenticity check skipped.",
        );
        audit_log("UNLOCK_CHECKSUM_SKIP", "Checksum skipped; field not set");
    }

    Ok(raw_bytes)
}

fn prompt_fallback_passphrase(
    ui: &UX,
    timing: &Timing,
    cfg: &ConfigFile,
    enc_root: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    ui.note(&format!(
        "Fallback activation: provide the passphrase for {}.",
        enc_root
    ));
    timing.pace(Pace::Prompt);

    if cfg.fallback.askpass {
        if let Some(path) = cfg.fallback.askpass_path.as_deref() {
            if Path::new(path).exists() {
                if let Ok(cmd) = Cmd::new_allowlisted(path, Duration::from_secs(90)) {
                    let prompt = format!("Beskar fallback passphrase for {}", enc_root);
                    match cmd.run(&["--timeout=90", &prompt], None) {
                        Ok(out) if out.status == 0 => {
                            let cleaned = out
                                .stdout
                                .trim_end_matches(|c| c == '\n' || c == '\r')
                                .to_string();
                            if !cleaned.is_empty() {
                                ui.info("Passphrase captured via systemd-ask-password.");
                                return Ok(Zeroizing::new(cleaned.into_bytes()));
                            }
                            ui.warn("Fallback prompt returned empty response.");
                        }
                        Ok(out) => {
                            ui.warn(&format!(
                                "systemd-ask-password exited with status {}: {}",
                                out.status, out.stderr
                            ));
                        }
                        Err(err) => {
                            ui.warn(&format!(
                                "Unable to invoke systemd-ask-password at {}: {}",
                                path, err
                            ));
                        }
                    }
                } else {
                    ui.warn(&format!(
                        "systemd-ask-password at {} is not allowlisted.",
                        path
                    ));
                }
            } else {
                ui.warn(&format!("Configured ask-password path {} not found.", path));
            }
        }
    }

    // Interactive fallback via dialoguer
    let prompt = format!("Enter fallback passphrase for {}", enc_root);
    let passphrase = Password::new()
        .with_prompt(prompt)
        .allow_empty_password(false)
        .interact()
        .context("interactive fallback passphrase prompt failed")?;

    Ok(Zeroizing::new(passphrase.into_bytes()))
}
