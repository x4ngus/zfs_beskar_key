// ============================================================================
// src/cmd/dracut_install.rs – Dedicated dracut installer subcommand
// ============================================================================

use crate::cmd::init::{detect_initramfs_flavor, install_dracut_module, InitramfsFlavor};
use crate::config::ConfigFile;
use crate::ui::UX;
use crate::util::binary::determine_binary_path;
use crate::zfs::Zfs;
use anyhow::{anyhow, Context, Result};
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
        "Preparing Beskar dracut module targeting dataset {}.",
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

    let binary_path = determine_binary_path(Some(cfg))?;

    let flavor = match flavor_hint {
        Some(f) => f,
        None => detect_initramfs_flavor()?,
    };

    let module_dir = match flavor {
        InitramfsFlavor::Dracut(path) => path,
        InitramfsFlavor::InitramfsTools => {
            return Err(anyhow!(
            "initramfs-tools detected; dracut module installation not applicable on this system."
        ))
        }
    };

    ui.info(&format!(
        "Installing dracut payload at {}.",
        module_dir.display()
    ));

    install_dracut_module(
        module_dir.as_path(),
        &encryption_root,
        cfg.path.as_path(),
        binary_path.as_path(),
        ui,
    )?;

    ui.note("Initramfs rebuild will run automatically; rerun `dracut -f` manually if it fails.");
    Ok(())
}
