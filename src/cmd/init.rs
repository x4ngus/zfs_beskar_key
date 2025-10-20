// ============================================================================
// src/cmd/init.rs – Flagship initialization workflow
// (Generates USB key, config file, and recovery key)
// ============================================================================

use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::ui::{Pace, Timing, UX};
use crate::util::atomic::{atomic_write_key, atomic_write_toml};
use crate::util::audit::audit_log;

use rand::distributions::Alphanumeric;
use rand::{rngs::OsRng, thread_rng, Rng, RngCore};

// ----------------------------------------------------------------------------
// Struct for passing init options from main.rs
// ----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub pool: Option<String>,
    pub usb_device: Option<String>,
    pub key_path: Option<PathBuf>,
    pub force: bool,
    pub auto_unlock: bool,
    pub offer_dracut_rebuild: bool,
}

// The on-disk TOML config we write atomically.
#[derive(Debug, Clone, Serialize)]
struct BeskarConfig {
    pool: String,
    usb_device: String,
    key_path: String,
    auto_unlock: bool,
}

// ----------------------------------------------------------------------------
// Public entrypoint
// ----------------------------------------------------------------------------

pub fn run_init(ui: &UX, timing: &Timing, opts: InitOptions) -> Result<()> {
    ui.banner();
    ui.info("Starting Beskar initialization workflow...");
    timing.pace(Pace::Info);

    // ------------------------------------------------------------------------
    // Step 1: Derive paths and perform sanity checks
    // ------------------------------------------------------------------------
    let pool = opts.pool.unwrap_or_else(|| "rpool".to_string());
    let usb_device = opts.usb_device.unwrap_or_else(|| "/dev/sdb1".to_string());

    let key_path = opts
        .key_path
        .unwrap_or_else(|| PathBuf::from(format!("/keys/{}.key", pool)));

    let config_path = PathBuf::from("/etc/zfs-beskar.toml");

    if key_path.exists() && !opts.force {
        ui.warn(&format!(
            "Key file already exists at {}. Use --force to overwrite.",
            key_path.display()
        ));
        return Ok(());
    }

    // Ensure parent dir for key exists, secure perms
    if let Some(parent) = key_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
            fs::set_permissions(parent, Permissions::from_mode(0o700)).ok();
        }
    }

    // ------------------------------------------------------------------------
    // Step 2: Generate and write the ZFS encryption key (RAW BYTES) atomically
    // ------------------------------------------------------------------------
    ui.info("Generating 32-byte ZFS key...");
    timing.pace(Pace::Info);

    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);

    // IMPORTANT: write RAW BYTES (not hex string)
    atomic_write_key(&key_path, &key[..], opts.force)?;
    // (atomic helper already sets secure perms, but enforce again defensively)
    fs::set_permissions(&key_path, Permissions::from_mode(0o400)).context("set key permissions")?;

    ui.success(&format!("Key file created at {}", key_path.display()));
    audit_log("INIT_KEY", &format!("Wrote key to {}", key_path.display()));

    // ------------------------------------------------------------------------
    // Step 3: Build and write TOML config atomically
    // ------------------------------------------------------------------------
    let cfg = BeskarConfig {
        pool: pool.clone(),
        usb_device: usb_device.clone(),
        key_path: key_path.display().to_string(),
        auto_unlock: opts.auto_unlock,
    };

    // atomic_write_toml expects a Serialize value + bool (force)
    atomic_write_toml(&config_path, &cfg, opts.force)?;
    fs::set_permissions(&config_path, Permissions::from_mode(0o600))
        .context("failed to set config permissions")?;

    ui.success(&format!("Config written to {}", config_path.display()));
    audit_log("INIT_CFG", &format!("Created {}", config_path.display()));

    // ------------------------------------------------------------------------
    // Step 4: Generate recovery key for safekeeping (print + audit)
    // ------------------------------------------------------------------------
    ui.info("Generating recovery key...");
    timing.pace(Pace::Info);

    let recovery = generate_recovery_key();
    ui.success(&format!("Recovery key: {}", recovery));
    audit_log("INIT_RECOVERY", "Generated recovery key");

    // ------------------------------------------------------------------------
    // Step 5: Optional Dracut rebuild (stub/placeholder)
    // ------------------------------------------------------------------------
    if opts.offer_dracut_rebuild {
        ui.info("Prepare to rebuild initramfs (Dracut)...");
        timing.pace(Pace::Info);

        // Future hook:
        // let cmd = crate::cmd::Cmd::new_allowlisted("/usr/bin/dracut", Duration::from_secs(30))?;
        // cmd.run(&["-f"], None)?;
        // audit_log("INIT_DRACUT", "Rebuilt initramfs with Beskar hooks");

        ui.warn("Dracut rebuild skipped (stubbed for now). Run `dracut -f` before reboot.");
    }

    // ------------------------------------------------------------------------
    // Step 6: Final summary
    // ------------------------------------------------------------------------
    ui.success("Initialization complete.");
    ui.info("Run `zbk doctor` to verify environment, then rebuild initramfs.");
    audit_log(
        "INIT_COMPLETE",
        "Beskar initialization completed successfully",
    );
    timing.pace(Pace::Critical);

    Ok(())
}

// ----------------------------------------------------------------------------
// Helper: Generate recovery key
// ----------------------------------------------------------------------------

/// Generate a random 24-character recovery key (A-Z, a-z, 0-9)
fn generate_recovery_key() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
}
