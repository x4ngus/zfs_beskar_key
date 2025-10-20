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
use tempfile::tempdir;
use zeroize::Zeroizing;

use crate::cmd::{Cmd, OutputData};
use crate::config::{ConfigFile, CryptoCfg, Fallback, Policy, Usb};
use crate::ui::{Pace, Timing, UX};
use crate::util::atomic::atomic_write_toml;
use crate::util::audit::audit_log;

const BESKAR_LABEL: &str = "BESKARKEY";
const DEFAULT_CONFIG_PATH: &str = "/etc/zfs-beskar.toml";
const DEFAULT_ZFS_BIN: &str = "/sbin/zfs";
const DEFAULT_TIMEOUT: u64 = 10;
const DRACUT_MODULE_DIR: &str = "/usr/lib/dracut/modules.d/95beskar";
const DRACUT_SCRIPT_NAME: &str = "beskar-unlock.sh";
const DRACUT_SETUP_NAME: &str = "module-setup.sh";

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

// ----------------------------------------------------------------------------
// Public entrypoint
// ----------------------------------------------------------------------------

pub fn run_init(ui: &UX, timing: &Timing, opts: InitOptions) -> Result<()> {
    ui.banner();
    ui.phase("Forge Initialization // Tempering Beskar");
    ui.info("Summoning the covert's forge to temper armour around your filesystem core.");
    timing.pace(Pace::Info);

    let dataset = opts
        .pool
        .clone()
        .unwrap_or_else(|| "rpool/ROOT".to_string());
    let key_basename = sanitize_key_name(&dataset);

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

    let usb_target = match opts.usb_device.clone() {
        Some(dev) => dev,
        None => discover_beskar_partition()?.ok_or_else(|| {
            anyhow!("Unable to auto-detect USB token labeled {BESKAR_LABEL}. Provide --usb-device.")
        })?,
    };

    let (usb_disk, usb_partition) = derive_device_layout(&usb_target)?;

    ui.data_panel(
        "Forge Ledger",
        &[
            ("Dataset", dataset.clone()),
            ("USB Disk", usb_disk.clone()),
            ("USB Partition", usb_partition.clone()),
            ("Key Mount Path", key_mount_dir.clone()),
            ("Key File", key_filename.clone()),
            ("Auto-Unlock", flag_label(opts.auto_unlock)),
        ],
    );
    timing.pace(Pace::Info);

    ui.phase("Forge Survey // Inspect Alloy");
    report_usb_target(ui, &usb_disk);
    if usb_partition != usb_disk {
        report_usb_target(ui, &usb_partition);
    }
    timing.pace(Pace::Info);

    if opts.force {
        ui.warn(&format!(
            "Force flag detected — remelting {} so a fresh Beskar token can be poured.",
            usb_disk
        ));
        wipe_usb_token(&usb_disk, &usb_partition, ui)?;
        settle_udev(ui)?;
        audit_log(
            "INIT_USB_WIPE",
            &format!("disk={} partition={}", usb_disk, usb_partition),
        );
    } else {
        ensure_beskar_partition(&usb_partition, ui)?;
    }

    ui.phase("Beskar Key Forge");
    let forge = forge_usb_key(&usb_partition, &key_filename, opts.force, ui)?;
    timing.pace(Pace::Info);

    let fingerprint_short = group_string(&forge.sha256[..32], 8, ' ');
    ui.security(&format!(
        "Key signet (SHA-256 · first 128 bits): {}",
        fingerprint_short
    ));
    audit_log(
        "INIT_KEY",
        &format!("partition={} sha256={}", usb_partition, forge.sha256),
    );

    ui.phase("Configuration Engraving");
    let config_path = PathBuf::from(DEFAULT_CONFIG_PATH);

    let (config, force_write) = if config_path.exists() {
        ui.note("Existing Beskar creed detected — aligning it with the new forge output.");

        let backup_path = backup_existing_config(&config_path)?;
        ui.note(&format!(
            "Previous inscription preserved at {}.",
            backup_path.display()
        ));

        let cfg = match ConfigFile::load(&config_path) {
            Ok(mut existing) => {
                normalize_config(
                    &mut existing,
                    &dataset,
                    &key_path,
                    &forge.sha256,
                    DEFAULT_TIMEOUT,
                );
                existing
            }
            Err(err) => {
                ui.warn(&format!(
                    "Existing creed unreadable ({}). Forging fresh Mandalorian template.",
                    err
                ));
                default_config(
                    &dataset,
                    &key_path,
                    &forge.sha256,
                    DEFAULT_TIMEOUT,
                    &config_path,
                )
            }
        };
        (cfg, true)
    } else {
        (
            default_config(
                &dataset,
                &key_path,
                &forge.sha256,
                DEFAULT_TIMEOUT,
                &config_path,
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

    ui.phase("Armor Fittings // Dracut Integration");
    install_dracut_module(&dataset, &config_path, ui)?;
    timing.pace(Pace::Info);

    ui.phase("Clan Contingency");
    let recovery = generate_recovery_key();
    let recovery_formatted = group_string(&recovery.to_uppercase(), 4, '-');
    ui.security(&format!(
        "Recovery signet (share only with the clan elders): {}",
        recovery_formatted
    ));
    audit_log("INIT_RECOVERY", "Generated recovery key");
    timing.pace(Pace::Info);

    ui.phase("Clan Briefing // Initramfs Advisory");
    if opts.offer_dracut_rebuild {
        ui.info("Armorer recommends refreshing the forge molds immediately.");
        ui.warn("Run `dracut -f` to bake the Beskar module into initramfs.");
    } else {
        ui.note(
            "Dracut rebuild deferred — rerun with --offer-dracut-rebuild when you wish the Armorer to handle it.",
        );
    }

    let usb_uuid = detect_partition_uuid(&usb_partition).unwrap_or_else(|_| "unknown".to_string());

    ui.phase("Forge Summary // Armour Inventory");
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

    ui.success("Beskar armour assembled. Defensive routines now stand ready.");
    ui.note("Next: run `zbk doctor`, then rebuild initramfs to honor the forge. This is the Way.");
    audit_log(
        "INIT_COMPLETE",
        &format!(
            "dataset={} partition={} uuid={}",
            dataset, usb_partition, usb_uuid
        ),
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
) -> ConfigFile {
    ConfigFile {
        policy: Policy {
            datasets: vec![dataset.to_string()],
            zfs_path: Some(DEFAULT_ZFS_BIN.to_string()),
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
                "Forge sensors register {} as a {} candidate.",
                device, descriptor
            ));
            audit_log(
                "INIT_USB_SCAN",
                &format!("device={} kind={}", device, descriptor),
            );
            if descriptor != "block device" {
                ui.warn("The Armorer prefers a raw block device (e.g., /dev/sdb) for true Beskar.");
            }
        }
        Err(err) => {
            ui.warn(&format!("Forge sensors cannot reach {} ({}).", device, err));
            ui.note("Proceeding blind — confirm the path before striking the hammer again.");
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

fn discover_beskar_partition() -> Result<Option<String>> {
    let out = run_external(
        BLKID_BINARIES,
        &["-o", "device", "-t", &format!("LABEL={}", BESKAR_LABEL)],
        Duration::from_secs(5),
    );
    match out {
        Ok(data) if data.status == 0 => {
            let device = data.stdout.trim();
            if device.is_empty() {
                Ok(None)
            } else {
                Ok(Some(device.to_string()))
            }
        }
        _ => Ok(None),
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
        "disk" => Ok((device.clone(), format!("{}1", device))),
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

fn ensure_beskar_partition(partition: &str, ui: &UX) -> Result<()> {
    let out = run_external(
        BLKID_BINARIES,
        &["-s", "LABEL", "-o", "value", partition],
        Duration::from_secs(5),
    )?;

    if out.status != 0 {
        ui.warn(&format!(
            "Unable to read label for {} ({}). Invoke --force to reforge the token.",
            partition,
            out.stderr.trim()
        ));
        return Err(anyhow!("Missing or unreadable label for {}", partition));
    }

    let label = out.stdout.trim();
    if label != BESKAR_LABEL {
        ui.warn(&format!(
            "Partition {} is stamped '{}', expected '{}'. Use --force to recast it.",
            partition, label, BESKAR_LABEL
        ));
        return Err(anyhow!("Unexpected label {} for {}", label, partition));
    }
    Ok(())
}

fn wipe_usb_token(disk: &str, partition: &str, ui: &UX) -> Result<()> {
    let _ = run_external(UMOUNT_BINARIES, &[partition], Duration::from_secs(5));

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
        "{} quenched and relabeled {}.",
        partition, BESKAR_LABEL
    ));
    Ok(())
}

fn settle_udev(ui: &UX) -> Result<()> {
    let res = run_external(UDEVADM_BINARIES, &["settle"], Duration::from_secs(10));
    if let Err(err) = res {
        ui.warn(&format!("udevadm settle failed: {}", err));
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

struct ForgeResult {
    sha256: String,
}

fn forge_usb_key(partition: &str, key_filename: &str, force: bool, ui: &UX) -> Result<ForgeResult> {
    let mut key_bytes = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(&mut *key_bytes);
    let key_hex = hex::encode(&*key_bytes);
    let sha256 = hex::encode(Sha256::digest(&*key_bytes));

    let mount_dir = tempdir().context("create temporary mount directory")?;
    mount_partition(partition, mount_dir.path())?;

    let key_path = mount_dir.path().join(key_filename);
    if key_path.exists() {
        if force {
            fs::remove_file(&key_path).context("remove existing key file")?;
        } else {
            ui.warn(&format!(
                "Found prior alloy {} — reforging in place.",
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

    if let Err(err) = unmount_partition(mount_dir.path()) {
        ui.warn(&format!("Failed to unmount {}: {}", partition, err));
    }

    ui.success(&format!(
        "Beskar key sealed at {} (on {}).",
        key_filename, partition
    ));
    Ok(ForgeResult { sha256 })
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

fn install_dracut_module(dataset: &str, config_path: &Path, ui: &UX) -> Result<()> {
    fs::create_dir_all(DRACUT_MODULE_DIR).context("create dracut module directory")?;

    let script_path = Path::new(DRACUT_MODULE_DIR).join(DRACUT_SCRIPT_NAME);
    let setup_path = Path::new(DRACUT_MODULE_DIR).join(DRACUT_SETUP_NAME);

    let script_content = format!(
        r#"#!/bin/bash

set -e

if ! type warn >/dev/null 2>&1; then
    warn() {{ echo "[BESKAR] ${{*}}" >&2; }}
fi

TOKEN_LABEL="{label}"
MOUNTPOINT="/run/beskar"
CONFIG_PATH="{config}"
DATASET="{dataset}"
BINARY="/usr/local/bin/zfs_beskar_key"

mkdir -p "$MOUNTPOINT"
DEVICE=$(blkid -L $TOKEN_LABEL 2>/dev/null || true)
if [ -z "$DEVICE" ]; then
    warn "Beskar token not detected; skipping initramfs unlock."
    exit 0
fi

if ! mount -o ro "$DEVICE" "$MOUNTPOINT"; then
    warn "Unable to mount Beskar token inside initramfs."
    exit 0
fi

if [ -x "$BINARY" ]; then
    "$BINARY" auto-unlock --config="$CONFIG_PATH" --dataset="$DATASET" --json || warn "Auto-unlock failed inside initramfs."
else
    warn "zfs_beskar_key binary unavailable in initramfs."
fi

umount "$MOUNTPOINT" || warn "Failed to unmount Beskar token in initramfs."
"#,
        label = BESKAR_LABEL,
        config = config_path.display(),
        dataset = dataset
    );

    let mut script_file =
        File::create(&script_path).with_context(|| format!("create {}", script_path.display()))?;
    script_file.write_all(script_content.as_bytes())?;
    script_file.sync_all().ok();
    fs::set_permissions(&script_path, Permissions::from_mode(0o750))
        .context("set dracut script permissions")?;

    let setup_content = format!(
        r#"#!/bin/bash

check() {{
    return 0
}}

depends() {{
    echo systemd
    return 0
}}

install() {{
    inst_multiple blkid mount umount
    inst_simple "$moddir/{script}" /sbin/{script}
    inst_simple /usr/local/bin/zfs_beskar_key /usr/local/bin/zfs_beskar_key
    inst_simple "{config}" "{config}"
    inst_hook initqueue/online 95 "$moddir/{script}"
}}
"#,
        script = DRACUT_SCRIPT_NAME,
        config = config_path.display(),
    );

    let mut setup_file =
        File::create(&setup_path).with_context(|| format!("create {}", setup_path.display()))?;
    setup_file.write_all(setup_content.as_bytes())?;
    setup_file.sync_all().ok();
    fs::set_permissions(&setup_path, Permissions::from_mode(0o750))
        .context("set module setup permissions")?;

    ui.success(&format!(
        "Dracut module refreshed at {}.",
        DRACUT_MODULE_DIR
    ));
    audit_log(
        "INIT_DRACUT_MODULE",
        &format!("dataset={} module={}", dataset, DRACUT_MODULE_DIR),
    );
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
