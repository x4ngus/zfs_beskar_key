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
            let timeout = Duration::from_secs(if cfg.crypto.timeout_secs > 0 {
                cfg.crypto.timeout_secs
            } else {
                10
            });
            let zfs = if let Some(path) = &cfg.policy.zfs_path {
                zfs::Zfs::with_path(path, timeout)?
            } else {
                zfs::Zfs::discover(timeout)?
            };
            zfs.unload_key(&dataset)?;
            ui.success("Vault sealed.");
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
            install_units(ui, cfg)?;
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
                offer_dracut_rebuild: false,
            };
            cmd::init::run_init(ui, timing, opts)?;
        }
        menu::MenuChoice::Unlock => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            cmd::unlock::run_unlock(ui, timing, cfg, &dataset)?;
        }
        menu::MenuChoice::Lock => {
            let dataset = resolve_dataset(&cli.dataset, cfg)?;
            let timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
            let zfs = if let Some(path) = &cfg.policy.zfs_path {
                zfs::Zfs::with_path(path, timeout)?
            } else {
                zfs::Zfs::discover(timeout)?
            };
            zfs.unload_key(&dataset)?;
            ui.success("Vault sealed.");
            timing.pace(Pace::Critical);
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

// Auto-unlock (unchanged logic)
fn auto_unlock_flow(ui: &UX, cfg: &ConfigFile, dataset: &str) -> Result<()> {
    ui.info(&format!("Auto-unlock sequence for {}…", dataset));

    let timeout = Duration::from_secs(cfg.crypto.timeout_secs.max(1));
    let zfs = if let Some(path) = &cfg.policy.zfs_path {
        zfs::Zfs::with_path(path, timeout)?
    } else {
        zfs::Zfs::discover(timeout)?
    };

    let unlocked = zfs.is_unlocked(dataset).unwrap_or(false);
    if unlocked {
        ui.info("Dataset already unlocked. Running USB key self-test…");
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
        ui.info("Attempting unlock using verified USB key…");
        zfs.load_key(dataset, &raw_key_bytes)?;
        ui.success("Key accepted from USB. This is the Way.");
    } else {
        ui.success("Self-test complete: USB key valid and verified.");
    }

    Ok(())
}

// Systemd install preserved
fn install_units(ui: &UX, cfg: &ConfigFile) -> Result<()> {
    let sysd_path = "/etc/systemd/system";
    let usb_unit = format!("{}/beskar-usb.mount", sysd_path);
    let unlock_unit = format!("{}/beskar-unlock.service", sysd_path);
    let usb_uuid = get_usb_uuid(&cfg.usb.key_hex_path)?;

    let mount_content = format!(
        r#"[Unit]
Description=Mount BESKAR key USB
DefaultDependencies=no
Before=local-fs-pre.target

[Mount]
What=/dev/disk/by-uuid/{uuid}
Where=/run/beskar
Type=ext4
Options=ro,nosuid,nodev,noexec,x-systemd.device-timeout=5s

[Install]
WantedBy=local-fs-pre.target
"#,
        uuid = usb_uuid
    );

    let unlock_content = format!(
        r#"[Unit]
Description=Unlock ZFS dataset with BESKAR USB key
DefaultDependencies=no
After=beskar-usb.mount zfs-import-cache.service zfs-import.target
Requires=beskar-usb.mount
Before=zfs-load-key.service zfs-mount.service local-fs.target

[Service]
Type=oneshot
User=root
Group=root
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
NoNewPrivileges=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true
RestrictRealtime=true
RestrictNamespaces=true
IPAddressDeny=any
ReadWritePaths=/dev
ReadOnlyPaths=/run/beskar
TemporaryFileSystem=/tmp:ro
UMask=0077
ExecStart=/usr/local/bin/zfs_beskar_key auto-unlock --config=/etc/zfs-beskar.toml --dataset={dataset}

[Install]
WantedBy=zfs-mount.service
"#,
        dataset = cfg
            .policy
            .datasets
            .first()
            .unwrap_or(&"rpool/ROOT".to_string())
    );

    write_unit(&usb_unit, &mount_content)?;
    write_unit(&unlock_unit, &unlock_content)?;

    ui.info("Reloading systemd daemon and enabling units…");
    crate::cmd::Cmd::new_allowlisted("/bin/systemctl", Duration::from_secs(5))?
        .run(&["daemon-reload"], None)?;
    crate::cmd::Cmd::new_allowlisted("/bin/systemctl", Duration::from_secs(5))?.run(
        &["enable", "beskar-usb.mount", "beskar-unlock.service"],
        None,
    )?;
    Ok(())
}

fn write_unit(path: &str, content: &str) -> Result<()> {
    let mut f = File::create(path).with_context(|| format!("create {}", path))?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

fn get_usb_uuid(_key_path: &str) -> Result<String> {
    let output = std::process::Command::new("blkid")
        .output()
        .context("detect USB UUID")?;
    let s = String::from_utf8_lossy(&output.stdout);
    for line in s.lines() {
        if line.contains("BESKARKEY") {
            if let Some(u) = line.split("UUID=\"").nth(1) {
                return Ok(u.split('"').next().unwrap_or("").to_string());
            }
        }
    }
    Err(anyhow!("could not detect BESKARKEY UUID"))
}
