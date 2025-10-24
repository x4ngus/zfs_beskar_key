// ============================================================================
// src/cmd/init.rs – Flagship initialization workflow
// (Formats USB token, forges key material, writes config & dracut module)
// ============================================================================

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rand::distributions::Alphanumeric;
use rand::RngCore;
use rand::{rngs::OsRng, thread_rng, Rng};
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata, Permissions};
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::{tempdir, NamedTempFile};
use zeroize::Zeroizing;

use crate::cmd::{Cmd, OutputData};
use crate::config::{ConfigFile, CryptoCfg, Fallback, Policy, Usb};
use crate::ui::{Pace, Timing, UX};
use crate::util::atomic::atomic_write_toml;
use crate::util::audit::audit_log;
use crate::util::binary::determine_binary_path;
use crate::zfs::Zfs;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::collections::HashMap;

const BESKAR_LABEL: &str = crate::dracut::BESKAR_TOKEN_LABEL;
const DEFAULT_CONFIG_PATH: &str = "/etc/zfs-beskar.toml";
const DEFAULT_ZFS_BIN: &str = "/sbin/zfs";
const DEFAULT_TIMEOUT: u64 = 10;
pub(crate) const INITRAMFS_HOOK_PATH: &str = "/etc/initramfs-tools/hooks/zz-beskar";
pub(crate) const INITRAMFS_LOCAL_TOP_PATH: &str = "/etc/initramfs-tools/scripts/local-top/beskar";
const PARTED_BINARIES: &[&str] = &["/sbin/parted", "/usr/sbin/parted", "/usr/bin/parted"];
const MKFS_BINARIES: &[&str] = &[
    "/sbin/mkfs.ext4",
    "/usr/sbin/mkfs.ext4",
    "/usr/bin/mkfs.ext4",
];
const BLKID_BINARIES: &[&str] = &["/sbin/blkid", "/usr/sbin/blkid", "/usr/bin/blkid"];
const LSBLK_BINARIES: &[&str] = &["/bin/lsblk", "/usr/bin/lsblk"];
const MOUNT_BINARIES: &[&str] = &["/bin/mount", "/usr/bin/mount"];
const UMOUNT_BINARIES: &[&str] = &["/bin/umount", "/usr/bin/umount"];
const UDEVADM_BINARIES: &[&str] = &["/sbin/udevadm", "/usr/sbin/udevadm", "/usr/bin/udevadm"];

#[derive(Debug, Clone)]
pub(crate) enum InitramfsFlavor {
    Dracut(PathBuf),
    InitramfsTools,
}

pub(crate) fn detect_initramfs_flavor() -> Result<InitramfsFlavor> {
    let dracut_paths = ["/usr/bin/dracut", "/usr/sbin/dracut"];
    let dracut_available = dracut_paths.iter().any(|p| Path::new(p).exists());
    if dracut_available {
        return Ok(InitramfsFlavor::Dracut(
            crate::dracut::preferred_module_dir(),
        ));
    }

    if Path::new("/usr/sbin/update-initramfs").exists() {
        return Ok(InitramfsFlavor::InitramfsTools);
    }

    Err(anyhow!(
        "No supported initramfs tooling detected (neither dracut nor initramfs-tools present)."
    ))
}

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
    pub confirm_each_phase: bool,
}

// ----------------------------------------------------------------------------
// Public entrypoint
// ----------------------------------------------------------------------------

pub fn run_init(ui: &UX, timing: &Timing, opts: InitOptions) -> Result<()> {
    ui.banner();
    begin_phase(
        ui,
        "Armorer's Preparation // Tempering Beskar",
        opts.confirm_each_phase,
    )?;
    ui.info(
        "You lay your beskar tribute before me; I will temper it into a ward for your dataset. Speak each detail as I call for it.",
    );
    timing.pace(Pace::Info);

    let binary_path = determine_binary_path(None)?;

    let zfs = Zfs::discover(Duration::from_secs(DEFAULT_TIMEOUT))
        .context("detect zfs binary for encryption checks")?;
    let detected_root = match zfs.dataset_with_mountpoint("/") {
        Ok(ds) => ds,
        Err(err) => {
            ui.warn(&format!(
                "Unable to auto-detect dataset mounted at / ({}). Defaulting to rpool/ROOT.",
                err
            ));
            None
        }
    };

    let target_dataset = if let Some(dataset) = opts.pool.clone() {
        dataset
    } else if let Some(auto) = detected_root.clone() {
        ui.info(&format!(
            "Detected dataset {} mounted at /. Using it as the forge target.",
            auto
        ));
        auto
    } else {
        "rpool/ROOT".to_string()
    };

    let enc_root = match zfs.encryption_root(&target_dataset) {
        Ok(root) if !root.trim().is_empty() => root,
        Ok(_) => target_dataset.clone(),
        Err(err) => {
            ui.warn(&format!(
                "Unable to identify encryption root for {} ({}). Falling back to the specified dataset.",
                target_dataset, err
            ));
            target_dataset.clone()
        }
    };

    if enc_root != target_dataset {
        ui.info(&format!(
            "Dataset {} draws its ward from encryption root {}.",
            target_dataset, enc_root
        ));
    }

    if !zfs
        .is_encrypted(&enc_root)
        .with_context(|| format!("verify encryption status of {}", enc_root))?
    {
        return Err(anyhow!(
            "Dataset {} is not encrypted — no key forge required.",
            enc_root
        ));
    }

    if !zfs
        .is_unlocked(&enc_root)
        .with_context(|| format!("check keystatus for {}", enc_root))?
    {
        return Err(anyhow!(
            "Encryption root {} is sealed. Unlock it before attempting a new forge.",
            enc_root
        ));
    }

    let key_basename = sanitize_key_name(&enc_root);

    let key_path = opts
        .key_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("/run/beskar/{}.keyhex", key_basename)));
    let key_filename = key_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| anyhow!("key path must include a file name"))?;
    let key_mount_dir = key_path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/run/beskar".to_string());
    let key_location_uri = format!("file://{}", key_path.display());

    let existing_key = read_existing_key(&key_path)?;

    let usb_target = match opts.usb_device.clone() {
        Some(dev) => dev,
        None => select_usb_device(ui, opts.confirm_each_phase)?,
    };

    let (usb_disk, usb_partition) = derive_device_layout(&usb_target)?;

    dismantle_mounts(&usb_disk, ui)?;
    dismantle_mounts(&usb_partition, ui)?;

    ui.data_panel(
        "Armorer's Ledger",
        &[
            ("Dataset", target_dataset.clone()),
            ("Encryption Root", enc_root.clone()),
            ("USB Disk", usb_disk.clone()),
            ("USB Partition", usb_partition.clone()),
            ("Key Mount Path", key_mount_dir.clone()),
            ("Key File", key_filename.clone()),
            ("Auto-Unlock", flag_label(opts.auto_unlock)),
        ],
    );
    timing.pace(Pace::Info);

    begin_phase(
        ui,
        "Material Survey // Inspect Alloy",
        opts.confirm_each_phase,
    )?;
    report_usb_target(ui, &usb_disk);
    if usb_partition != usb_disk {
        report_usb_target(ui, &usb_partition);
    }
    timing.pace(Pace::Info);

    let mut effective_force = opts.force;

    if effective_force {
        ui.warn(&format!(
            "Override accepted. I will scour {} clean so the tribute accepts a new inscription.",
            usb_disk
        ));
        wipe_usb_token(&usb_disk, &usb_partition, ui)?;
        settle_udev(ui)?;
        audit_log(
            "INIT_USB_WIPE",
            &format!("disk={} partition={}", usb_disk, usb_partition),
        );
    } else {
        loop {
            match ensure_beskar_partition(&usb_partition, ui) {
                Ok(_) => break,
                Err(err) => {
                    if !opts.confirm_each_phase {
                        return Err(err);
                    }

                    ui.note("Safe mode: this ingot bears the wrong crest. Direct the next strike.");
                    let theme = ColorfulTheme::default();
                    let actions = vec![
                        "Cleanse it now (wipe & relabel)",
                        "Rescan its sigils",
                        "Stand down",
                    ];
                    let choice = Select::with_theme(&theme)
                        .with_prompt("Safe mode: how shall we correct this ingot?")
                        .items(&actions)
                        .default(0)
                        .interact()
                        .unwrap_or(actions.len() - 1);

                    match choice {
                        0 => {
                            ui.warn(&format!(
                                "As commanded — cleansing {} and carving the proper crest.",
                                usb_disk
                            ));
                            wipe_usb_token(&usb_disk, &usb_partition, ui)?;
                            settle_udev(ui)?;
                            effective_force = true;
                            continue;
                        }
                        1 => {
                            ui.note("Let the signals settle. I will verify the crest once more.");
                            settle_udev(ui)?;
                            continue;
                        }
                        _ => {
                            ui.warn("Safe mode terminated. The forge rests until you return.");
                            return Err(anyhow!("initialization aborted by operator"));
                        }
                    }
                }
            }
        }
    }

    begin_phase(ui, "Keysmithing // Beskar Pattern", opts.confirm_each_phase)?;
    let key_material = generate_key_material()?;
    apply_key_to_encryption_root(
        &zfs,
        &enc_root,
        &key_material,
        existing_key.as_ref(),
        &key_location_uri,
        ui,
    )?;
    write_key_to_usb(
        &usb_partition,
        &key_filename,
        effective_force,
        &key_material.hex,
        ui,
    )?;
    timing.pace(Pace::Info);

    ensure_runtime_mount(
        &usb_partition,
        Path::new(&key_mount_dir),
        &key_filename,
        opts.confirm_each_phase,
        ui,
    )?;
    timing.pace(Pace::Info);

    let fingerprint_short = group_string(&key_material.sha256[..32], 8, ' ');
    ui.security(&format!(
        "Key signet etched — SHA-256 (first 128 bits): {}",
        fingerprint_short
    ));
    audit_log(
        "INIT_KEY",
        &format!("partition={} sha256={}", usb_partition, key_material.sha256),
    );

    begin_phase(ui, "Configuration Engraving", opts.confirm_each_phase)?;
    let config_path = PathBuf::from(DEFAULT_CONFIG_PATH);

    let (config, force_write) = if config_path.exists() {
        ui.note(
            "A prior creed is etched into this plate. I will bring its lines in step with today's forging.",
        );

        let backup_path = backup_existing_config(&config_path)?;
        ui.note(&format!(
            "Previous inscription preserved at {}.",
            backup_path.display()
        ));

        let cfg = match ConfigFile::load(&config_path) {
            Ok(mut existing) => {
                normalize_config(
                    &mut existing,
                    &enc_root,
                    &key_path,
                    &key_material.sha256,
                    DEFAULT_TIMEOUT,
                    &binary_path,
                );
                existing
            }
            Err(err) => {
                ui.warn(&format!(
                    "Existing creed unreadable ({}). I will hammer out a fresh Mandalorian template.",
                    err
                ));
                default_config(
                    &enc_root,
                    &key_path,
                    &key_material.sha256,
                    DEFAULT_TIMEOUT,
                    &config_path,
                    &binary_path,
                )
            }
        };
        (cfg, true)
    } else {
        (
            default_config(
                &enc_root,
                &key_path,
                &key_material.sha256,
                DEFAULT_TIMEOUT,
                &config_path,
                &binary_path,
            ),
            false,
        )
    };

    atomic_write_toml(&config_path, &config, force_write)?;
    fs::set_permissions(&config_path, Permissions::from_mode(0o600))
        .context("failed to set config permissions")?;
    ui.success(&format!(
        "Config etched into plate at {}.",
        config_path.display()
    ));
    audit_log("INIT_CFG", &format!("Created {}", config_path.display()));
    timing.pace(Pace::Info);

    let initramfs_flavor =
        detect_initramfs_flavor().context("detect initramfs tooling for auto-unlock")?;

    begin_phase(
        ui,
        "Armor Fittings // Initramfs Integration",
        opts.confirm_each_phase,
    )?;
    match &initramfs_flavor {
        InitramfsFlavor::Dracut(_) => crate::cmd::dracut_install::install_for_dataset(
            ui,
            &config,
            Some(&enc_root),
            Some(initramfs_flavor.clone()),
        )?,
        InitramfsFlavor::InitramfsTools => {
            install_initramfs_tools_scripts(Path::new(&key_mount_dir), ui)?
        }
    }
    timing.pace(Pace::Info);

    begin_phase(ui, "Clan Contingency", opts.confirm_each_phase)?;
    let recovery = generate_recovery_key();
    let recovery_formatted = group_string(&recovery.to_uppercase(), 4, '-');
    ui.security(&format!(
        "Recovery signet (share only with the clan elders): {}",
        recovery_formatted
    ));
    audit_log("INIT_RECOVERY", "Generated recovery key");
    timing.pace(Pace::Info);

    begin_phase(
        ui,
        "Clan Briefing // Initramfs Advisory",
        opts.confirm_each_phase,
    )?;
    if opts.offer_dracut_rebuild {
        ui.info("I recommend refreshing the forge molds immediately.");
        match rebuild_initramfs(ui, &initramfs_flavor) {
            Ok(_) => ui.success("Initramfs reforged with the beskar module embedded."),
            Err(e) => {
                ui.warn(&format!("Automatic initramfs rebuild failed ({}).", e));
                ui.note(
                    "Manual fallback: run `zfs_beskar_key install-dracut` followed by `sudo dracut -f`.",
                );
            }
        }
    } else {
        ui.note(
            "Initramfs rebuild deferred — rerun with --offer-dracut-rebuild when you wish me to handle it.",
        );
        ui.note(
            "Manual path: execute `zfs_beskar_key install-dracut` and then `sudo dracut -f` to embed the module.",
        );
    }

    let usb_uuid = detect_partition_uuid(&usb_partition).unwrap_or_else(|_| "unknown".to_string());

    begin_phase(
        ui,
        "Forge Summary // Armour Inventory",
        opts.confirm_each_phase,
    )?;
    ui.data_panel(
        "Artifacts",
        &[
            ("Key Path", key_path.to_string_lossy().into_owned()),
            ("Config", config_path.display().to_string()),
            ("Recovery Token", recovery_formatted.clone()),
            ("Fingerprint", fingerprint_short),
            ("USB UUID", usb_uuid.clone()),
        ],
    );

    ui.success("The beskar plating is secured. Defensive routines now await deployment.");
    ui.note("Marching orders: run `zfs_beskar_key doctor`, then `zfs_beskar_key install-dracut` and `sudo dracut -f` to bake the initramfs. This is the Way.");
    audit_log(
        "INIT_COMPLETE",
        &format!(
            "dataset={} encryption_root={} partition={} uuid={}",
            target_dataset, enc_root, usb_partition, usb_uuid
        ),
    );
    timing.pace(Pace::Critical);

    Ok(())
}

fn begin_phase(ui: &UX, label: &str, confirm: bool) -> Result<()> {
    if confirm {
        let theme = ColorfulTheme::default();
        let prompt = format!("Safe mode: proceed with {}?", label);
        let proceed = Confirm::with_theme(&theme)
            .with_prompt(prompt)
            .default(false)
            .interact()
            .context("safe mode confirmation failed")?;
        if !proceed {
            ui.warn("Safe mode abort acknowledged — forge sequence halted at your command.");
            return Err(anyhow!("initialization aborted by operator"));
        }
    }
    ui.phase(label);
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

fn sanitize_key_name(dataset: &str) -> String {
    dataset
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
}

fn default_config(
    dataset: &str,
    key_path: &Path,
    sha256: &str,
    timeout: u64,
    config_path: &Path,
    binary_path: &Path,
) -> ConfigFile {
    ConfigFile {
        policy: Policy {
            datasets: vec![dataset.to_string()],
            zfs_path: Some(DEFAULT_ZFS_BIN.to_string()),
            binary_path: Some(binary_path.to_string_lossy().into_owned()),
            allow_root: true,
        },
        crypto: CryptoCfg {
            timeout_secs: timeout,
        },
        usb: Usb {
            key_hex_path: key_path.to_string_lossy().into_owned(),
            expected_sha256: Some(sha256.to_string()),
        },
        fallback: Fallback::default(),
        path: config_path.to_path_buf(),
    }
}

fn normalize_config(
    cfg: &mut ConfigFile,
    dataset: &str,
    key_path: &Path,
    sha256: &str,
    default_timeout: u64,
    binary_path: &Path,
) {
    cfg.policy.allow_root = true;

    cfg.policy.datasets.retain(|entry| entry != dataset);
    cfg.policy.datasets.insert(0, dataset.to_string());

    if cfg
        .policy
        .zfs_path
        .as_ref()
        .map(|path| path.trim().is_empty())
        .unwrap_or(true)
    {
        cfg.policy.zfs_path = Some(DEFAULT_ZFS_BIN.to_string());
    }

    cfg.policy.binary_path = Some(binary_path.to_string_lossy().into_owned());

    if cfg.crypto.timeout_secs == 0 {
        cfg.crypto.timeout_secs = default_timeout;
    }

    cfg.usb.key_hex_path = key_path.to_string_lossy().into_owned();
    cfg.usb.expected_sha256 = Some(sha256.to_string());

    if cfg.fallback.askpass_path.is_none() {
        cfg.fallback.askpass_path = Some("/usr/bin/systemd-ask-password".to_string());
    }
}

fn backup_existing_config(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(anyhow!(
            "No existing config to backup at {}",
            path.display()
        ));
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("Config path missing filename"))?;
    let stamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup_name = format!("{}.bak-{}", file_name, stamp);
    let backup_path = path.with_file_name(backup_name);

    fs::copy(path, &backup_path).with_context(|| {
        format!(
            "Failed to backup existing config to {}",
            backup_path.display()
        )
    })?;
    fs::set_permissions(&backup_path, Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "Failed to set permissions on backup {}",
            backup_path.display()
        )
    })?;
    Ok(backup_path)
}

fn report_usb_target(ui: &UX, device: &str) {
    match fs::metadata(device) {
        Ok(meta) => {
            let descriptor = describe_target(&meta);
            ui.info(&format!(
                "My sensors read {} as a {} candidate.",
                device, descriptor
            ));
            audit_log(
                "INIT_USB_SCAN",
                &format!("device={} kind={}", device, descriptor),
            );
            if descriptor != "block device" {
                ui.warn("I prefer a raw block device (e.g., /dev/sdb) for true beskar.");
            }
        }
        Err(err) => {
            ui.warn(&format!(
                "My instruments cannot reach {} ({}).",
                device, err
            ));
            ui.note("Proceeding blind — confirm the path before we strike the hammer again.");
            audit_log(
                "INIT_USB_SCAN_FAIL",
                &format!("device={} err={}", device, err),
            );
        }
    }
}

fn describe_target(meta: &Metadata) -> &'static str {
    let ty = meta.file_type();
    if ty.is_block_device() {
        "block device"
    } else if ty.is_char_device() {
        "character device"
    } else if ty.is_symlink() {
        "symbolic link"
    } else if meta.is_dir() {
        "directory"
    } else if meta.is_file() {
        "regular file"
    } else {
        "unknown"
    }
}

fn group_string(input: &str, chunk: usize, separator: char) -> String {
    if chunk == 0 {
        return input.to_string();
    }
    input
        .chars()
        .collect::<Vec<_>>()
        .chunks(chunk)
        .map(|part| part.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(&separator.to_string())
}

fn flag_label(enabled: bool) -> String {
    if enabled {
        "enabled".to_string()
    } else {
        "disabled".to_string()
    }
}

fn derive_device_layout(device: &str) -> Result<(String, String)> {
    let device = device.to_string();
    let path = Path::new(&device);
    if !path.exists() {
        return Err(anyhow!("Device {} does not exist", device));
    }

    let block_type = query_block_info(&device, "TYPE")?;
    match block_type.as_str() {
        "disk" => {
            let partition = match existing_partition_for_disk(&device) {
                Ok(Some(path)) => path,
                Ok(None) | Err(_) => predict_partition_name(&device),
            };
            Ok((device.clone(), partition))
        }
        "part" => {
            let parent = query_block_info(&device, "PKNAME")?;
            if parent.is_empty() {
                return Err(anyhow!(
                    "Unable to determine parent disk for partition {}",
                    device
                ));
            }
            Ok((format!("/dev/{}", parent.trim()), device))
        }
        other => Err(anyhow!("Unsupported block type '{}' for {}", other, device)),
    }
}

fn existing_partition_for_disk(disk: &str) -> Result<Option<String>> {
    let out = run_external(
        LSBLK_BINARIES,
        &["-P", "-nrpo", "PATH,TYPE", disk],
        Duration::from_secs(5),
    )?;

    if out.status != 0 {
        return Ok(None);
    }

    for line in out.stdout.lines() {
        let pairs = parse_lsblk_pairs(line);
        if pairs.get("TYPE").map(String::as_str) == Some("part") {
            if let Some(path) = pairs.get("PATH") {
                if Path::new(path).exists() {
                    return Ok(Some(path.clone()));
                }
            }
        }
    }

    Ok(None)
}

fn predict_partition_name(disk: &str) -> String {
    let suffix_is_digit = Path::new(disk)
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.chars().last())
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false);

    if suffix_is_digit {
        format!("{}p1", disk)
    } else {
        format!("{}1", disk)
    }
}

fn ensure_beskar_partition(partition: &str, ui: &UX) -> Result<()> {
    let out = run_external(
        BLKID_BINARIES,
        &["-s", "LABEL", "-o", "value", partition],
        Duration::from_secs(5),
    )?;

    if out.status != 0 {
        ui.warn(&format!(
            "Label on {} refuses to reveal itself ({}). Invoke --force to reforge the token.",
            partition,
            out.stderr.trim()
        ));
        return Err(anyhow!("Missing or unreadable label for {}", partition));
    }

    let label = out.stdout.trim();
    if label != BESKAR_LABEL {
        ui.warn(&format!(
            "Partition {} bears the stamp '{}'; expected '{}'. Invoke --force to recast it.",
            partition, label, BESKAR_LABEL
        ));
        return Err(anyhow!("Unexpected label {} for {}", label, partition));
    }
    Ok(())
}

fn wipe_usb_token(disk: &str, partition: &str, ui: &UX) -> Result<()> {
    dismantle_mounts(disk, ui)?;
    dismantle_mounts(partition, ui)?;

    run_external(
        PARTED_BINARIES,
        &["-s", disk, "mklabel", "gpt"],
        Duration::from_secs(20),
    )?;
    run_external(
        PARTED_BINARIES,
        &["-s", disk, "mkpart", "BESKAR_PART", "ext4", "1MiB", "100%"],
        Duration::from_secs(20),
    )?;

    settle_udev(ui)?;

    run_external(
        MKFS_BINARIES,
        &["-F", "-L", BESKAR_LABEL, partition],
        Duration::from_secs(60),
    )?;

    ui.success(&format!(
        "{} quenched; it now carries the {} sigil.",
        partition, BESKAR_LABEL
    ));
    Ok(())
}

fn settle_udev(ui: &UX) -> Result<()> {
    let res = run_external(UDEVADM_BINARIES, &["settle"], Duration::from_secs(10));
    if let Err(err) = res {
        ui.warn(&format!(
            "udevadm settle faltered ({}). Expect a brief delay.",
            err
        ));
    }
    Ok(())
}

fn query_block_info(device: &str, field: &str) -> Result<String> {
    let out = run_external(
        LSBLK_BINARIES,
        &["-no", field, device],
        Duration::from_secs(5),
    )?;
    if out.status != 0 {
        return Err(anyhow!(
            "lsblk -no {} {} failed: {}",
            field,
            device,
            out.stderr.trim()
        ));
    }
    Ok(out.stdout.trim().to_string())
}

struct KeyMaterial {
    raw: Zeroizing<Vec<u8>>,
    hex: String,
    sha256: String,
}

fn generate_key_material() -> Result<KeyMaterial> {
    let mut raw = Zeroizing::new(vec![0u8; 32]);
    OsRng.fill_bytes(&mut raw[..]);
    let hex = hex::encode(&*raw);
    let sha256 = hex::encode(Sha256::digest(&*raw));
    Ok(KeyMaterial { raw, hex, sha256 })
}

fn write_key_to_usb(
    partition: &str,
    key_filename: &str,
    force: bool,
    key_hex: &str,
    ui: &UX,
) -> Result<()> {
    let mount_dir = tempdir().context("create temporary mount directory")?;
    mount_partition(partition, mount_dir.path())?;

    let key_path = mount_dir.path().join(key_filename);
    if key_path.exists() {
        if force {
            fs::remove_file(&key_path).context("remove existing key file")?;
        } else {
            ui.warn(&format!(
                "Found a prior alloy {} — reforging it in place.",
                key_filename
            ));
            fs::remove_file(&key_path).ok();
        }
    }

    let mut file = File::create(&key_path)
        .with_context(|| format!("create key file at {}", key_path.display()))?;
    file.write_all(key_hex.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all().ok();
    fs::set_permissions(&key_path, Permissions::from_mode(0o400))
        .context("set key file permissions")?;
    drop(file);
    std::thread::sleep(Duration::from_millis(150));

    let mount_path = mount_dir.path().to_path_buf();
    let mut unmounted = false;
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..3 {
        match unmount_partition(mount_path.as_path()) {
            Ok(_) => {
                unmounted = true;
                break;
            }
            Err(err) => {
                last_err = Some(err);
                std::thread::sleep(Duration::from_millis(200 * (attempt as u64 + 1)));
            }
        }
    }

    if !unmounted {
        if let Some(err) = last_err.take() {
            ui.warn(&format!(
                "Standard release failed for {} ({}).",
                partition, err
            ));
        }

        if let Some(mp) = mount_path.to_str() {
            if let Err(inner) = force_unmount(mp, ui) {
                ui.error(&format!(
                    "Unable to disengage temporary mount {} ({}).",
                    mp, inner
                ));
                return Err(anyhow!(
                    "USB token busy at {} — close any open shells or file browsers and retry",
                    mp
                ));
            }
        }
    }

    if let Ok(true) = device_has_mounts(partition) {
        if let Err(inner) = force_unmount(partition, ui) {
            ui.error(&format!(
                "Partition {} remains busy after the release attempt ({}).",
                partition, inner
            ));
            return Err(anyhow!(
                "Unable to release {}. Remove any processes using the USB token and retry.",
                partition
            ));
        }
    }

    if let Err(err) = mount_dir.close() {
        ui.warn(&format!(
            "Temporary mount directory cleanup faltered ({}): {}",
            mount_path.display(),
            err
        ));
    }

    ui.success(&format!(
        "Beskar key sealed at {} atop {}.",
        key_filename, partition
    ));
    Ok(())
}

struct ExistingKey {
    raw: Zeroizing<Vec<u8>>,
}

fn read_existing_key(path: &Path) -> Result<Option<ExistingKey>> {
    if !path.exists() {
        return Ok(None);
    }

    let raw_text = fs::read_to_string(path)
        .with_context(|| format!("read existing key file {}", path.display()))?;
    let cleaned: String = raw_text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() != 64 {
        return Err(anyhow!(
            "Existing key file {} malformed (expected 64 hex chars, found {}).",
            path.display(),
            cleaned.len()
        ));
    }

    let raw_bytes = Zeroizing::new(
        hex::decode(cleaned)
            .with_context(|| format!("decode existing key material at {}", path.display()))?,
    );
    Ok(Some(ExistingKey { raw: raw_bytes }))
}

fn apply_key_to_encryption_root(
    zfs: &Zfs,
    enc_root: &str,
    key_material: &KeyMaterial,
    existing_key: Option<&ExistingKey>,
    key_location: &str,
    ui: &UX,
) -> Result<()> {
    ui.info(&format!(
        "Re-keying encryption root {} with freshly forged beskar.",
        enc_root
    ));

    change_key_with_bytes(zfs, enc_root, &key_material.raw[..])
        .with_context(|| format!("change-key invocation for {}", enc_root))?;

    zfs.set_property(enc_root, "keylocation", "prompt")
        .with_context(|| format!("restore keylocation=prompt on {}", enc_root))?;
    verify_keyformat_raw(zfs, enc_root)?;

    match zfs.load_key_tree(enc_root, &key_material.raw[..]) {
        Ok(unlocked) => {
            let descendants = unlocked.iter().filter(|ds| *ds != enc_root).count();
            if descendants > 0 {
                ui.success(&format!(
                    "Encryption root {} now recognizes the reforged key and released {} descendant dataset(s).",
                    enc_root, descendants
                ));
            } else {
                ui.success(&format!(
                    "Encryption root {} now recognizes the reforged key.",
                    enc_root
                ));
            }
            audit_log(
                "INIT_ZFS_REKEY",
                &format!(
                    "encryption_root={} sha256={} descendants={}",
                    enc_root, key_material.sha256, descendants
                ),
            );
            zfs.set_property(enc_root, "keylocation", key_location)
                .with_context(|| format!("set keylocation to {} on {}", key_location, enc_root))?;
            Ok(())
        }
        Err(err) => {
            let err_msg = err.to_string();
            if err_msg.contains("Key already loaded") {
                zfs.set_property(enc_root, "keylocation", key_location)
                    .with_context(|| {
                        format!("set keylocation to {} on {}", key_location, enc_root)
                    })?;
                ui.note("ZFS reports the key was already resident; verification deferred to the self-test.");
                audit_log(
                    "INIT_ZFS_REKEY_WARN",
                    &format!(
                        "encryption_root={} note={}",
                        enc_root,
                        err_msg.replace('\n', " ")
                    ),
                );
                return Ok(());
            }

            ui.error(&format!(
                "New key rejected when loading {} ({}).",
                enc_root, err_msg
            ));
            if let Some(previous) = existing_key {
                ui.warn("Attempting to restore the prior key material to maintain access.");
                if let Err(revert_err) = change_key_with_bytes(zfs, enc_root, &previous.raw[..]) {
                    ui.error(&format!(
                        "Unable to revert encryption root {} ({}). Manual recovery required.",
                        enc_root, revert_err
                    ));
                } else {
                    let _ = zfs.set_property(enc_root, "keylocation", "prompt");
                    if let Err(check_err) = verify_keyformat_raw(zfs, enc_root) {
                        ui.warn(&format!(
                            "Encryption root {} reports unexpected keyformat after revert ({}).",
                            enc_root, check_err
                        ));
                    }
                    if let Err(load_err) = zfs.load_key_tree(enc_root, &previous.raw[..]) {
                        ui.warn(&format!(
                            "Reverted key could not be loaded automatically ({}).",
                            load_err
                        ));
                    }
                }
            }

            Err(err.context(format!(
                "new key rejected while loading encryption root {}",
                enc_root
            )))
        }
    }
}

fn change_key_with_bytes(zfs: &Zfs, dataset: &str, key_bytes: &[u8]) -> Result<()> {
    let mut temp = NamedTempFile::new().context("create temporary key material file")?;
    temp.write_all(key_bytes)
        .context("write key material to temporary file")?;
    temp.as_file().sync_all().ok();
    fs::set_permissions(temp.path(), Permissions::from_mode(0o600))
        .context("set temporary key permissions")?;
    zfs.change_key_from_file(dataset, temp.path())?;
    Ok(())
}

fn verify_keyformat_raw(zfs: &Zfs, dataset: &str) -> Result<()> {
    let keyformat = zfs
        .get_property(dataset, "keyformat")
        .with_context(|| format!("query keyformat for {}", dataset))?;
    if keyformat != "raw" {
        return Err(anyhow!(
            "Dataset {} reports keyformat={} (expected raw)",
            dataset,
            keyformat
        ));
    }
    Ok(())
}

fn ensure_runtime_mount(
    partition: &str,
    mount_path: &Path,
    key_filename: &str,
    confirm_each_phase: bool,
    ui: &UX,
) -> Result<()> {
    fs::create_dir_all(mount_path).context("prepare runtime mountpoint")?;
    let mount_str = mount_path
        .to_str()
        .ok_or_else(|| anyhow!("invalid runtime mount path"))?;

    // If already mounted elsewhere, detach first.
    if let Ok(current) = query_block_info(partition, "MOUNTPOINT") {
        let current = current.trim();
        if !current.is_empty() && current != mount_str {
            ui.warn(&format!(
                "Partition {} currently rests at {} — relocating to {}.",
                partition, current, mount_str
            ));
            force_unmount(current, ui)?;
            settle_udev(ui)?;
        } else if current == mount_str {
            ui.note(&format!("Beskar token already mounted at {}.", mount_str));
        }
    }

    let theme = ColorfulTheme::default();

    let mut attempts = 0;
    loop {
        let current = query_block_info(partition, "MOUNTPOINT").unwrap_or_default();
        if current.trim() == mount_str {
            break;
        }

        match mount_partition(partition, mount_path) {
            Ok(_) => {
                settle_udev(ui)?;
                ui.info(&format!(
                    "Beskar token mounted at {} for runtime operations.",
                    mount_str
                ));
                break;
            }
            Err(err) => {
                if !confirm_each_phase {
                    return Err(
                        err.context(format!("failed to mount {} at {}", partition, mount_str))
                    );
                }
                ui.warn(&format!(
                    "Unable to mount {} at {} ({}). Awaiting your directive.",
                    partition, mount_str, err
                ));
                let options = vec!["Retry mount", "Abort forge"];
                let selection = Select::with_theme(&theme)
                    .with_prompt("Safe mode: mount attempt failed")
                    .items(&options)
                    .default(0)
                    .interact()
                    .unwrap_or(1);
                if selection == 0 {
                    attempts += 1;
                    if attempts >= 5 {
                        return Err(anyhow!(
                            "unable to mount {} at {} after repeated attempts",
                            partition,
                            mount_str
                        ));
                    }
                    settle_udev(ui)?;
                    continue;
                } else {
                    return Err(anyhow!("initialization aborted by operator"));
                }
            }
        }
    }

    let key_on_mount = mount_path.join(key_filename);
    if !key_on_mount.exists() {
        return Err(anyhow!(
            "Key file {} not found on mounted token",
            key_on_mount.display()
        ));
    }
    fs::set_permissions(&key_on_mount, Permissions::from_mode(0o400))
        .context("set runtime key permissions")?;
    Ok(())
}

fn mount_partition(partition: &str, mountpoint: &Path) -> Result<()> {
    fs::create_dir_all(mountpoint).context("create mount directory")?;
    let mount_str = mountpoint
        .to_str()
        .ok_or_else(|| anyhow!("invalid mount path"))?;
    let out = run_external(
        MOUNT_BINARIES,
        &[partition, mount_str],
        Duration::from_secs(10),
    )?;
    if out.status != 0 {
        return Err(anyhow!(
            "Failed to mount {} at {}: {}",
            partition,
            mountpoint.display(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

fn unmount_partition(mountpoint: &Path) -> Result<()> {
    let mount_str = mountpoint
        .to_str()
        .ok_or_else(|| anyhow!("invalid mount path"))?;
    let out = run_external(UMOUNT_BINARIES, &[mount_str], Duration::from_secs(10))?;
    if out.status != 0 {
        return Err(anyhow!(
            "Failed to unmount {}: {}",
            mountpoint.display(),
            out.stderr.trim()
        ));
    }
    Ok(())
}

fn detect_partition_uuid(partition: &str) -> Result<String> {
    let out = run_external(
        BLKID_BINARIES,
        &["-s", "UUID", "-o", "value", partition],
        Duration::from_secs(5),
    )?;
    if out.status != 0 {
        return Err(anyhow!("blkid failed for {}", partition));
    }
    Ok(out.stdout.trim().to_string())
}

fn select_usb_device(ui: &UX, confirm_each_phase: bool) -> Result<String> {
    begin_phase(
        ui,
        "Target Selection // Choose Beskar Ingot",
        confirm_each_phase,
    )?;
    let theme = ColorfulTheme::default();

    let (disks, beskar_index) = loop {
        let out = run_external(
            LSBLK_BINARIES,
            &["-P", "-nrpo", "NAME,TYPE,RM,SIZE,MODEL,LABEL"],
            Duration::from_secs(5),
        )?;

        let mut scanned = Vec::new();
        let mut detected_beskar: Option<usize> = None;

        for line in out.stdout.lines() {
            let pairs = parse_lsblk_pairs(line);
            let kind = pairs.get("TYPE").cloned().unwrap_or_default();
            let removable = pairs.get("RM").map(String::as_str) == Some("1");
            let name = pairs.get("NAME").cloned().unwrap_or_default();
            let label = pairs.get("LABEL").cloned().unwrap_or_default();

            if kind == "disk" && removable {
                let size = pairs
                    .get("SIZE")
                    .cloned()
                    .unwrap_or_else(|| "?".to_string());
                let model = pairs
                    .get("MODEL")
                    .cloned()
                    .unwrap_or_else(|| "Unknown".to_string());
                if label.eq_ignore_ascii_case(BESKAR_LABEL) {
                    detected_beskar = Some(scanned.len());
                }
                let desc = format!(
                    "{}  [{}]  {}{}",
                    name,
                    size,
                    model,
                    if label.is_empty() {
                        String::new()
                    } else {
                        format!("  (label: {})", label)
                    }
                );
                scanned.push((format!("/dev/{}", name), desc));
            } else if kind == "part" && removable {
                let desc = format!(
                    "{} (partition){}",
                    name,
                    if label.is_empty() {
                        String::new()
                    } else {
                        format!(" label={}", label)
                    }
                );
                if label.eq_ignore_ascii_case(BESKAR_LABEL) {
                    detected_beskar = Some(scanned.len());
                }
                scanned.push((format!("/dev/{}", name), desc));
            }
        }

        if scanned.is_empty() {
            if confirm_each_phase {
                ui.warn("No removable block devices answered the call.");
                ui.note(
                    "Reconnect the USB tribute, wait for the system to register it, \
                    then choose how to proceed.",
                );
                let choices = vec!["Retry scan", "Enter device path manually", "Abort forge"];
                let selection = Select::with_theme(&theme)
                    .with_prompt("Safe mode: choose next action")
                    .items(&choices)
                    .default(0)
                    .interact()
                    .unwrap_or(choices.len() - 1);
                match selection {
                    0 => {
                        settle_udev(ui)?;
                        continue;
                    }
                    1 => {
                        let manual: String = Input::with_theme(&theme)
                            .with_prompt("Enter full /dev/<device> path for the USB token")
                            .with_initial_text("/dev/")
                            .interact_text()
                            .context("manual USB device entry")?;
                        let trimmed = manual.trim();
                        if trimmed.is_empty() {
                            ui.warn("Empty device path received — restarting the scan.");
                            continue;
                        }
                        ui.note(&format!(
                            "Safe mode: proceeding with operator-specified vessel {}.",
                            trimmed
                        ));
                        return Ok(trimmed.to_string());
                    }
                    _ => {
                        ui.warn(
                            "Operator withdrew from the forge during safe-mode device selection.",
                        );
                        return Err(anyhow!("initialization aborted by operator"));
                    }
                }
            } else {
                return Err(anyhow!(
                    "No removable block devices detected. Attach a USB token or specify --usb-device."
                ));
            }
        } else {
            break (scanned, detected_beskar);
        }
    };

    let mut options: Vec<String> = disks.iter().map(|(_, desc)| desc.clone()).collect();
    options.push("Manual entry (specify /dev/ path)".to_string());

    let selection = Select::with_theme(&theme)
        .with_prompt("Select Beskar carrier")
        .default(beskar_index.unwrap_or(0))
        .items(&options)
        .interact()
        .map_err(|e| anyhow!("selection aborted: {}", e))?;

    if selection == options.len() - 1 {
        let device: String = Input::with_theme(&theme)
            .with_prompt("Enter block device path (e.g., /dev/sdb)")
            .validate_with(|input: &String| -> Result<(), &str> {
                if Path::new(input).exists() {
                    Ok(())
                } else {
                    Err("Device not found on filesystem")
                }
            })
            .interact_text()
            .map_err(|e| anyhow!("manual entry aborted: {}", e))?;
        ui.info(&format!("Beskar carrier chosen: {}", device));
        Ok(device)
    } else {
        let (device, _) = &disks[selection];
        ui.info(&format!("Beskar carrier locked: {}", device));
        Ok(device.clone())
    }
}

fn parse_lsblk_pairs(line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let chars: Vec<char> = line.chars().collect();
    let mut idx = 0;
    while idx < chars.len() {
        while idx < chars.len() && chars[idx].is_whitespace() {
            idx += 1;
        }
        if idx >= chars.len() {
            break;
        }
        let key_start = idx;
        while idx < chars.len() && chars[idx] != '=' {
            idx += 1;
        }
        if idx >= chars.len() {
            break;
        }
        let key = chars[key_start..idx]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
        idx += 1; // skip '='
        if idx >= chars.len() {
            break;
        }
        let value;
        if chars[idx] == '"' {
            idx += 1;
            let value_start = idx;
            while idx < chars.len() && chars[idx] != '"' {
                idx += 1;
            }
            value = chars[value_start..idx].iter().collect::<String>();
            idx += 1; // skip closing quote
        } else {
            let value_start = idx;
            while idx < chars.len() && !chars[idx].is_whitespace() {
                idx += 1;
            }
            value = chars[value_start..idx].iter().collect::<String>();
        }
        if !key.is_empty() {
            map.insert(key, value);
        }
    }
    map
}

fn dismantle_mounts(node: &str, ui: &UX) -> Result<()> {
    let out = run_external(
        LSBLK_BINARIES,
        &["-P", "-nrpo", "NAME,MOUNTPOINT", node],
        Duration::from_secs(5),
    )?;

    for line in out.stdout.lines() {
        let pairs = parse_lsblk_pairs(line);
        let target = pairs.get("NAME").cloned().unwrap_or_default();
        let mount = pairs.get("MOUNTPOINT").cloned().unwrap_or_default();
        if mount.is_empty() {
            continue;
        }
        ui.note(&format!(
            "Disengaging mount {} at {} — the path must be clear.",
            target, mount
        ));
        match run_external(UMOUNT_BINARIES, &[mount.as_str()], Duration::from_secs(10)) {
            Ok(res) if res.status == 0 => {
                settle_udev(ui)?;
            }
            _ => {
                force_unmount(&mount, ui)?;
            }
        }
    }

    // also ensure the block node itself is not mounted
    if let Err(err) = run_external(UMOUNT_BINARIES, &[node], Duration::from_secs(10)) {
        ui.warn(&format!(
            "Direct unmount of {} resisted release ({}).",
            node, err
        ));
        if let Err(inner) = force_unmount(node, ui) {
            ui.warn(&format!(
                "Force unmount of {} faltered as well ({}).",
                node, inner
            ));
        }
    }
    Ok(())
}

fn force_unmount(target: &str, ui: &UX) -> Result<()> {
    ui.warn(&format!("Applying force to unmount {}", target));
    let attempts = vec![
        vec![target.to_string()],
        vec!["-l".to_string(), target.to_string()],
        vec!["-f".to_string(), target.to_string()],
        vec!["-f".to_string(), "-l".to_string(), target.to_string()],
    ];

    for args in attempts {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        for backoff in 0..3 {
            if let Ok(out) = run_external(UMOUNT_BINARIES, &refs, Duration::from_secs(10)) {
                if out.status == 0 {
                    settle_udev(ui)?;
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(150 * (backoff as u64 + 1)));
        }
    }

    Err(anyhow!("unable to unmount {} (resource busy)", target))
}

fn device_has_mounts(node: &str) -> Result<bool> {
    let out = run_external(
        LSBLK_BINARIES,
        &["-P", "-nrpo", "NAME,MOUNTPOINT", node],
        Duration::from_secs(5),
    )?;

    if out.status != 0 {
        return Ok(false);
    }

    for line in out.stdout.lines() {
        let pairs = parse_lsblk_pairs(line);
        let mount = pairs.get("MOUNTPOINT").cloned().unwrap_or_default();
        if !mount.is_empty() {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(crate) fn install_initramfs_tools_scripts(key_mount_path: &Path, ui: &UX) -> Result<()> {
    let hook_path = Path::new(INITRAMFS_HOOK_PATH);
    if let Some(parent) = hook_path.parent() {
        fs::create_dir_all(parent).context("create initramfs-tools hook directory")?;
    }

    let hook_content = r#"#!/bin/sh
set -e

. /usr/share/initramfs-tools/hook-functions

if command -v zfs >/dev/null 2>&1; then
    copy_exec "$(command -v zfs)"
fi
if command -v blkid >/dev/null 2>&1; then
    copy_exec "$(command -v blkid)"
fi
if command -v mount >/dev/null 2>&1; then
    copy_exec "$(command -v mount)"
fi
if command -v umount >/dev/null 2>&1; then
    copy_exec "$(command -v umount)"
fi
"#
    .to_string();

    let mut hook_file =
        File::create(hook_path).with_context(|| format!("create {}", hook_path.display()))?;
    hook_file.write_all(hook_content.as_bytes())?;
    hook_file.sync_all().ok();
    fs::set_permissions(hook_path, Permissions::from_mode(0o755))
        .context("set initramfs hook permissions")?;

    let local_top_path = Path::new(INITRAMFS_LOCAL_TOP_PATH);
    if let Some(parent) = local_top_path.parent() {
        fs::create_dir_all(parent).context("create initramfs-tools local-top directory")?;
    }

    let mountpoint = key_mount_path
        .to_str()
        .ok_or_else(|| anyhow!("invalid key mount path for initramfs-tools"))?;

    let local_top_content = format!(
        r#"#!/bin/sh
set -e

PREREQ="zfs"

prereqs() {{
    echo "$PREREQ"
}}

case "$1" in
    prereqs)
        prereqs
        exit 0
        ;;
esac

TOKEN_LABEL="{label}"
MOUNTPOINT="{mountpoint}"
mounted=false

mkdir -p "$MOUNTPOINT"
DEVICE="$(blkid -L "$TOKEN_LABEL" 2>/dev/null || true)"
if [ -z "$DEVICE" ]; then
    echo "beskar: token not detected; skipping auto-unlock" >&2
    exit 0
fi

if ! mount -o ro "$DEVICE" "$MOUNTPOINT"; then
    echo "beskar: unable to mount token at $MOUNTPOINT" >&2
    exit 0
fi

mounted=true
cleanup() {{
    if $mounted; then
        umount "$MOUNTPOINT" 2>/dev/null || true
    fi
}}
trap cleanup EXIT

if ! zfs load-key -a; then
    echo "beskar: zfs load-key -a failed; fallback to native prompts." >&2
fi
"#,
        label = BESKAR_LABEL,
        mountpoint = mountpoint
    );

    let mut local_top_file = File::create(local_top_path)
        .with_context(|| format!("create {}", local_top_path.display()))?;
    local_top_file.write_all(local_top_content.as_bytes())?;
    local_top_file.sync_all().ok();
    fs::set_permissions(local_top_path, Permissions::from_mode(0o755))
        .context("set initramfs local-top permissions")?;

    ui.success("Initramfs-tools scripts installed for beskar auto-unlock (Ubuntu style).");
    audit_log(
        "INIT_INITRAMFS_TOOLS",
        "Ubuntu initramfs-tools hook installed.",
    );
    Ok(())
}

pub(crate) fn rebuild_initramfs(ui: &UX, flavor: &InitramfsFlavor) -> Result<()> {
    use std::path::Path;

    match flavor {
        InitramfsFlavor::Dracut(_) => {
            let candidates = ["/usr/bin/dracut", "/usr/sbin/dracut"];
            let dracut_path = candidates
                .iter()
                .find(|p| Path::new(p).exists())
                .ok_or_else(|| anyhow!("dracut binary not found on PATH {:?}", candidates))?;

            ui.info(&format!(
                "Calling dracut via {} to refresh the initramfs image…",
                dracut_path
            ));
            let cmd = Cmd::new_allowlisted(*dracut_path, Duration::from_secs(180))?;
            let out = cmd.run(&["-f"], None)?;
            if out.status != 0 {
                return Err(anyhow!(
                    "dracut exited with status {}: {}",
                    out.status,
                    out.stderr.trim()
                ));
            }
        }
        InitramfsFlavor::InitramfsTools => {
            let update_initramfs = "/usr/sbin/update-initramfs";
            if !Path::new(update_initramfs).exists() {
                return Err(anyhow!(
                    "initramfs-tools detected but {} missing",
                    update_initramfs
                ));
            }
            ui.info("Calling update-initramfs -u to refresh the initramfs image…");
            let cmd = Cmd::new_allowlisted(update_initramfs, Duration::from_secs(180))?;
            let out = cmd.run(&["-u"], None)?;
            if out.status != 0 {
                return Err(anyhow!(
                    "update-initramfs exited with status {}: {}",
                    out.status,
                    out.stderr.trim()
                ));
            }
        }
    }
    Ok(())
}

fn run_external(candidates: &[&str], args: &[&str], timeout: Duration) -> Result<OutputData> {
    for &path in candidates {
        if Path::new(path).exists() {
            let cmd = Cmd::new_allowlisted(path, timeout)?;
            return cmd.run(args, None);
        }
    }
    Err(anyhow!(
        "None of the candidate binaries {:?} were found on this system",
        candidates
    ))
}
