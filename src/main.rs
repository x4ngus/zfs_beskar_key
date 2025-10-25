// ============================================================================
// src/main.rs – CLI entry (USB-first with passphrase fallback) + Menu dispatch
// ============================================================================

mod cmd;
mod config;
mod dracut;
mod menu;
mod ui;
mod util;
mod zfs;

use crate::cmd::unlock::UnlockOptions;
use crate::config::ConfigFile;
use crate::util::binary::determine_binary_path;
use crate::util::keyfile::ensure_raw_key_file;
use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use rand::RngCore;
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ui::{Pace, Timing, UX};
use zeroize::Zeroizing;

// ----------------------------------------------------------------------------
// CLI
// ----------------------------------------------------------------------------
#[derive(Parser, Debug)]
#[command(
    name = "zfs_beskar_key",
    version,
    about = "Manage ZFS encrypted dataset keys with USB-first auto-unlock."
)]
struct Cli {
    /// Path to config file (TOML or YAML)
    #[arg(short, long, global = true, default_value = "/etc/zfs-beskar.toml")]
    config: String,

    /// Dataset target when relevant (e.g., rpool/ROOT or rpool/ROOT/ubuntu)
    #[arg(short = 'd', long)]
    dataset: Option<String>,

    /// Force JSON logs (legacy env compatibility)
    #[arg(long, global = true)]
    json: bool,

    /// Launch interactive menu when no subcommand provided
    #[arg(long)]
    menu: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init {
        /// Explicit USB block device (e.g., /dev/sdb). Prompts if omitted.
        #[arg(long)]
        usb_device: Option<String>,

        /// Override key file output path (defaults to /run/beskar/<dataset>.keyhex).
        #[arg(long)]
        key_path: Option<PathBuf>,

        /// Safe mode: prompt before each forge phase and skip forced wipe.
        #[arg(long)]
        safe: bool,
    },
    ForgeKey,
    Unlock,
    Lock,
    AutoUnlock {
        /// USB-only mode for initramfs: disable passphrase fallback.
        #[arg(long)]
        strict_usb: bool,
    },
    Doctor,
    Recover,
    InstallUnits,
    InstallDracut,
    SelfTest,
}

// ----------------------------------------------------------------------------
// main()
// ----------------------------------------------------------------------------
fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.json {
        std::env::set_var("BESKAR_UI", "json");
    }

    // New UI layer (no from_env in UX)
    let ui = UX::new(false, false);
    let timing = Timing::new(false, false);

    // ------------------------------------------------------------------------
    // Ensure config file exists
    // ------------------------------------------------------------------------
    let cfg_path = Path::new(&cli.config);
    if !cfg_path.exists() {
        ui.warn(&format!(
            "Forge ledger missing at {} — I will inscribe a starter creed.",
            cfg_path.display()
        ));
        let default_cfg = r#"[crypto]
timeout_secs = 10

[usb]
key_hex_path = "/run/beskar/key.hex"

[policy]
zfs_path = "/sbin/zfs"
binary_path = "/usr/local/bin/zfs_beskar_key"
datasets = ["rpool/ROOT"]

[fallback]
enabled = true
askpass = true
askpass_path = "/usr/bin/systemd-ask-password"
"#;

        let mut f = File::create(cfg_path)
            .with_context(|| format!("create default config at {}", cfg_path.display()))?;
        f.write_all(default_cfg.as_bytes())?;
        fs::set_permissions(cfg_path, fs::Permissions::from_mode(0o600))
            .context("set config permissions")?;
        ui.info(&format!(
            "Template etched at {}. Inspect every value before the next muster.",
            cfg_path.display()
        ));
    }

    // Load config
    let cfg: ConfigFile = ConfigFile::load(&cli.config)?;

    // ------------------------------------------------------------------------
    // Command dispatch or menu
    // ------------------------------------------------------------------------
    if let Some(ref command) = cli.command {
        dispatch_command(command, &ui, &timing, &cli, &cfg)?;
    } else if cli.menu {
        if let Some(choice) = menu::show_main_menu(&ui, &timing) {
            dispatch_menu_choice(choice, &ui, &timing, &cli, &cfg)?;
        }
    } else {
        // No subcommand: fall back to menu
        if let Some(choice) = menu::show_main_menu(&ui, &timing) {
            dispatch_menu_choice(choice, &ui, &timing, &cli, &cfg)?;
        }
    }

    Ok(())
}

// ----------------------------------------------------------------------------
// Dispatchers
// ----------------------------------------------------------------------------
fn dispatch_command(
    command: &Commands,
    ui: &UX,
    timing: &Timing,
    cli: &Cli,
    cfg: &ConfigFile,
) -> Result<()> {
    match command {
        Commands::Init {
            usb_device,
            key_path,
            safe,
        } => {
            let opts = cmd::init::InitOptions {
                pool: cli.dataset.clone(),
                usb_device: usb_device.clone(),
                key_path: key_path.clone(),
                force: !safe,
                auto_unlock: true,
                confirm_each_phase: *safe,
            };
            cmd::init::run_init(ui, timing, opts)?;
        }
        Commands::ForgeKey => {
            let mut key = Zeroizing::new([0u8; 32]);
            OsRng.fill_bytes(&mut *key);
            println!("{}", hex::encode(&key[..]));
            ui.success("Raw beskar drawn into key form. This is the Way.");
            timing.pace(Pace::Prompt);
        }

        Commands::Unlock => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            cmd::unlock::run_unlock(ui, timing, cfg, &dataset, UnlockOptions::default())?;
        }

        Commands::Lock => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            let timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
            let zfs = if let Some(path) = &cfg.policy.zfs_path {
                zfs::Zfs::with_path(path, timeout)?
            } else {
                zfs::Zfs::discover(timeout)?
            };
            let enc_root = determine_encryption_root(&zfs, &dataset, ui);
            zfs.unload_key(&enc_root)?;
            ui.success(&format!("Vault sealed tight around {}.", enc_root));
            timing.pace(Pace::Critical);
        }

        Commands::AutoUnlock { strict_usb } => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            let opts = UnlockOptions {
                strict_usb: *strict_usb,
            };
            cmd::unlock::run_unlock(ui, timing, cfg, &dataset, opts)?;
        }

        Commands::Recover => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            cmd::recover::run_recover(ui, timing, &dataset)?;
            timing.pace(Pace::Prompt);
        }

        Commands::Doctor => {
            cmd::doctor::run_doctor(ui, timing)?;
        }

        Commands::InstallUnits => {
            let binary_path = determine_binary_path(Some(cfg))?;
            cmd::repair::install_units(ui, cfg, &binary_path)?;
            ui.success("Systemd sentries posted. This is the Way.");
            timing.pace(Pace::Prompt);
        }
        Commands::InstallDracut => {
            cmd::dracut_install::run(ui, cfg, cli.dataset.as_deref(), None)?;
            timing.pace(Pace::Prompt);
        }

        Commands::SelfTest => {
            ui.info("Initiating beskar self-test sequence…");
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            let timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
            let zfs = if let Some(path) = &cfg.policy.zfs_path {
                zfs::Zfs::with_path(path, timeout)?
            } else {
                zfs::Zfs::discover(timeout)?
            };
            let enc_root = zfs.encryption_root(&dataset).unwrap_or(dataset.clone());
            ui.info(&format!("Encryption root confirmed as {}.", enc_root));
            let _ = zfs.unload_key(&enc_root);
            if !zfs.is_unlocked(&enc_root)? {
                ui.info("Key withdrawn from memory space.");
            }
            match cmd::unlock::run_unlock(ui, timing, cfg, &enc_root, UnlockOptions::default()) {
                Ok(_) => {
                    ui.success("Self-test passed; the auto-unlock path holds.");
                    timing.pace(Pace::Prompt);
                }
                Err(e) => {
                    ui.error(&format!(
                        "Self-test failed ({}). Inspect the forge logs and remediate.",
                        e
                    ));
                    timing.pace(Pace::Error);
                }
            }
        }
    }
    Ok(())
}

fn dispatch_menu_choice(
    choice: menu::MenuChoice,
    ui: &UX,
    timing: &Timing,
    cli: &Cli,
    cfg: &ConfigFile,
) -> Result<()> {
    match choice {
        menu::MenuChoice::Init => {
            let opts = cmd::init::InitOptions {
                pool: cli.dataset.clone(),
                usb_device: None,
                key_path: None,
                force: true,
                auto_unlock: true,
                confirm_each_phase: false,
            };
            cmd::init::run_init(ui, timing, opts)?;
        }
        menu::MenuChoice::InitSafe => {
            let opts = cmd::init::InitOptions {
                pool: cli.dataset.clone(),
                usb_device: None,
                key_path: None,
                force: false,
                auto_unlock: true,
                confirm_each_phase: true,
            };
            cmd::init::run_init(ui, timing, opts)?;
        }
        menu::MenuChoice::VaultDrill => {
            cmd::simulate::run_vault_drill(ui, timing, cfg)?;
        }
        menu::MenuChoice::Recover => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            cmd::recover::run_recover(ui, timing, &dataset)?;
        }
        menu::MenuChoice::Doctor => {
            cmd::doctor::run_doctor(ui, timing)?;
        }
        menu::MenuChoice::Quit => {
            ui.info("Forge console banked. Return with new orders.");
            std::process::exit(0);
        }
    }
    Ok(())
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------
fn resolve_dataset(dataset_opt: &Option<String>, cfg: &ConfigFile) -> Result<String> {
    if let Some(d) = dataset_opt {
        Ok(d.clone())
    } else if let Some(d) = cfg.policy.datasets.first() {
        Ok(d.clone())
    } else {
        Err(anyhow!(
            "dataset not specified; use --dataset or config.policy.datasets[0]"
        ))
    }
}

fn determine_encryption_root(zfs: &impl ZfsCryptoOps, dataset: &str, ui: &UX) -> String {
    match zfs.encryption_root(dataset) {
        Ok(root) => {
            if root != dataset {
                ui.info(&format!(
                    "Dataset {} draws its ward from encryption root {}.",
                    dataset, root
                ));
            } else {
                ui.info(&format!("Encryption root stands as {}.", root));
            }
            root
        }
        Err(e) => {
            ui.warn(&format!(
                "Unable to read the encryption lineage for {} ({}). Proceeding against the dataset directly.",
                dataset, e
            ));
            dataset.to_string()
        }
    }
}

trait ZfsCryptoOps {
    fn encryption_root(&self, dataset: &str) -> Result<String>;
}

impl ZfsCryptoOps for zfs::Zfs {
    fn encryption_root(&self, dataset: &str) -> Result<String> {
        zfs::Zfs::encryption_root(self, dataset)
    }
}

#[cfg(test)]
trait TestZfsCryptoOps: ZfsCryptoOps {
    fn is_unlocked(&self, dataset: &str) -> Result<bool>;
    fn load_key(&self, dataset: &str, key: &[u8]) -> Result<()>;
}

// Auto-unlock flow
#[cfg(test)]
fn auto_unlock_with(
    zfs: &impl TestZfsCryptoOps,
    ui: &UX,
    cfg: &ConfigFile,
    dataset: &str,
) -> Result<()> {
    let enc_root = determine_encryption_root(zfs, dataset, ui);

    let unlocked = match zfs.is_unlocked(&enc_root) {
        Ok(state) => state,
        Err(e) => {
            ui.warn(&format!(
                "Keystatus for {} unknown ({}). Assuming locked.",
                enc_root, e
            ));
            false
        }
    };
    if unlocked {
        ui.info(&format!(
            "{} already unlocked; running USB self-test.",
            enc_root
        ));
    }

    let usb_path = Path::new(&cfg.usb.key_hex_path);
    let raw_key_bytes = ensure_raw_key_file(usb_path)
        .with_context(|| format!("normalize USB key file {}", usb_path.display()))?
        .raw;

    let mut hasher = Sha256::new();
    hasher.update(&raw_key_bytes);
    let actual_hash = hex::encode(hasher.finalize());

    if let Some(expected_hash) = cfg.usb.expected_sha256.as_ref() {
        if !actual_hash.eq_ignore_ascii_case(expected_hash) {
            ui.error("USB key checksum mismatch detected.");
            ui.warn(&format!(
                "Expected: {}\nFound:    {}",
                expected_hash, actual_hash
            ));
            return Err(anyhow!("USB key checksum mismatch"));
        }
        ui.info("USB key checksum verified (SHA-256 match).");
    } else {
        ui.warn("No stored SHA-256; authenticity skipped.");
    }

    if !unlocked {
        ui.info(&format!("Unlocking {} with USB key.", enc_root));
        zfs.load_key(&enc_root, &raw_key_bytes)?;
        ui.success(&format!(
            "Key accepted from the tribute. {} now stands unlocked. This is the Way.",
            enc_root
        ));
    } else {
        ui.success("Self-test complete; the beskar key holds true.");
    }

    Ok(())
}

// Systemd install preserved
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigFile, CryptoCfg, Fallback, Policy, Usb};
    use anyhow::Result;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    #[derive(Default)]
    struct MockZfs {
        root: String,
        unlocked: bool,
        load_calls: Mutex<Vec<String>>,
        is_unlocked_calls: Mutex<Vec<String>>,
        encryption_queries: Mutex<Vec<String>>,
    }

    impl MockZfs {
        fn new(root: &str, unlocked: bool) -> Self {
            Self {
                root: root.to_string(),
                unlocked,
                load_calls: Mutex::new(Vec::new()),
                is_unlocked_calls: Mutex::new(Vec::new()),
                encryption_queries: Mutex::new(Vec::new()),
            }
        }
    }

    impl ZfsCryptoOps for MockZfs {
        fn encryption_root(&self, dataset: &str) -> Result<String> {
            self.encryption_queries
                .lock()
                .unwrap()
                .push(dataset.to_string());
            Ok(self.root.clone())
        }
    }

    impl TestZfsCryptoOps for MockZfs {
        fn is_unlocked(&self, dataset: &str) -> Result<bool> {
            self.is_unlocked_calls
                .lock()
                .unwrap()
                .push(dataset.to_string());
            Ok(self.unlocked)
        }

        fn load_key(&self, dataset: &str, _key: &[u8]) -> Result<()> {
            self.load_calls.lock().unwrap().push(dataset.to_string());
            Ok(())
        }
    }

    #[test]
    fn auto_unlock_targets_encryption_root_when_dataset_inherits() -> Result<()> {
        let mut key_file = NamedTempFile::new()?;
        let hex_key = "ab".repeat(32);
        writeln!(key_file, "{hex_key}")?;

        let cfg = ConfigFile {
            policy: Policy {
                datasets: vec!["rpool/ROOT/ubuntu".into()],
                zfs_path: None,
                binary_path: None,
                allow_root: false,
            },
            crypto: CryptoCfg { timeout_secs: 5 },
            usb: Usb {
                key_hex_path: key_file.path().to_string_lossy().into_owned(),
                expected_sha256: None,
            },
            fallback: Fallback::default(),
            path: PathBuf::from("/tmp/test-config"),
        };

        let ui = UX::new(false, false);
        let mock = MockZfs::new("rpool/ROOT", false);

        auto_unlock_with(&mock, &ui, &cfg, "rpool/ROOT/ubuntu")?;

        let load_calls = mock.load_calls.lock().unwrap().clone();
        assert_eq!(load_calls, vec!["rpool/ROOT"]);

        let is_calls = mock.is_unlocked_calls.lock().unwrap().clone();
        assert_eq!(is_calls, vec!["rpool/ROOT"]);

        let queries = mock.encryption_queries.lock().unwrap().clone();
        assert_eq!(queries, vec!["rpool/ROOT/ubuntu"]);

        Ok(())
    }
}
