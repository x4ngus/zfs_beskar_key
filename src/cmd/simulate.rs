// ============================================================================
// src/cmd/simulate.rs – Ephemeral ZFS vault simulation for menu demos
// ============================================================================

use crate::cmd::{Cmd, OutputData};
use crate::config::{ConfigFile, CryptoCfg, Fallback, Policy, Usb};
use crate::ui::{Pace, Timing, UX};
use crate::zfs::Zfs;
use anyhow::{anyhow, Context, Result};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

pub fn run_vault_drill(ui: &UX, timing: &Timing, base_cfg: &ConfigFile) -> Result<()> {
    ui.banner();
    ui.phase("Ephemeral Forge // Preparing Vault Simulation");

    let mut sim = match VaultSimulation::prepare(base_cfg) {
        Ok(sim) => sim,
        Err(err) => {
            emit_preflight_remediation(ui, timing, base_cfg, &err);
            return Err(err);
        }
    };
    timing.pace(Pace::Info);

    ui.info(&format!(
        "Ephemeral pool {} hammered atop {}.",
        sim.pool_name,
        sim.image_path.display()
    ));
    ui.note("Sealing the training vault to mimic a cold boot state…");
    sim.ensure_locked()?;
    timing.pace(Pace::Prompt);

    ui.phase("Vault Drill // USB-First Unseal");
    match crate::cmd::unlock::run_unlock(ui, timing, &sim.config, &sim.dataset_name) {
        Ok(_) => {
            let zfs = sim.zfs()?;
            if zfs.is_unlocked(&sim.dataset_name)? {
                ui.success("Ephemeral vault unlocked — the beskar token proves its worth.");
                audit_status(&zfs, &sim.dataset_name, ui);
            } else {
                ui.warn("Vault status uncertain after simulation; inspect `zfs keystatus` directly.");
            }
        }
        Err(err) => {
            emit_unlock_remediation(ui, timing, base_cfg, &err);
            return Err(err);
        }
    }
    timing.pace(Pace::Info);

    ui.phase("Reseal Protocol // Disengaging Key");
    let zfs = sim.zfs()?;
    if let Err(err) = zfs.unload_key(&sim.dataset_name) {
        emit_reseal_remediation(ui, timing, base_cfg, &err);
        return Err(err);
    }
    ui.success("Ephemeral vault sealed; key withdrawn from memory.");
    timing.pace(Pace::Critical);

    ui.phase("Forge Cleanup // Dismantling Simulation");
    sim.teardown()?;
    timing.pace(Pace::Info);

    ui.phase("Post-Drill Briefing // Clan Readiness");
    ui.data_panel(
        "Recommended Steps",
        &[
            (
                "Re-run init",
                "sudo zfs_beskar_key init --dataset=<dataset>".to_string(),
            ),
            (
                "Refresh initramfs",
                "sudo dracut -f  # ensures beskar module within rescue image".to_string(),
            ),
            (
                "Verify USB health",
                "sudo zfs_beskar_key self-test --dataset=<dataset>".to_string(),
            ),
        ],
    );
    ui.note(
        "If any step raises concerns, rerun the forge with --force and inspect `/etc/zfs-beskar.toml.bak-*` for recovery.",
    );
    ui.success("Simulation complete. Your true pools remain untouched. This is the Way.");
    Ok(())
}

// ----------------------------------------------------------------------------
// Internal scaffolding
// ----------------------------------------------------------------------------

struct VaultSimulation {
    _temp_dir: TempDir,
    pool_name: String,
    dataset_name: String,
    image_path: PathBuf,
    config: ConfigFile,
    zfs_path: String,
    zpool_path: String,
    timeout: Duration,
    cleaned: bool,
}

impl VaultSimulation {
    fn prepare(base_cfg: &ConfigFile) -> Result<Self> {
        let timeout = Duration::from_secs(base_cfg.crypto.timeout_secs.max(1));
        let zfs_path = resolve_zfs_path(base_cfg)?;
        let zpool_path = resolve_zpool_path()?;

        let temp_dir = TempDir::new().context("create simulation tempdir")?;
        let image_path = temp_dir.path().join("beskar-sim.img");
        let backing = File::create(&image_path).context("create simulation backing file")?;
        backing
            .set_len(128 * 1024 * 1024)
            .context("size simulation backing file")?;

        let pool_name = format!("beskar_sim_{}", nanoid::nanoid!(6).to_lowercase());
        let dataset_name = format!("{}/forge", pool_name);

        let raw_key_path = temp_dir.path().join("beskar.raw");
        let hex_key_path = temp_dir.path().join("beskar.keyhex");
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);

        {
            let mut raw = File::create(&raw_key_path).context("create raw key file")?;
            raw.write_all(&key_bytes)
                .context("write raw key material")?;
            raw.sync_all().ok();
            fs::set_permissions(&raw_key_path, fs::Permissions::from_mode(0o400))
                .context("set raw key permissions")?;
        }
        {
            let mut hex = File::create(&hex_key_path).context("create hex key file")?;
            let hex_str = hex::encode(key_bytes);
            writeln!(hex, "{}", hex_str).context("write hex key")?;
            hex.sync_all().ok();
            fs::set_permissions(&hex_key_path, fs::Permissions::from_mode(0o400))
                .context("set hex key permissions")?;
        }

        let sha256 = hex::encode(Sha256::digest(key_bytes));

        let zpool_cmd = Cmd::new_allowlisted(&zpool_path, Duration::from_secs(10))?;
        let pool_out = zpool_cmd
            .run(
                &[
                    "create",
                    "-f",
                    &pool_name,
                    image_path.to_string_lossy().as_ref(),
                ],
                None,
            )
            .with_context(|| format!("create simulated pool {}", pool_name))?;
        if pool_out.status != 0 {
            return Err(anyhow!(
                "zpool create failed for {}: {}",
                pool_name,
                pool_out.stderr
            ));
        }

        let zfs_cmd = Cmd::new_allowlisted(&zfs_path, Duration::from_secs(10))?;
        let keylocation = format!("keylocation=file://{}", raw_key_path.to_string_lossy());
        let dataset_out = zfs_cmd
            .run(
                &[
                    "create",
                    "-o",
                    "encryption=on",
                    "-o",
                    "keyformat=raw",
                    "-o",
                    &keylocation,
                    "-o",
                    "mountpoint=none",
                    &dataset_name,
                ],
                None,
            )
            .with_context(|| format!("create simulated dataset {}", dataset_name))?;
        if dataset_out.status != 0 {
            return Err(anyhow!(
                "zfs create failed for {}: {}",
                dataset_name,
                dataset_out.stderr
            ));
        }

        let config_path = temp_dir.path().join("zfs-beskar-sim.toml");
        let sim_config = ConfigFile {
            policy: Policy {
                datasets: vec![dataset_name.clone()],
                zfs_path: Some(zfs_path.clone()),
                binary_path: base_cfg.policy.binary_path.clone(),
                allow_root: true,
            },
            crypto: CryptoCfg {
                timeout_secs: base_cfg.crypto.timeout_secs.max(1),
            },
            usb: Usb {
                key_hex_path: hex_key_path.to_string_lossy().into_owned(),
                expected_sha256: Some(sha256.clone()),
            },
            fallback: Fallback::default(),
            path: config_path.clone(),
        };

        let toml = toml::to_string_pretty(&sim_config).context("serialize simulation config")?;
        fs::write(&config_path, toml).context("write simulation config file")?;
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))
            .context("set simulation config permissions")?;

        Ok(Self {
            _temp_dir: temp_dir,
            pool_name,
            dataset_name,
            image_path,
            config: sim_config,
            zfs_path,
            zpool_path,
            timeout,
            cleaned: false,
        })
    }

    fn zfs(&self) -> Result<Zfs> {
        Zfs::with_path(&self.zfs_path, self.timeout)
    }

    fn ensure_locked(&self) -> Result<()> {
        let zfs = self.zfs()?;
        if zfs.is_unlocked(&self.dataset_name)? {
            zfs.unload_key(&self.dataset_name)?;
        }
        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        if self.cleaned {
            return Ok(());
        }
        if let Ok(out) = self.destroy_dataset() {
            if out.status != 0 {
                return Err(anyhow!(
                    "failed to destroy simulated dataset {}: {}",
                    self.dataset_name,
                    out.stderr
                ));
            }
        }
        if let Ok(out) = self.destroy_pool() {
            if out.status != 0 {
                return Err(anyhow!(
                    "failed to destroy simulated pool {}: {}",
                    self.pool_name,
                    out.stderr
                ));
            }
        }
        self.cleaned = true;
        Ok(())
    }

    fn destroy_dataset(&self) -> Result<OutputData> {
        let cmd = Cmd::new_allowlisted(&self.zfs_path, Duration::from_secs(10))?;
        cmd.run(&["destroy", "-R", &self.dataset_name], None)
    }

    fn destroy_pool(&self) -> Result<OutputData> {
        let cmd = Cmd::new_allowlisted(&self.zpool_path, Duration::from_secs(10))?;
        cmd.run(&["destroy", "-f", &self.pool_name], None)
    }
}

impl Drop for VaultSimulation {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.destroy_dataset();
            let _ = self.destroy_pool();
            self.cleaned = true;
        }
    }
}

fn resolve_zfs_path(cfg: &ConfigFile) -> Result<String> {
    if let Some(p) = &cfg.policy.zfs_path {
        return Ok(p.clone());
    }

    let candidates = [
        "/sbin/zfs",
        "/usr/sbin/zfs",
        "/usr/local/sbin/zfs",
        "/bin/zfs",
    ];
    for candidate in &candidates {
        if Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }
    }
    Err(anyhow!("zfs binary not found. Checked: {:?}", candidates))
}

fn resolve_zpool_path() -> Result<String> {
    let candidates = [
        "/sbin/zpool",
        "/usr/sbin/zpool",
        "/usr/local/sbin/zpool",
        "/bin/zpool",
    ];
    for candidate in &candidates {
        if Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }
    }
    Err(anyhow!("zpool binary not found. Checked: {:?}", candidates))
}

fn emit_preflight_remediation(
    ui: &UX,
    timing: &Timing,
    base_cfg: &ConfigFile,
    err: &anyhow::Error,
) {
    let message = err.to_string();
    ui.error(&format!("Vault drill aborted before liftoff: {}", message));
    timing.pace(Pace::Error);

    let dataset_hint = base_cfg
        .policy
        .datasets
        .first()
        .cloned()
        .unwrap_or_else(|| "<dataset>".to_string());

    let mut checklist: Vec<(&str, String)> = vec![
        ("Run as root", "sudo zfs_beskar_key --menu".to_string()),
        (
            "Install ZFS utilities",
            "sudo apt install zfsutils-linux".to_string(),
        ),
        ("Load kernel modules", "sudo modprobe zfs".to_string()),
    ];

    if message.contains("binary not found") {
        checklist.push((
            "Verify zfs/zpool path",
            "which zfs && which zpool".to_string(),
        ));
    }
    if message.contains("permission denied") {
        checklist.push((
            "Confirm device access",
            "lsmod | grep zfs  # ensure modules are loaded".to_string(),
        ));
    }
    checklist.push((
        "Practice manual drill",
        format!("sudo zfs load-key {}", dataset_hint),
    ));

    ui.data_panel("Preflight Checklist", &checklist);
    ui.note("Work through these items, then relaunch the vault drill to verify the forge.");
}

fn audit_status(zfs: &Zfs, dataset: &str, ui: &UX) {
    if let Ok(enc_root) = zfs.encryption_root(dataset) {
        ui.note(&format!("Encryption root confirmed as {}.", enc_root));
    }
    match zfs.is_unlocked(dataset) {
        Ok(true) => ui.note("Keystatus: available (key is resident)."),
        Ok(false) => ui.note("Keystatus: locked (key removed)."),
        Err(err) => ui.warn(&format!("Unable to query keystatus ({}).", err)),
    }
}

fn emit_unlock_remediation(ui: &UX, timing: &Timing, base_cfg: &ConfigFile, err: &anyhow::Error) {
    let dataset_hint = base_cfg
        .policy
        .datasets
        .first()
        .cloned()
        .unwrap_or_else(|| "<dataset>".to_string());
    let cfg_hint = base_cfg.path.as_path().to_string_lossy().to_string();

    ui.error(&format!("Ephemeral unlock attempt failed: {}", err));
    timing.pace(Pace::Error);
    ui.data_panel(
        "Unlock Remediation Checklist",
        &[
            (
                "Inspect keystatus",
                format!("sudo zfs list -o name,keystatus | grep {}", dataset_hint),
            ),
            (
                "Manual load-key",
                format!("sudo zfs load-key {}", dataset_hint),
            ),
            (
                "Re-forge config",
                format!(
                    "sudo zfs_beskar_key init --dataset={} --force",
                    dataset_hint
                ),
            ),
            (
                "Refresh initramfs",
                "sudo dracut -f  # ensure Beskar module in recovery image".to_string(),
            ),
            ("Review config", format!("sudo editor {}", cfg_hint)),
        ],
    );
    ui.note("When each item reports green, rerun the vault drill to confirm the forge is stable.");
}

fn emit_reseal_remediation(ui: &UX, timing: &Timing, base_cfg: &ConfigFile, err: &anyhow::Error) {
    let dataset_hint = base_cfg
        .policy
        .datasets
        .first()
        .cloned()
        .unwrap_or_else(|| "<dataset>".to_string());

    ui.error(&format!("Reseal attempt on the ephemeral vault failed: {}", err));
    timing.pace(Pace::Error);
    ui.data_panel(
        "Reseal Troubleshooting",
        &[
            ("Check active mounts", "sudo zfs mount".to_string()),
            (
                "Manual unload-key",
                format!("sudo zfs unload-key {}", dataset_hint),
            ),
            (
                "Identify busy consumers",
                format!("sudo fuser -vm {}", dataset_hint),
            ),
            ("Ensure dracut rebuild", "sudo dracut -f".to_string()),
        ],
    );
    ui.note("If the dataset stays busy, silence dependent services, unload again, and rerun the forge drill.");
}
