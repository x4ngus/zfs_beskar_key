// ============================================================================
// src/main.rs – CLI entry (USB-first with passphrase fallback) + Menu dispatch
// ============================================================================

mod cmd;
mod config;
mod menu;
mod ui;
mod util;
mod zfs;

use crate::config::ConfigFile;
use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
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
    ForgeKey,
    Unlock {
        #[arg(long)]
        key_hex: Option<String>,
    },
    Lock,
    AutoUnlock,
    Doctor,
    InstallUnits,
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
            "No config found at {} — creating default template.",
            cfg_path.display()
        ));
        let default_cfg = r#"[crypto]
timeout_secs = 10

[usb]
key_hex_path = "/run/beskar/key.hex"

[policy]
zfs_path = "/sbin/zfs"
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
            "Default config written to {} — review before next reboot.",
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
        Commands::ForgeKey => {
            let mut key = Zeroizing::new([0u8; 32]);
            OsRng.fill_bytes(&mut *key);
            println!("{}", hex::encode(&key[..]));
            ui.success("Key forged. This is the Way.");
            timing.pace(Pace::Prompt);
        }

        Commands::Unlock { key_hex: _ } => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            cmd::unlock::run_unlock(ui, timing, cfg, &dataset)?;
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
            ui.success(&format!("Vault sealed for {}.", enc_root));
            timing.pace(Pace::Critical);
        }

        Commands::AutoUnlock => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            auto_unlock_flow(ui, cfg, &dataset)?;
        }

        Commands::Doctor => {
            cmd::doctor::run_doctor(ui, timing)?;
        }

        Commands::InstallUnits => {
            cmd::repair::install_units(ui, cfg)?;
            ui.success("Systemd units installed. This is the Way.");
            timing.pace(Pace::Prompt);
        }

        Commands::SelfTest => {
            ui.info("Running BESKAR self-test…");
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            let timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
            let zfs = if let Some(path) = &cfg.policy.zfs_path {
                zfs::Zfs::with_path(path, timeout)?
            } else {
                zfs::Zfs::discover(timeout)?
            };
            let enc_root = zfs.encryption_root(&dataset).unwrap_or(dataset.clone());
            ui.info(&format!("Encryption root: {}", enc_root));
            let _ = zfs.unload_key(&enc_root);
            if !zfs.is_unlocked(&enc_root)? {
                ui.info("Key unloaded successfully.");
            }
            match cmd::unlock::run_unlock(ui, timing, cfg, &enc_root) {
                Ok(_) => {
                    ui.success("Self-test PASSED. Auto-unlock logic verified.");
                    timing.pace(Pace::Prompt);
                }
                Err(e) => {
                    ui.error(&format!("Self-test FAILED: {}", e));
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
                force: false,
                auto_unlock: true,
                offer_dracut_rebuild: true,
            };
            cmd::init::run_init(ui, timing, opts)?;
        }
        menu::MenuChoice::VaultDrill => {
            cmd::simulate::run_vault_drill(ui, timing, cfg)?;
        }
        menu::MenuChoice::Status => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            let timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
            let zfs = if let Some(path) = &cfg.policy.zfs_path {
                zfs::Zfs::with_path(path, timeout)?
            } else {
                zfs::Zfs::discover(timeout)?
            };
            let encrypted = zfs.is_encrypted(&dataset).unwrap_or(false);
            let unlocked = zfs.is_unlocked(&dataset).unwrap_or(false);
            let enc_root = zfs.encryption_root(&dataset).unwrap_or(dataset.clone());
            ui.info(&format!(
                "Dataset: {}\n- Encryption root: {}\n- Encrypted: {}\n- Unlocked: {}",
                dataset, enc_root, encrypted, unlocked
            ));
            timing.pace(Pace::Info);
        }
        menu::MenuChoice::Doctor => {
            cmd::doctor::run_doctor(ui, timing)?;
        }
        menu::MenuChoice::Quit => {
            ui.info("Exiting console.");
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
                    "Dataset {} inherits encryption from root {}.",
                    dataset, root
                ));
            } else {
                ui.info(&format!("Encryption root identified as {}.", root));
            }
            root
        }
        Err(e) => {
            ui.warn(&format!(
                "Unable to determine encryption root for {}: {}. Using dataset directly.",
                dataset, e
            ));
            dataset.to_string()
        }
    }
}

trait ZfsCryptoOps {
    fn is_unlocked(&self, dataset: &str) -> Result<bool>;
    fn encryption_root(&self, dataset: &str) -> Result<String>;
    fn load_key(&self, dataset: &str, key: &[u8]) -> Result<()>;
}

impl ZfsCryptoOps for zfs::Zfs {
    fn is_unlocked(&self, dataset: &str) -> Result<bool> {
        zfs::Zfs::is_unlocked(self, dataset)
    }

    fn encryption_root(&self, dataset: &str) -> Result<String> {
        zfs::Zfs::encryption_root(self, dataset)
    }

    fn load_key(&self, dataset: &str, key: &[u8]) -> Result<()> {
        zfs::Zfs::load_key(self, dataset, key)
    }
}

// Auto-unlock flow
fn auto_unlock_flow(ui: &UX, cfg: &ConfigFile, dataset: &str) -> Result<()> {
    ui.info(&format!("Auto-unlock sequence for {}…", dataset));

    let timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
    let zfs = if let Some(path) = &cfg.policy.zfs_path {
        zfs::Zfs::with_path(path, timeout)?
    } else {
        zfs::Zfs::discover(timeout)?
    };

    auto_unlock_with(&zfs, ui, cfg, dataset)
}

fn auto_unlock_with(
    zfs: &impl ZfsCryptoOps,
    ui: &UX,
    cfg: &ConfigFile,
    dataset: &str,
) -> Result<()> {
    let enc_root = determine_encryption_root(zfs, dataset, ui);

    let unlocked = match zfs.is_unlocked(&enc_root) {
        Ok(state) => state,
        Err(e) => {
            ui.warn(&format!(
                "Unable to determine current keystatus for {}: {}. Assuming locked.",
                enc_root, e
            ));
            false
        }
    };
    if unlocked {
        ui.info(&format!(
            "Encryption root {} already unlocked. Running USB key self-test…",
            enc_root
        ));
    }

    let usb_path = &cfg.usb.key_hex_path;
    let raw_text = fs::read_to_string(usb_path)
        .with_context(|| format!("Failed to read USB key file {}", usb_path))?;

    let cleaned: String = raw_text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() != 64 {
        ui.error(&format!(
            "USB key malformed: expected 64 hex chars, found {}.",
            cleaned.len()
        ));
        ui.warn("Tip: regenerate with `openssl rand -hex 32 | sudo tee /run/beskar/key.hex`");
        return Err(anyhow!(
            "USB key integrity check failed — malformed or truncated: {}",
            usb_path
        ));
    }

    let raw_key_bytes =
        hex::decode(&cleaned).context(format!("Failed to decode hex data in {}", usb_path))?;

    let mut hasher = Sha256::new();
    hasher.update(&raw_key_bytes);
    let actual_hash = hex::encode(hasher.finalize());

    if let Some(expected_hash) = cfg.usb.expected_sha256.as_ref() {
        if !actual_hash.eq_ignore_ascii_case(expected_hash) {
            ui.error("❌ USB key checksum mismatch!");
            ui.warn(&format!(
                "Expected: {}\nFound:    {}",
                expected_hash, actual_hash
            ));
            return Err(anyhow!("USB key checksum mismatch"));
        }
        ui.info("✅ USB key checksum verified (SHA-256 match).");
    } else {
        ui.warn("No expected SHA-256 in config.usb.expected_sha256 — skipping authenticity check.");
    }

    if !unlocked {
        ui.info(&format!(
            "Attempting unlock of encryption root {} using verified USB key…",
            enc_root
        ));
        zfs.load_key(&enc_root, &raw_key_bytes)?;
        ui.success(&format!(
            "Key accepted from USB. {} unlocked. This is the Way.",
            enc_root
        ));
    } else {
        ui.success("Self-test complete: USB key valid and verified.");
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
        fn is_unlocked(&self, dataset: &str) -> Result<bool> {
            self.is_unlocked_calls
                .lock()
                .unwrap()
                .push(dataset.to_string());
            Ok(self.unlocked)
        }

        fn encryption_root(&self, dataset: &str) -> Result<String> {
            self.encryption_queries
                .lock()
                .unwrap()
                .push(dataset.to_string());
            Ok(self.root.clone())
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
