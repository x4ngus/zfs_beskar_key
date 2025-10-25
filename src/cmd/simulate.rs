// ============================================================================
// src/cmd/simulate.rs – Ephemeral ZFS vault simulation for menu demos
// ============================================================================

use crate::cmd::{unlock::UnlockOptions, Cmd, OutputData};
use crate::config::{ConfigFile, CryptoCfg, Fallback, Policy, Usb};
use crate::ui::{Pace, Timing, UX};
use crate::zfs::Zfs;
use anyhow::{anyhow, Context, Result};
use nanoid::nanoid;
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
    ui.phase("Holoforge // Prep");

    let mut sim = match VaultSimulation::prepare(base_cfg) {
        Ok(sim) => sim,
        Err(err) => {
            emit_preflight_remediation(ui, timing, base_cfg, &err);
            return Err(err);
        }
    };
    timing.pace(Pace::Info);

    ui.info(&format!(
        "Holoforge basin {} hammered atop {}.",
        sim.pool_name,
        sim.image_path.display()
    ));
    ui.note("Vault sealed to mimic cold boot.");
    sim.ensure_locked()?;
    timing.pace(Pace::Prompt);

    ui.phase("Holoforge // USB Drill");
    match crate::cmd::unlock::run_unlock(
        ui,
        timing,
        &sim.config,
        &sim.dataset_name,
        UnlockOptions::default(),
    ) {
        Ok(_) => {
            let zfs = sim.zfs()?;
            if zfs.is_unlocked(&sim.dataset_name)? {
                ui.success("Drill complete — token proven.");
            } else {
                ui.warn("Vault status hazy; inspect `zfs keystatus`.");
            }
        }
        Err(err) => {
            emit_unlock_remediation(ui, timing, base_cfg, &err);
            return Err(err);
        }
    }
    timing.pace(Pace::Info);

    ui.phase("Holoforge // Reseal Key");
    let zfs = sim.zfs()?;
    if let Err(err) = zfs.unload_key(&sim.dataset_name) {
        emit_reseal_remediation(ui, timing, base_cfg, &err);
        return Err(err);
    }
    ui.success("Holoforge vault resealed; echoes cleared.");
    timing.pace(Pace::Critical);

    ui.phase("Holoforge // Cleanup");
    sim.teardown()?;
    timing.pace(Pace::Info);

    ui.phase("Holoforge // Debrief");
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
    ui.note("If any step falters, rerun with --force and inspect `/etc/zfs-beskar.toml.bak-*`.");
    ui.success("Simulation complete. This is the Way.");
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

        let pool_name = format!("beskar_sim_{}", nanoid!(6).to_lowercase());
        let dataset_name = format!("{}/forge", pool_name);

        let raw_key_path = temp_dir.path().join("beskar.key");
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
                key_hex_path: raw_key_path.to_string_lossy().into_owned(),
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
        let cmd = Cmd::new_allowlisted(&self.zfs_path, self.timeout)?;
        cmd.run(&["destroy", "-f", &self.dataset_name], None)
            .context("destroy simulated dataset")
    }

    fn destroy_pool(&self) -> Result<OutputData> {
        let cmd = Cmd::new_allowlisted(&self.zpool_path, self.timeout)?;
        cmd.run(&["destroy", "-f", &self.pool_name], None)
            .context("destroy simulated pool")
    }
}

fn emit_preflight_remediation(ui: &UX, timing: &Timing, cfg: &ConfigFile, err: &anyhow::Error) {
    ui.error(&format!(
        "Simulation prep failed: {}. Confirm zfs/zpool binaries and free space.",
        err
    ));
    ui.note(&format!(
        "Config path: {}. Dataset list: {:?}.",
        cfg.path.display(),
        cfg.policy.datasets
    ));
    timing.pace(Pace::Error);
}

fn emit_unlock_remediation(ui: &UX, timing: &Timing, cfg: &ConfigFile, err: &anyhow::Error) {
    ui.error(&format!("Vault drill unlock failed: {}", err));
    ui.note("Review `journalctl -b` and rerun `zfs_beskar_key doctor`.");
    ui.note(&format!(
        "Using simulated config {}; USB path {}.",
        cfg.path.display(),
        cfg.usb.key_hex_path
    ));
    timing.pace(Pace::Error);
}

fn emit_reseal_remediation(ui: &UX, timing: &Timing, _cfg: &ConfigFile, err: &anyhow::Error) {
    ui.error(&format!("Unable to reseal simulated dataset: {}", err));
    ui.note("Run `zfs unload-key <dataset>` manually before retrying.");
    timing.pace(Pace::Error);
}

fn resolve_zfs_path(cfg: &ConfigFile) -> Result<String> {
    cfg.policy
        .zfs_path
        .clone()
        .ok_or_else(|| anyhow!("cfg.policy.zfs_path missing"))
}

fn resolve_zpool_path() -> Result<String> {
    for candidate in ["/sbin/zpool", "/usr/sbin/zpool", "/usr/bin/zpool"] {
        if Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }
    }
    Err(anyhow!("zpool binary not found on standard paths"))
}
