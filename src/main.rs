// ============================================================================
// src/main.rs – CLI entry
// ============================================================================

mod cmd;
mod config;
mod ui;
mod zfs;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
use rand::RngCore;
use std::time::Duration;
use ui::UI;
use zeroize::Zeroizing;

#[derive(Parser, Debug)]
#[command(
    name = "zfs_beskar_key",
    version,
    about = "Tasteful Mandalorian-flavoured ZFS key tool"
)]
struct Cli {
    /// Path to config file (TOML or YAML)
    #[arg(short, long)]
    config: Option<String>,

    /// Dataset target when relevant (e.g., rpool/ROOT)
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

    /// Preflight checks
    Doctor,
}

fn main() -> Result<()> {
    // --- Setup --------------------------------------------------------------
    let cli = Cli::parse();

    if cli.json {
        std::env::set_var("BESKAR_UI", "json");
    }
    let ui = UI::from_env();

    // Borrow config path immutably (no move from `cli`).
    let cfg = if let Some(p) = &cli.config {
        Some(config::Config::load(p)?)
    } else {
        None
    };

    let timeout = Duration::from_secs(cfg.as_ref().map(|c| c.crypto.timeout_secs).unwrap_or(10));

    // Prepare ZFS handle (policy path or discover), no moves from `cli`.
    let zfs = if let Some(cfg) = &cfg {
        if let Some(path) = &cfg.policy.zfs_path {
            zfs::Zfs::with_path(path, timeout)?
        } else {
            zfs::Zfs::discover(timeout)?
        }
    } else {
        zfs::Zfs::discover(timeout)?
    };

    // 🔒 KEY FIX: Snapshot dataset option BEFORE matching on cli.command.
    // After this point, we never borrow `&cli` again.
    let dataset_opt: Option<String> = cli.dataset.clone();

    // --- Command dispatch ---------------------------------------------------
    match cli.command {
        Commands::ForgeKey => {
            let mut key = Zeroizing::new([0u8; 32]);
            OsRng.fill_bytes(&mut *key);
            println!("{}", hex::encode(*key));
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

        Commands::Doctor => {
            let dataset = resolve_dataset(dataset_opt.clone(), &cfg)
                .unwrap_or_else(|_| String::from("rpool"));
            let enc = zfs.is_encrypted(&dataset).unwrap_or(false);
            let unlocked = zfs.is_unlocked(&dataset).unwrap_or(false);
            ui.warn("Warning test (forging resilience check)")?;
            ui.heartbeat("doctor", Duration::from_secs(1))?;
            ui.blaster(25, "Doctor progress test")?;
            ui.info(&format!(
                "dataset: {} | encrypted: {} | unlocked: {}",
                dataset, enc, unlocked
            ))?;
            ui.error("Error test (diagnostic only)")?;
            ui.finish("Diagnostics complete.")?;
        }
    }

    Ok(())
}

// Resolve dataset from a pre-snapshotted Option<String> and config.
// This avoids borrowing `&cli` after `cli.command` has been moved.
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
