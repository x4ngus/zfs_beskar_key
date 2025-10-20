// ============================================================================
// src/main.rs – CLI entry (USB-first with passphrase fallback)
// ============================================================================

mod cmd;
mod config;
mod ui;
mod zfs;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use rand::RngCore;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use ui::UI;
use zeroize::Zeroizing;

#[derive(Parser, Debug)]
#[command(
    name = "zfs_beskar_key",
    version,
    about = "This is the Way to manage ZFS encrypted dataset keys with USB-first auto-unlock."
)]
struct Cli {
    /// Path to config file (TOML or YAML)
    #[arg(short, long)]
    config: Option<String>,

    /// Dataset target when relevant (e.g., rpool/ROOT or rpool/ROOT/ubuntu)
    #[arg(short = 'd', long)]
    dataset: Option<String>,

    /// JSON logs / quiet handled by BESKAR_UI env; this flag forces JSON.
    #[arg(long)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate a 32-byte key and print as hex to stdout
    ForgeKey,

    /// Attach/load a key to an encrypted dataset (reads key from stdin if not provided)
    Unlock {
        /// Key in hex; if omitted, read from stdin
        #[arg(long)]
        key_hex: Option<String>,
    },

    /// Unload key for dataset
    Lock,

    /// USB-first auto unlock with passphrase fallback (for systemd at boot)
    AutoUnlock,

    /// Preflight checks
    Doctor,

    /// Generate and install systemd units for USB + AutoUnlock
    InstallUnits,
}

fn main() -> Result<()> {
    // --- Setup --------------------------------------------------------------
    let cli = Cli::parse();

    if cli.json {
        std::env::set_var("BESKAR_UI", "json");
    }
    let ui = UI::from_env();

    let cfg = if let Some(p) = &cli.config {
        Some(config::Config::load(p)?)
    } else {
        None
    };

    let timeout = Duration::from_secs(cfg.as_ref().map(|c| c.crypto.timeout_secs).unwrap_or(10));

    let zfs = if let Some(cfg) = &cfg {
        if let Some(path) = &cfg.policy.zfs_path {
            zfs::Zfs::with_path(path, timeout)?
        } else {
            zfs::Zfs::discover(timeout)?
        }
    } else {
        zfs::Zfs::discover(timeout)?
    };

    // Snapshot dataset option before match (avoid borrowing cli later)
    let dataset_opt: Option<String> = cli.dataset.clone();

    // --- Command dispatch ---------------------------------------------------
    match cli.command {
        Commands::ForgeKey => {
            let mut key = Zeroizing::new([0u8; 32]);
            OsRng.fill_bytes(&mut *key);
            println!("{}", hex::encode(&key[..]));
            ui.finish("Key forged. This is the Way.")?;
        }

        Commands::Unlock { key_hex } => {
            let dataset = resolve_dataset(dataset_opt.clone(), &cfg)?;
            ui.info(&format!("Forging bond with {}…", dataset))?;

            if zfs.is_unlocked(&dataset)? {
                ui.info("Already unlocked; no action taken.")?;
                ui.finish("The vault is open.")?;
                return Ok(());
            }

            let key_bytes = match key_hex {
                Some(h) => Zeroizing::new(hex::decode(h.trim())?),
                None => {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    Zeroizing::new(hex::decode(buf.trim())?)
                }
            };

            zfs.load_key(&dataset, &key_bytes)?;
            ui.finish("Key accepted. This is the Way.")?;
        }

        Commands::Lock => {
            let dataset = resolve_dataset(dataset_opt.clone(), &cfg)?;
            zfs.unload_key(&dataset)?;
            ui.finish("Vault sealed.")?;
        }

        Commands::AutoUnlock => {
            let dataset = resolve_dataset(dataset_opt.clone(), &cfg)?;
            auto_unlock_flow(&ui, &zfs, &cfg, &dataset)?;
        }

        Commands::Doctor => {
            let dataset = resolve_dataset(dataset_opt.clone(), &cfg)
                .unwrap_or_else(|_| String::from("rpool"));
            let enc = zfs.is_encrypted(&dataset).unwrap_or(false);
            let unlocked = zfs.is_unlocked(&dataset).unwrap_or(false);
            ui.info(&format!(
                "dataset: {} | encrypted: {} | unlocked: {}",
                dataset, enc, unlocked
            ))?;
            ui.finish("Diagnostics complete.")?;
        }

        Commands::InstallUnits => {
            install_units(&ui, &cfg)?;
            ui.finish("Systemd units installed. This is the Way.")?;
        }
    }

    Ok(())
}

// Resolve dataset from a pre-snapshotted Option<String> and config.
fn resolve_dataset(dataset_opt: Option<String>, cfg: &Option<config::Config>) -> Result<String> {
    if let Some(d) = dataset_opt {
        return Ok(d);
    }
    if let Some(cfg) = cfg {
        if let Some(d) = cfg.policy.datasets.first() {
            return Ok(d.clone());
        }
    }
    Err(anyhow!(
        "dataset not specified; use --dataset or config.policy.datasets[0]"
    ))
}

// USB-first unlock with passphrase fallback
fn auto_unlock_flow(
    ui: &UI,
    zfs: &zfs::Zfs,
    cfg: &Option<config::Config>,
    dataset: &str,
) -> Result<()> {
    ui.info(&format!("Auto-unlock sequence for {}…", dataset))?;

    if zfs.is_unlocked(dataset).unwrap_or(false) {
        ui.finish("Already unlocked; nothing to do.")?;
        return Ok(());
    }

    // 1) Try USB hex file first
    if let Some(cfg_ref) = cfg {
        let usb_path = &cfg_ref.usb.key_hex_path;
        match read_usb_hex(usb_path) {
            Ok(key_bytes) => {
                ui.info(&format!(
                    "Found USB key at {} — attempting unlock…",
                    usb_path
                ))?;
                if let Err(e) = zfs.load_key(dataset, &key_bytes) {
                    ui.warn(&format!("USB key failed: {}", e))?;
                } else {
                    ui.finish("Key accepted from USB. This is the Way.")?;
                    return Ok(());
                }
            }
            Err(e) => {
                ui.warn(&format!("USB key not usable at {}: {}", usb_path, e))?;
            }
        }
    } else {
        ui.warn("No config provided; skipping USB key attempt.")?;
    }

    // 2) Fallback (passphrase or hex) if enabled
    if let Some(cfg_ref) = cfg {
        if cfg_ref.fallback.enabled {
            let fmt = zfs
                .keyformat(dataset)
                .unwrap_or_else(|_| "unknown".to_string());
            ui.info(&format!(
                "Fallback enabled; dataset keyformat is '{}'.",
                fmt
            ))?;

            let pass = read_fallback_secret(cfg_ref, dataset)
                .context("fallback secret acquisition failed")?;

            // For both passphrase and hex keyformat, ZFS expects the corresponding text via stdin.
            // We pass exactly what the user/system provides; ZFS validates it.
            zfs.load_key(dataset, &pass)?;
            ui.finish("Fallback accepted. The vault opens.")?;
            return Ok(());
        }
    }

    Err(anyhow!(
        "auto-unlock failed: USB key unusable and fallback disabled or unsuccessful"
    ))
}

fn install_units(ui: &UI, cfg: &Option<config::Config>) -> Result<()> {
    let sysd_path = "/etc/systemd/system";
    let usb_unit = format!("{}/beskar-usb.mount", sysd_path);
    let unlock_unit = format!("{}/beskar-unlock.service", sysd_path);

    let cfg_ref = cfg.as_ref().ok_or_else(|| anyhow!("config required"))?;
    let usb_uuid = get_usb_uuid(&cfg_ref.usb.key_hex_path)?;

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
ExecStart=/usr/local/bin/zfs_beskar_key auto-unlock --config={cfg_path} --dataset={dataset}

[Install]
WantedBy=zfs-mount.service
"#,
        cfg_path = cfg_ref_path(),
        dataset = cfg_ref
            .policy
            .datasets
            .first()
            .unwrap_or(&"rpool/ROOT".to_string())
    );

    write_unit(&usb_unit, &mount_content)?;
    write_unit(&unlock_unit, &unlock_content)?;

    ui.info("Reloading systemd daemon and enabling units…")?;
    crate::cmd::Cmd::new_allowlisted("/bin/systemctl", Duration::from_secs(5))?
        .run(&["daemon-reload"], None)?;
    crate::cmd::Cmd::new_allowlisted("/bin/systemctl", Duration::from_secs(5))?.run(
        &["enable", "beskar-usb.mount", "beskar-unlock.service"],
        None,
    )?;

    Ok(())
}

fn write_unit(path: &str, content: &str) -> Result<()> {
    let p = Path::new(path);
    let mut f = File::create(p).with_context(|| format!("create {}", path))?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

fn get_usb_uuid(_key_path: &str) -> Result<String> {
    // naive: extract device by label from key path /run/beskar/...
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

fn cfg_ref_path() -> String {
    // default fallback path
    "/etc/zfs-beskar.toml".to_string()
}

fn read_usb_hex(path: &str) -> Result<Zeroizing<Vec<u8>>> {
    let data = fs::read_to_string(path).with_context(|| format!("read usb hex key: {}", path))?;
    let trimmed = data.trim();
    let decoded = hex::decode(trimmed).context("decode usb hex")?;
    Ok(Zeroizing::new(decoded))
}

/// Acquire a fallback secret via systemd-ask-password (preferred at boot), or stdin if interactive.
/// Returns the raw bytes that will be fed to `zfs load-key` (with newline added later by zfs.rs)
fn read_fallback_secret(cfg: &config::Config, dataset: &str) -> Result<Zeroizing<Vec<u8>>> {
    if cfg.fallback.askpass {
        if let Some(path) = &cfg.fallback.askpass_path {
            // Use allowlisted command runner
            let cmd =
                cmd::Cmd::new_allowlisted(path, Duration::from_secs(cfg.crypto.timeout_secs))?;
            let prompt = format!("BESKAR passphrase (or hex) for {}", dataset);
            let out = cmd.run(&[&prompt], None).context("systemd-ask-password")?;
            if out.status != 0 {
                return Err(anyhow!("ask-password failed: {}", out.stderr));
            }
            return Ok(Zeroizing::new(out.stdout.trim_end().as_bytes().to_vec()));
        }
    }
    // Fallback to stdin (interactive)
    use std::io::{self, Read};
    eprintln!("Enter fallback passphrase (or hex), then <Enter>:");
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(Zeroizing::new(buf.trim_end().as_bytes().to_vec()))
}
