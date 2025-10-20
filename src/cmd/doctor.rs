// ============================================================================
// src/cmd/doctor.rs – Verify and repair Beskar environment
// ============================================================================

use crate::cmd::init::{
    install_dracut_module, rebuild_initramfs, DRACUT_MODULE_DIR, DRACUT_SCRIPT_NAME,
    DRACUT_SETUP_NAME,
};
use crate::cmd::repair;
use crate::cmd::Cmd;
use crate::config::ConfigFile;
use crate::ui::{Pace, Timing, UX};
use crate::util::atomic::atomic_write_toml;
use crate::util::audit::audit_log;
use crate::zfs::Zfs;
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

const CONFIG_PATH: &str = "/etc/zfs-beskar.toml";
const KEY_RUNTIME_DIR: &str = "/run/beskar";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Pass,
    Fixed,
    Warn,
    Fail,
}

impl Status {
    fn glyph(self) -> &'static str {
        match self {
            Status::Pass => "✴",
            Status::Fixed => "✷",
            Status::Warn => "⚠",
            Status::Fail => "✖",
        }
    }
}

struct ReportEntry {
    name: &'static str,
    status: Status,
    detail: String,
}

pub fn run_doctor(ui: &UX, timing: &Timing) -> Result<()> {
    ui.banner();
    ui.phase("Holoforge Diagnostics // Clan Systems Report");

    let mut report: Vec<ReportEntry> = Vec::new();
    let mut need_dracut = false;

    // ---------------------------------------------------------------------
    // Check essential binaries
    // ---------------------------------------------------------------------
    let binary_checks: &[(&str, &[&str], &str)] = &[
        (
            "zfs",
            &[
                "/sbin/zfs",
                "/usr/sbin/zfs",
                "/usr/local/sbin/zfs",
                "/bin/zfs",
            ],
            "Install zfsutils-linux and ensure modules are loaded.",
        ),
        (
            "zpool",
            &[
                "/sbin/zpool",
                "/usr/sbin/zpool",
                "/usr/local/sbin/zpool",
                "/bin/zpool",
            ],
            "Install zfsutils-linux.",
        ),
        (
            "dracut",
            &["/usr/bin/dracut", "/usr/sbin/dracut"],
            "Install dracut so initramfs can be rebuilt.",
        ),
        (
            "systemctl",
            &["/bin/systemctl", "/usr/bin/systemctl"],
            "Ensure systemd is available.",
        ),
    ];

    for (name, candidates, remedy) in binary_checks {
        match find_binary(candidates) {
            Some(path) => log_entry(
                &mut report,
                ui,
                timing,
                name,
                Status::Pass,
                format!("Found at {}", path),
            ),
            None => log_entry(
                &mut report,
                ui,
                timing,
                name,
                Status::Fail,
                remedy.to_string(),
            ),
        }
    }

    // ---------------------------------------------------------------------
    // Load configuration
    // ---------------------------------------------------------------------
    let config_path = Path::new(CONFIG_PATH);
    if !config_path.exists() {
        log_entry(
            &mut report,
            ui,
            timing,
            "Config file",
            Status::Fail,
            format!("Missing {} – run `zfs_beskar_key init` first.", CONFIG_PATH),
        );
        summarize(&report, ui, timing)?;
        return Err(anyhow!("Beskar config missing"));
    }

    let mut cfg = match ConfigFile::load(config_path) {
        Ok(cfg) => {
            log_entry(
                &mut report,
                ui,
                timing,
                "Config file",
                Status::Pass,
                format!("Loaded {}", CONFIG_PATH),
            );
            cfg
        }
        Err(err) => {
            log_entry(
                &mut report,
                ui,
                timing,
                "Config file",
                Status::Fail,
                format!("Unable to parse config: {}", err),
            );
            summarize(&report, ui, timing)?;
            return Err(anyhow!("Invalid config"));
        }
    };

    if cfg.policy.datasets.is_empty() {
        log_entry(
            &mut report,
            ui,
            timing,
            "Dataset roster",
            Status::Warn,
            "No datasets listed in policy.datasets – update config.".to_string(),
        );
    } else {
        log_entry(
            &mut report,
            ui,
            timing,
            "Dataset roster",
            Status::Pass,
            cfg.policy.datasets.join(", "),
        );
    }
    let primary_dataset = cfg
        .policy
        .datasets
        .first()
        .cloned()
        .unwrap_or_else(|| "rpool/ROOT".to_string());

    // ---------------------------------------------------------------------
    // Verify key material
    // ---------------------------------------------------------------------
    let key_path = Path::new(&cfg.usb.key_hex_path);
    if !key_path.exists() {
        log_entry(
            &mut report,
            ui,
            timing,
            "USB key file",
            Status::Warn,
            format!(
                "Key file {} missing – insert the token and rerun `zfs_beskar_key init --force`.",
                key_path.display()
            ),
        );
    } else {
        match fs::read_to_string(key_path) {
            Ok(contents) => {
                let cleaned: String = contents.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                if cleaned.len() != 64 {
                    log_entry(
                        &mut report,
                        ui,
                        timing,
                        "USB key file",
                        Status::Warn,
                        "Key is not 64 hex characters – forge a new token.".to_string(),
                    );
                } else {
                    match hex::decode(&cleaned) {
                        Ok(bytes) => {
                            let actual_sha = hex::encode(Sha256::digest(&bytes));
                            match cfg.usb.expected_sha256.as_ref() {
                                Some(expected) if expected.eq_ignore_ascii_case(&actual_sha) => {
                                    log_entry(
                                        &mut report,
                                        ui,
                                        timing,
                                        "USB checksum",
                                        Status::Pass,
                                        "SHA-256 matches recorded expectation.".to_string(),
                                    );
                                }
                                _ => {
                                    cfg.usb.expected_sha256 = Some(actual_sha.clone());
                                    if let Err(err) = persist_config(&cfg) {
                                        log_entry(
                                            &mut report,
                                            ui,
                                            timing,
                                            "USB checksum",
                                            Status::Warn,
                                            format!(
                                                "Checksum mismatch ({}). Re-run init --force.",
                                                err
                                            ),
                                        );
                                    } else {
                                        need_dracut = true;
                                        log_entry(
                                            &mut report,
                                            ui,
                                            timing,
                                            "USB checksum",
                                            Status::Fixed,
                                            "Updated config.expected_sha256 to match token."
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                        }
                        Err(err) => log_entry(
                            &mut report,
                            ui,
                            timing,
                            "USB key file",
                            Status::Warn,
                            format!("Key contents are not valid hex: {}", err),
                        ),
                    }
                }
            }
            Err(err) => log_entry(
                &mut report,
                ui,
                timing,
                "USB key file",
                Status::Warn,
                format!("Unable to read {}: {}", key_path.display(), err),
            ),
        }
    }

    // Ensure /run/beskar exists
    let run_dir = Path::new(KEY_RUNTIME_DIR);
    if !run_dir.exists() {
        if let Err(err) = fs::create_dir_all(run_dir) {
            log_entry(
                &mut report,
                ui,
                timing,
                "Runtime directory",
                Status::Warn,
                format!("Failed to create {}: {}", run_dir.display(), err),
            );
        } else {
            log_entry(
                &mut report,
                ui,
                timing,
                "Runtime directory",
                Status::Fixed,
                format!("Created {}", run_dir.display()),
            );
        }
    } else {
        log_entry(
            &mut report,
            ui,
            timing,
            "Runtime directory",
            Status::Pass,
            format!("{} present", run_dir.display()),
        );
    }

    // ---------------------------------------------------------------------
    // Dracut module
    // ---------------------------------------------------------------------
    let module_dir = Path::new(DRACUT_MODULE_DIR);
    let script_path = module_dir.join(DRACUT_SCRIPT_NAME);
    let setup_path = module_dir.join(DRACUT_SETUP_NAME);
    if module_dir.exists() && script_path.exists() && setup_path.exists() {
        log_entry(
            &mut report,
            ui,
            timing,
            "Dracut module",
            Status::Pass,
            format!("{} ready", DRACUT_MODULE_DIR),
        );
    } else {
        match install_dracut_module(&primary_dataset, config_path, ui) {
            Ok(_) => {
                need_dracut = true;
                log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Dracut module",
                    Status::Fixed,
                    "Module reinstalled.".to_string(),
                );
            }
            Err(err) => {
                log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Dracut module",
                    Status::Fail,
                    format!("Unable to install module: {}", err),
                );
            }
        }
    }

    // ---------------------------------------------------------------------
    // Systemd units
    // ---------------------------------------------------------------------
    if repair::units_exist() {
        log_entry(
            &mut report,
            ui,
            timing,
            "Systemd units",
            Status::Pass,
            "beskar-usb.mount & beskar-unlock.service present.".to_string(),
        );
    } else {
        match repair::install_units(ui, &cfg) {
            Ok(_) => {
                log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Systemd units",
                    Status::Fixed,
                    "Regenerated Beskar unit files.".to_string(),
                );
            }
            Err(err) => {
                log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Systemd units",
                    Status::Fail,
                    format!("Unable to install units: {}", err),
                );
            }
        }
    }

    match ensure_units_enabled(ui) {
        Ok(msg) => {
            if let Some(detail) = msg {
                log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Systemctl enable",
                    Status::Fixed,
                    detail,
                );
            } else {
                log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Systemctl enable",
                    Status::Pass,
                    "Units already enabled.".to_string(),
                );
            }
        }
        Err(err) => log_entry(
            &mut report,
            ui,
            timing,
            "Systemctl enable",
            Status::Warn,
            format!("Unable to enable units: {}", err),
        ),
    }

    // ---------------------------------------------------------------------
    // Dataset sanity via zfs
    // ---------------------------------------------------------------------
    let zfs_timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
    let zfs = cfg
        .policy
        .zfs_path
        .as_ref()
        .map(|p| Zfs::with_path(p, zfs_timeout))
        .unwrap_or_else(|| Zfs::discover(zfs_timeout));

    match zfs.and_then(|client| client.is_encrypted(&primary_dataset)) {
        Ok(true) => log_entry(
            &mut report,
            ui,
            timing,
            "Dataset encryption",
            Status::Pass,
            format!("{} is encrypted", primary_dataset),
        ),
        Ok(false) => log_entry(
            &mut report,
            ui,
            timing,
            "Dataset encryption",
            Status::Warn,
            format!("{} reports encryption=off", primary_dataset),
        ),
        Err(err) => log_entry(
            &mut report,
            ui,
            timing,
            "Dataset encryption",
            Status::Warn,
            format!("Unable to query dataset: {}", err),
        ),
    }

    // ---------------------------------------------------------------------
    // Rebuild initramfs if required
    // ---------------------------------------------------------------------
    if need_dracut {
        match rebuild_initramfs(ui) {
            Ok(_) => log_entry(
                &mut report,
                ui,
                timing,
                "Initramfs",
                Status::Fixed,
                "dracut -f completed.".to_string(),
            ),
            Err(err) => log_entry(
                &mut report,
                ui,
                timing,
                "Initramfs",
                Status::Warn,
                format!("dracut failed: {}", err),
            ),
        }
    }

    summarize(&report, ui, timing)?;
    audit_log("DOCTOR", "Environment diagnostics completed");
    Ok(())
}

fn log_entry(
    report: &mut Vec<ReportEntry>,
    ui: &UX,
    timing: &Timing,
    name: &'static str,
    status: Status,
    detail: String,
) {
    match status {
        Status::Pass => ui.success(&format!("{} {}", status.glyph(), detail)),
        Status::Fixed => ui.success(&format!("{} {}", status.glyph(), detail)),
        Status::Warn => ui.warn(&format!("{} {}", status.glyph(), detail)),
        Status::Fail => ui.error(&format!("{} {}", status.glyph(), detail)),
    }
    timing.pace(match status {
        Status::Pass | Status::Fixed => Pace::Info,
        Status::Warn => Pace::Prompt,
        Status::Fail => Pace::Error,
    });
    report.push(ReportEntry {
        name,
        status,
        detail,
    });
}

fn summarize(report: &[ReportEntry], ui: &UX, timing: &Timing) -> Result<()> {
    let mut passes = 0;
    let mut fixed = 0;
    let mut warns = 0;
    let mut fails = 0;
    let mut warn_details = Vec::new();
    let mut fail_details = Vec::new();

    for entry in report {
        match entry.status {
            Status::Pass => passes += 1,
            Status::Fixed => fixed += 1,
            Status::Warn => {
                warns += 1;
                warn_details.push(format!("{}: {}", entry.name, entry.detail));
            }
            Status::Fail => {
                fails += 1;
                fail_details.push(format!("{}: {}", entry.name, entry.detail));
            }
        }
    }

    ui.data_panel(
        "Diagnostic Summary",
        &[
            ("Pass", passes.to_string()),
            ("Fixed", fixed.to_string()),
            ("Warn", warns.to_string()),
            ("Fail", fails.to_string()),
        ],
    );
    timing.pace(Pace::Info);

    if !warn_details.is_empty() {
        ui.note(&format!("Warnings: {}", warn_details.join(" | ")));
    }
    if !fail_details.is_empty() {
        ui.warn(&format!("Failures: {}", fail_details.join(" | ")));
    }

    if fails > 0 {
        Err(anyhow!("Diagnostics uncovered blocking issues"))
    } else {
        ui.success("Diagnostics complete. This is the Way.");
        Ok(())
    }
}

fn find_binary(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .map(|p| *p)
        .find(|p| Path::new(p).exists())
        .map(|p| p.to_string())
}

fn persist_config(cfg: &ConfigFile) -> Result<()> {
    let path = cfg.path.as_path();
    atomic_write_toml(path, cfg, true)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn ensure_units_enabled(ui: &UX) -> Result<Option<String>> {
    let systemctl_path = find_binary(&["/bin/systemctl", "/usr/bin/systemctl"])
        .ok_or_else(|| anyhow!("systemctl not found on PATH"))?;
    let cmd = Cmd::new_allowlisted(systemctl_path.clone(), Duration::from_secs(5))?;
    let usb = cmd.run(&["is-enabled", "beskar-usb.mount"], None)?;
    let unlock = cmd.run(&["is-enabled", "beskar-unlock.service"], None)?;

    if usb.status == 0 && unlock.status == 0 {
        return Ok(None);
    }

    repair::ensure_units_enabled(ui)?;
    Ok(Some(
        "Enabled beskar-usb.mount & beskar-unlock.service via systemctl.".to_string(),
    ))
}
