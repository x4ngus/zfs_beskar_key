// ============================================================================
// src/cmd/dracut_install.rs – Dedicated dracut installer subcommand
// ============================================================================

use crate::cmd::init::{detect_initramfs_flavor, rebuild_initramfs, InitramfsFlavor};
use crate::config::ConfigFile;
use crate::dracut::{self, ModuleContext, ModulePaths, DEFAULT_MOUNTPOINT};
use crate::ui::UX;
use crate::util::keyfile::{ensure_raw_key_file, KeyEncoding};
use crate::zfs::Zfs;
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::time::Duration;

pub fn run(
    ui: &UX,
    cfg: &ConfigFile,
    dataset_override: Option<&str>,
    flavor_hint: Option<InitramfsFlavor>,
) -> Result<()> {
    ui.banner();
    install_for_dataset(ui, cfg, dataset_override, flavor_hint)
}

pub fn install_for_dataset(
    ui: &UX,
    cfg: &ConfigFile,
    dataset_override: Option<&str>,
    flavor_hint: Option<InitramfsFlavor>,
) -> Result<()> {
    let dataset_hint = dataset_override
        .map(|d| d.to_string())
        .or_else(|| cfg.policy.datasets.first().cloned())
        .unwrap_or_else(|| "rpool/ROOT".to_string());

    ui.info(&format!(
        "Preparing Ubuntu-style unlock for dataset {}.",
        dataset_hint
    ));

    let zfs_timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
    let zfs_client = cfg
        .policy
        .zfs_path
        .as_ref()
        .map(|p| Zfs::with_path(p, zfs_timeout))
        .unwrap_or_else(|| Zfs::discover(zfs_timeout));

    let client = zfs_client.map_err(|err| anyhow!("Unable to initialize zfs client: {}", err))?;

    let mut encryption_root = client
        .encryption_root(&dataset_hint)
        .with_context(|| format!("resolve encryption root for {}", dataset_hint))?;
    if encryption_root.trim().is_empty() {
        encryption_root = dataset_hint.clone();
    }

    if encryption_root != dataset_hint {
        ui.info(&format!(
            "Dataset {} anchored at encryption root {}.",
            dataset_hint, encryption_root
        ));
    }

    if !client
        .is_encrypted(&encryption_root)
        .with_context(|| format!("verify encryption status for {}", encryption_root))?
    {
        return Err(anyhow!(
            "Dataset {} reports encryption=off. Beskar dracut module not required.",
            encryption_root
        ));
    }

    let key_path = Path::new(&cfg.usb.key_hex_path);
    if !key_path.is_absolute() {
        return Err(anyhow!(
            "usb.key_hex_path ({}) must be an absolute path.",
            key_path.display()
        ));
    }
    let material = ensure_raw_key_file(key_path)
        .with_context(|| format!("normalize key file at {}", key_path.display()))?;
    if material.encoding == KeyEncoding::Hex {
        ui.info(&format!(
            "Converted legacy hex key at {} into raw bytes for Ubuntu's initramfs chain.",
            key_path.display()
        ));
    }
    let mountpoint_path = key_path
        .parent()
        .unwrap_or_else(|| Path::new(DEFAULT_MOUNTPOINT));
    let mountpoint_owned = mountpoint_path.to_string_lossy().into_owned();
    let key_path_owned = key_path.to_string_lossy().into_owned();
    let key_location = format!("file://{}", key_path.display());

    client
        .set_property(&encryption_root, "keylocation", &key_location)
        .with_context(|| format!("set keylocation on {}", encryption_root))?;
    ui.info(&format!(
        "keylocation for {} set to {}.",
        encryption_root, key_location
    ));

    let flavor = match flavor_hint {
        Some(f) => f,
        None => detect_initramfs_flavor()?,
    };

    let module_dir = match &flavor {
        InitramfsFlavor::Dracut(path) => path.clone(),
        InitramfsFlavor::InitramfsTools => {
            return Err(anyhow!(
            "initramfs-tools detected; dracut module installation not applicable on this system."
        ))
        }
    };
    let key_sha = cfg.usb.expected_sha256.as_deref();
    if key_sha.is_none() {
        ui.warn(
            "config.usb.expected_sha256 missing — initramfs loader will skip checksum enforcement.",
        );
    }
    let module_paths = ModulePaths::new(&module_dir);
    let ctx = ModuleContext {
        mountpoint: &mountpoint_owned,
        key_path: &key_path_owned,
        key_sha256: key_sha,
    };

    dracut::install_module(&module_paths, &ctx)?;
    ui.success(&format!(
        "Dracut module stamped at {}.",
        module_dir.display()
    ));

    rebuild_initramfs(ui, &flavor).context("invoke dracut -f for updated Beskar module")?;
    ui.success("dracut -f completed with Beskar module embedded.");

    Ok(())
}
