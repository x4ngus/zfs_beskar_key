// ============================================================================
// src/cmd/doctor.rs – Verify and repair Beskar environment
// ============================================================================

use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::ui::{Pace, Timing, UX};
use crate::util::audit::audit_log;

/// Performs integrity checks on configuration, dracut modules, and key paths.
/// Non-destructive: reports problems, suggests fixes, but never overwrites.
pub fn run_doctor(ui: &UX, timing: &Timing) -> Result<()> {
    ui.banner();
    ui.info("Running environment diagnostics...");
    timing.pace(Pace::Info);

    let checks = [
        "/etc/zfs-beskar.toml",
        "/usr/lib/dracut/modules.d/95beskar/beskar-unlock.sh",
    ];

    for path in &checks {
        let p = Path::new(path);
        if !p.exists() {
            ui.warn(&format!("Missing: {}", path));
            timing.pace(Pace::Error);
        } else {
            let meta = p.metadata()?;
            let mode = meta.permissions().mode() & 0o777;
            ui.info(&format!("Found {} (mode {:o})", path, mode));
            timing.pace(Pace::Info);
        }
    }

    // Check TOML content sanity
    let cfg_path = Path::new("/etc/zfs-beskar.toml");
    if cfg_path.exists() {
        let toml = fs::read_to_string(cfg_path).context("Unable to read configuration file")?;
        if !toml.contains("pool") || !toml.contains("key_path") {
            ui.error("Configuration missing required fields (pool/key_path).");
            timing.pace(Pace::Error);
        } else {
            ui.success("Configuration syntax appears valid.");
            timing.pace(Pace::Info);
        }
    }

    // Optional: verify key path exists
    if let Ok(toml) = fs::read_to_string(cfg_path) {
        if let Some(line) = toml
            .lines()
            .find(|l| l.trim_start().starts_with("key_path"))
        {
            if let Some(path) = line.split('=').nth(1) {
                let key_trim = path.trim().trim_matches('"');
                if Path::new(key_trim).exists() {
                    ui.success(&format!("Key file exists at {key_trim}."));
                } else {
                    ui.warn(&format!("Key file missing: {key_trim}."));
                }
            }
        }
    }

    ui.success("Diagnostics complete.");
    audit_log("DOCTOR", "Environment diagnostics completed");
    timing.pace(Pace::Critical);
    Ok(())
}
