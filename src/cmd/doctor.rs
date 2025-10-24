// ============================================================================
// src/cmd/doctor.rs – Verify and repair Beskar environment
// ============================================================================

use crate::cmd::dracut_install;
use crate::cmd::init::{
    detect_initramfs_flavor, install_initramfs_tools_scripts, rebuild_initramfs, InitramfsFlavor,
    INITRAMFS_HOOK_PATH, INITRAMFS_LOCAL_TOP_PATH,
};
use crate::cmd::repair::{self, USB_MOUNT_UNIT};
use crate::cmd::Cmd;
use crate::config::ConfigFile;
use crate::dracut::{self, ModuleContext, ModulePaths, DEFAULT_MOUNTPOINT};
use crate::ui::{Pace, Timing, UX};
use crate::util::atomic::atomic_write_toml;
use crate::util::audit::audit_log;
use crate::util::binary::determine_binary_path;
use crate::zfs::Zfs;
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CONFIG_PATH: &str = "/etc/zfs-beskar.toml";
const UNLOCK_UNIT_NAME: &str = "beskar-unlock.service";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Pass,
    Fixed,
    Warn,
    Fail,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Pass => "[PASS]",
            Status::Fixed => "[FIXED]",
            Status::Warn => "[WARN]",
            Status::Fail => "[FAIL]",
        }
    }
}

struct ReportEntry {
    name: &'static str,
    status: Status,
    detail: String,
}

enum UnitVerification {
    Pass(String),
    Fixed(String),
    Warn(String),
    Fail(String),
}

pub fn run_doctor(ui: &UX, timing: &Timing) -> Result<()> {
    ui.banner();
    ui.phase("Armorer's Diagnostics // Clan Systems Report");

    let mut report: Vec<ReportEntry> = Vec::new();
    let mut need_initramfs_refresh = false;

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

    let initramfs_flavor = match detect_initramfs_flavor() {
        Ok(flavor) => {
            match &flavor {
                InitramfsFlavor::Dracut(module_dir) => log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Initramfs tooling",
                    Status::Pass,
                    format!("dracut detected; module path {}", module_dir.display()),
                ),
                InitramfsFlavor::InitramfsTools => log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Initramfs tooling",
                    Status::Pass,
                    "initramfs-tools detected".to_string(),
                ),
            }
            Some(flavor)
        }
        Err(err) => {
            log_entry(
                &mut report,
                ui,
                timing,
                "Initramfs tooling",
                Status::Warn,
                format!("{}", err),
            );
            None
        }
    };

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

    let key_path = Path::new(&cfg.usb.key_hex_path);
    let key_runtime_dir: PathBuf = key_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MOUNTPOINT));

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

    let zfs_timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
    let zfs_client = cfg
        .policy
        .zfs_path
        .as_ref()
        .map(|p| Zfs::with_path(p, zfs_timeout))
        .unwrap_or_else(|| Zfs::discover(zfs_timeout));

    let mut primary_encryption_root = primary_dataset.clone();
    match zfs_client.as_ref() {
        Ok(client) => match client.encryption_root(&primary_dataset) {
            Ok(root) => {
                if root != primary_dataset {
                    let detail =
                        format!("{} anchored at encryption root {}", primary_dataset, root);
                    if cfg
                        .policy
                        .datasets
                        .first()
                        .map(|d| d != &root)
                        .unwrap_or(true)
                    {
                        cfg.policy.datasets.retain(|d| d != &root);
                        cfg.policy.datasets.insert(0, root.clone());
                        match persist_config(&cfg) {
                            Ok(_) => log_entry(
                                &mut report,
                                ui,
                                timing,
                                "Encryption root",
                                Status::Fixed,
                                format!("{} (policy realigned)", detail),
                            ),
                            Err(err) => log_entry(
                                &mut report,
                                ui,
                                timing,
                                "Encryption root",
                                Status::Warn,
                                format!("{} (failed to persist: {})", detail, err),
                            ),
                        }
                    } else {
                        log_entry(
                            &mut report,
                            ui,
                            timing,
                            "Encryption root",
                            Status::Pass,
                            detail.clone(),
                        );
                    }
                    primary_encryption_root = root;
                } else {
                    log_entry(
                        &mut report,
                        ui,
                        timing,
                        "Encryption root",
                        Status::Pass,
                        format!("Encryption root confirmed as {}", root),
                    );
                    primary_encryption_root = root;
                }
            }
            Err(err) => log_entry(
                &mut report,
                ui,
                timing,
                "Encryption root",
                Status::Warn,
                format!(
                    "Unable to resolve encryption root for {}: {}",
                    primary_dataset, err
                ),
            ),
        },
        Err(err) => log_entry(
            &mut report,
            ui,
            timing,
            "Encryption root",
            Status::Warn,
            format!("Unable to initialize zfs client: {}", err),
        ),
    }

    let expected_keylocation = format!("file://{}", key_path.display());
    if key_path.is_absolute() {
        match zfs_client.as_ref() {
            Ok(client) => match client.get_property(&primary_encryption_root, "keylocation") {
                Ok(current) if current.eq_ignore_ascii_case(&expected_keylocation) => log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Keylocation",
                    Status::Pass,
                    format!("{}", current),
                ),
                Ok(_) => match client.set_property(
                    &primary_encryption_root,
                    "keylocation",
                    &expected_keylocation,
                ) {
                    Ok(_) => log_entry(
                        &mut report,
                        ui,
                        timing,
                        "Keylocation",
                        Status::Fixed,
                        format!("Aligned to {}", expected_keylocation),
                    ),
                    Err(err) => log_entry(
                        &mut report,
                        ui,
                        timing,
                        "Keylocation",
                        Status::Warn,
                        format!(
                            "Unable to set keylocation to {}: {}",
                            expected_keylocation, err
                        ),
                    ),
                },
                Err(err) => log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Keylocation",
                    Status::Warn,
                    format!("Unable to query keylocation: {}", err),
                ),
            },
            Err(err) => log_entry(
                &mut report,
                ui,
                timing,
                "Keylocation",
                Status::Warn,
                format!("ZFS unavailable to verify keylocation: {}", err),
            ),
        }
    } else {
        log_entry(
            &mut report,
            ui,
            timing,
            "Keylocation",
            Status::Warn,
            format!(
                "usb.key_hex_path ({}) is not absolute; update config to file:///path",
                cfg.usb.key_hex_path
            ),
        );
    }

    let binary_path = match determine_binary_path(Some(&cfg)) {
        Ok(path) => path,
        Err(err) => {
            log_entry(
                &mut report,
                ui,
                timing,
                "Binary path",
                Status::Fail,
                format!("Unable to resolve zfs_beskar_key binary: {}", err),
            );
            summarize(&report, ui, timing)?;
            return Err(anyhow!("Missing zfs_beskar_key binary"));
        }
    };
    let binary_path_string = binary_path.to_string_lossy().to_string();
    match cfg.policy.binary_path.as_deref() {
        Some(existing) if existing == binary_path_string => log_entry(
            &mut report,
            ui,
            timing,
            "Binary path",
            Status::Pass,
            format!("Using {}", binary_path_string),
        ),
        _ => {
            cfg.policy.binary_path = Some(binary_path_string.clone());
            match persist_config(&cfg) {
                Ok(_) => {
                    need_initramfs_refresh = true;
                    log_entry(
                        &mut report,
                        ui,
                        timing,
                        "Binary path",
                        Status::Fixed,
                        format!("Recorded {}", binary_path_string),
                    );
                }
                Err(err) => log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Binary path",
                    Status::Warn,
                    format!("Failed to record {}: {}", binary_path_string, err),
                ),
            }
        }
    }

    // ---------------------------------------------------------------------
    // Verify key material
    // ---------------------------------------------------------------------
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
                                        need_initramfs_refresh = true;
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

    // Ensure runtime mount directory exists
    let run_dir = key_runtime_dir.as_path();
    if run_dir.is_absolute() {
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
    } else {
        log_entry(
            &mut report,
            ui,
            timing,
            "Runtime directory",
            Status::Warn,
            format!(
                "Runtime mount path {} is relative; adjust usb.key_hex_path",
                key_runtime_dir.display()
            ),
        );
    }

    // ---------------------------------------------------------------------
    // Initramfs integration
    // ---------------------------------------------------------------------
    match &initramfs_flavor {
        Some(InitramfsFlavor::Dracut(module_dir)) => {
            let mountpoint_owned = key_runtime_dir.to_string_lossy().into_owned();
            let key_path_owned = key_path.to_string_lossy().into_owned();
            let module_paths = ModulePaths::new(module_dir);
            let ctx = ModuleContext {
                mountpoint: &mountpoint_owned,
                key_path: &key_path_owned,
            };

            let module_exists = module_paths.root.exists();

            let needs_reinstall = if module_exists {
                match dracut::module_is_current(&module_paths, &ctx) {
                    Ok(true) => {
                        log_entry(
                            &mut report,
                            ui,
                            timing,
                            "Initramfs module",
                            Status::Pass,
                            format!("{} ready", module_dir.display()),
                        );
                        false
                    }
                    Ok(false) => true,
                    Err(err) => {
                        log_entry(
                            &mut report,
                            ui,
                            timing,
                            "Initramfs module",
                            Status::Warn,
                            format!("Unable to verify dracut module: {}", err),
                        );
                        true
                    }
                }
            } else {
                true
            };

            if needs_reinstall {
                match dracut_install::install_for_dataset(
                    ui,
                    &cfg,
                    Some(&primary_encryption_root),
                    Some(InitramfsFlavor::Dracut(module_dir.clone())),
                ) {
                    Ok(_) => {
                        let detail = if module_exists {
                            "Dracut module refreshed and dracut -f executed for Ubuntu-style unlock.".to_string()
                        } else {
                            format!(
                                "Dracut module installed at {} with dracut -f.",
                                module_dir.display()
                            )
                        };
                        log_entry(
                            &mut report,
                            ui,
                            timing,
                            "Initramfs module",
                            Status::Fixed,
                            detail,
                        );
                    }
                    Err(err) => log_entry(
                        &mut report,
                        ui,
                        timing,
                        "Initramfs module",
                        Status::Fail,
                        format!("Unable to install dracut module: {}", err),
                    ),
                }
            }
        }
        Some(InitramfsFlavor::InitramfsTools) => {
            let hook_exists = Path::new(INITRAMFS_HOOK_PATH).exists();
            let local_top_exists = Path::new(INITRAMFS_LOCAL_TOP_PATH).exists();
            let ubuntu_style = hook_exists
                && local_top_exists
                && fs::read_to_string(INITRAMFS_LOCAL_TOP_PATH)
                    .map(|content| content.contains("zfs load-key -a"))
                    .unwrap_or(false);

            if hook_exists && local_top_exists && ubuntu_style {
                log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Initramfs module",
                    Status::Pass,
                    "initramfs-tools scripts present".to_string(),
                );
            } else {
                match install_initramfs_tools_scripts(key_runtime_dir.as_path(), key_path, ui) {
                    Ok(_) => {
                        need_initramfs_refresh = true;
                        log_entry(
                            &mut report,
                            ui,
                            timing,
                            "Initramfs module",
                            Status::Fixed,
                            "initramfs-tools scripts installed.".to_string(),
                        );
                    }
                    Err(err) => log_entry(
                        &mut report,
                        ui,
                        timing,
                        "Initramfs module",
                        Status::Fail,
                        format!("Unable to install initramfs-tools scripts: {}", err),
                    ),
                }
            }
        }
        None => {
            log_entry(
                &mut report,
                ui,
                timing,
                "Initramfs module",
                Status::Warn,
                "Initramfs tooling unavailable; auto-unlock integration skipped.".to_string(),
            );
        }
    }

    // ---------------------------------------------------------------------
    // Systemd units
    // ---------------------------------------------------------------------
    if repair::units_exist() {
        match repair::unit_exec_matches(&binary_path) {
            Ok(true) => log_entry(
                &mut report,
                ui,
                timing,
                "Systemd units",
                Status::Pass,
                format!("{} & beskar-unlock.service present.", USB_MOUNT_UNIT),
            ),
            Ok(false) => match repair::install_units(ui, &cfg, &binary_path) {
                Ok(_) => log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Systemd units",
                    Status::Fixed,
                    format!("Updated beskar-unlock ExecStart to {}.", binary_path_string),
                ),
                Err(err) => log_entry(
                    &mut report,
                    ui,
                    timing,
                    "Systemd units",
                    Status::Fail,
                    format!("Unable to refresh units: {}", err),
                ),
            },
            Err(err) => log_entry(
                &mut report,
                ui,
                timing,
                "Systemd units",
                Status::Warn,
                format!("Unable to verify unit ExecStart: {}", err),
            ),
        }
    } else {
        match repair::install_units(ui, &cfg, &binary_path) {
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

    match verify_systemd_units(ui, &cfg, &binary_path) {
        UnitVerification::Pass(detail) => log_entry(
            &mut report,
            ui,
            timing,
            "Systemd verification",
            Status::Pass,
            detail,
        ),
        UnitVerification::Fixed(detail) => log_entry(
            &mut report,
            ui,
            timing,
            "Systemd verification",
            Status::Fixed,
            detail,
        ),
        UnitVerification::Warn(detail) => log_entry(
            &mut report,
            ui,
            timing,
            "Systemd verification",
            Status::Warn,
            detail,
        ),
        UnitVerification::Fail(detail) => log_entry(
            &mut report,
            ui,
            timing,
            "Systemd verification",
            Status::Fail,
            detail,
        ),
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
    match zfs_client.and_then(|client| client.is_encrypted(&primary_encryption_root)) {
        Ok(true) => log_entry(
            &mut report,
            ui,
            timing,
            "Dataset encryption",
            Status::Pass,
            format!("{} is encrypted", primary_encryption_root),
        ),
        Ok(false) => log_entry(
            &mut report,
            ui,
            timing,
            "Dataset encryption",
            Status::Warn,
            format!("{} reports encryption=off", primary_encryption_root),
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
    if need_initramfs_refresh {
        match initramfs_flavor
            .as_ref()
            .map(|flavor| rebuild_initramfs(ui, flavor))
        {
            Some(Ok(())) => log_entry(
                &mut report,
                ui,
                timing,
                "Initramfs",
                Status::Fixed,
                "Initramfs image refreshed.".to_string(),
            ),
            Some(Err(err)) => log_entry(
                &mut report,
                ui,
                timing,
                "Initramfs",
                Status::Warn,
                format!(
                    "Initramfs rebuild failed: {} (try `zfs_beskar_key install-dracut` && `sudo dracut -f`).",
                    err
                ),
            ),
            None => log_entry(
                &mut report,
                ui,
                timing,
                "Initramfs",
                Status::Warn,
                "Initramfs tooling not detected; rebuild skipped — rerun once dracut is available.".to_string(),
            ),
        }
        if matches!(initramfs_flavor, Some(InitramfsFlavor::Dracut(_))) {
            ui.note(
                "Follow-up: `zfs_beskar_key install-dracut` then `sudo dracut -f` ensure the refreshed module lands in initramfs.",
            );
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
        Status::Pass => ui.success(&format!("{} {}", status.label(), detail)),
        Status::Fixed => ui.success(&format!("{} {}", status.label(), detail)),
        Status::Warn => ui.warn(&format!("{} {}", status.label(), detail)),
        Status::Fail => ui.error(&format!("{} {}", status.label(), detail)),
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
        ui.note(&format!("Warnings tallied: {}", warn_details.join(" | ")));
    }
    if !fail_details.is_empty() {
        ui.warn(&format!(
            "Failures demanding attention: {}",
            fail_details.join(" | ")
        ));
    }

    if fails > 0 {
        Err(anyhow!("Diagnostics uncovered blocking issues"))
    } else {
        ui.success("Diagnostics complete. The armour's story is recorded. This is the Way.");
        Ok(())
    }
}

fn verify_systemd_units(ui: &UX, cfg: &ConfigFile, binary_path: &Path) -> UnitVerification {
    match run_unit_verification(&[USB_MOUNT_UNIT, UNLOCK_UNIT_NAME]) {
        Ok(_) => {
            UnitVerification::Pass("systemd-analyze verify clean for Beskar units.".to_string())
        }
        Err(err) => {
            let err_msg = err.to_string();
            if err_msg.contains("systemd-analyze not found") {
                return UnitVerification::Warn(
                    "systemd-analyze missing; skipping verification.".to_string(),
                );
            }
            match repair::install_units(ui, cfg, binary_path) {
                Ok(_) => match run_unit_verification(&[USB_MOUNT_UNIT, UNLOCK_UNIT_NAME]) {
                    Ok(_) => UnitVerification::Fixed(format!(
                        "Reinstalled units after verification error: {}",
                        err_msg
                    )),
                    Err(err2) => UnitVerification::Fail(format!(
                        "systemd-analyze verify still failing: {}",
                        err2
                    )),
                },
                Err(install_err) => UnitVerification::Fail(format!(
                    "Verification failed: {err_msg}; reinstall error: {install_err}"
                )),
            }
        }
    }
}

fn run_unit_verification(units: &[&str]) -> Result<()> {
    let analyzer = systemd_analyze(Duration::from_secs(5))?;
    for unit in units {
        let output = analyzer.run(&["verify", unit], None)?;
        if output.status != 0 {
            let msg = if output.stderr.trim().is_empty() {
                output.stdout.trim().to_string()
            } else {
                output.stderr.trim().to_string()
            };
            return Err(anyhow!("{} verification failed: {}", unit, msg));
        }
    }
    Ok(())
}

fn systemd_analyze(timeout: Duration) -> Result<Cmd> {
    for candidate in ["/bin/systemd-analyze", "/usr/bin/systemd-analyze"] {
        if Path::new(candidate).exists() {
            return Cmd::new_allowlisted(candidate, timeout);
        }
    }
    Err(anyhow!("systemd-analyze not found"))
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
    let usb = cmd.run(&["is-enabled", USB_MOUNT_UNIT], None)?;
    let unlock = cmd.run(&["is-enabled", "beskar-unlock.service"], None)?;

    if usb.status == 0 && unlock.status == 0 {
        return Ok(None);
    }

    repair::ensure_units_enabled(ui)?;
    Ok(Some(format!(
        "Enabled {} & beskar-unlock.service via systemctl.",
        USB_MOUNT_UNIT
    )))
}
