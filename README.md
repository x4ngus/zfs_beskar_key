[![Forge Verification](https://github.com/x4ngus/zfs_beskar_key/actions/workflows/rust.yml/badge.svg)](https://github.com/x4ngus/zfs_beskar_key/actions)

# **ZFS_BESKAR_KEY**

<img width="860" height="430" alt="image" src="https://github.com/user-attachments/assets/309192cc-9f2b-42ac-b36a-918083e472ef" />

A USB-first ZFS unlock companion forged for dependable, unattended boots. 
Tribute ▸ Temper ▸ Drill ▸ Diagnose ▸ Deploy.

---

## Overview

`zfs_beskar_key` unlocks encrypted ZFS datasets from a dedicated USB key.

> **Alpha build** – v1.8.1 is experimental. Validate in disposable labs, keep known-good backups, and never rely on this release without a tested recovery plan.

### Release highlights (v1.8.1)

- **USB recovery forge** – `zfs_beskar_key recover` rebuilds a Beskar token on any compatible Linux host using only the recorded Base32 recovery key. The command wipes the selected USB, recreates the filesystem, and writes the original raw key without touching the local system.
- **Passphrase fallback drill** – `init` can seal the raw key with an Armorer-approved fallback passphrase (PBKDF2-protected), and `self-test --fallback` now hides the USB to prove the passphrase path before disaster strikes.
- **Narrative + bootstrap polish** – Bootstrap now tolerates missing binaries during version detection, and all UI/menu text follows the concise bounty-hunter cadence while still ending with “This is the Way.”
- **Raw-key enforcement + cleanups** – Legacy hex flows were removed from unlock, doctor, simulation, and bootstrap; passphrase data is stored explicitly in the config; Clippy-driven cleanups trimmed dead code and tightened the dracut templates.

---

## Requirements

- ZFS CLI tools (`/sbin/zfs`, `/sbin/zpool`).
- Root access on the target host.
- Dedicated USB media for the Beskar key.
- Rust toolchain if compiling from source.

---

## Installation

### Preferred: bootstrap script

```bash
curl -fsSL https://raw.githubusercontent.com/x4ngus/zfs_beskar_key/main/scripts/bootstrap.sh | sudo bash
```

The script drives `zfs_beskar_key init` directly so the USB forge mirrors the Rust workflow—auto-detecting the dataset mounted at `/`, wiping the token, writing `/etc/zfs-beskar.toml`, installing systemd units, mounting `/run/beskar`, and refreshing either dracut or initramfs-tools images as appropriate.

### Alternative: build from source

```bash
git clone https://github.com/x4ngus/zfs_beskar_key.git
cd zfs_beskar_key
cargo build --release
sudo cp target/release/zfs_beskar_key /usr/local/bin/
sudo zfs_beskar_key
```

---

## Configuration

1. **Recommended guided mode**:
   ```bash
   sudo /usr/local/bin/zfs_beskar_key --menu
   ```
   The menu surfaces every command with prompts for first-time operators.
1. **Optional prepare USB manually** (skip if the bootstrap script already handled it):
   ```bash
   sudo parted /dev/sdb -- mklabel gpt
   sudo parted /dev/sdb -- mkpart BESKARKEY ext4 1MiB 100%
   sudo mkfs.ext4 -L BESKARKEY /dev/sdb1
   sudo mkdir -p /mnt/beskar
   sudo mount /dev/disk/by-label/BESKARKEY /mnt/beskar
   sudo openssl rand -out /mnt/beskar/rpool.keyhex 32
   sudo chmod 0400 /mnt/beskar/rpool.keyhex
   sudo umount /mnt/beskar
   ```
2. **Normalize policy and checksums**:
   ```bash
   sudo /usr/local/bin/zfs_beskar_key init --dataset=rpool/ROOT
   ```
   `init` records the dataset list, USB path, SHA-256 fingerprint, and binary location, backing up any existing config. It also prints a Base32 recovery key—store it offline so you can rebuild the USB later—and offers an optional fallback passphrase that can unlock the pool even without the USB.

---

## Validation

```bash
sudo /usr/local/bin/zfs_beskar_key doctor 
sudo /usr/local/bin/zfs_beskar_key self-test 
sudo /usr/local/bin/zfs_beskar_key self-test --fallback
```

`doctor` verifies USB presence, key integrity, config permissions, dracut modules, and systemd units. `self-test` simulates the boot unlock sequence end-to-end. Pass `--fallback` to hide the USB temporarily and prove the Armorer passphrase alone can recover the pool.

---

## Deployment

```bash
sudo /usr/local/bin/zfs_beskar_key install-units --config=/etc/zfs-beskar.toml
systemctl status beskar-load-key.service
systemctl status beskar-unlock.service
```

---

## Operations

- Rotate the key with `init --safe`, confirm prompts, rerun `doctor`, then replace the USB.
- Auto-unlock now cascades across the encryption root and its descendants (e.g., `rpool/ROOT/ubuntu_*`), retrying stubborn children with the same key to ensure the stack unlocks together.
- Use `auto-unlock --strict-usb` on a running system to mirror initramfs behaviour and confirm the USB token alone can restore the pool.
- Use `self-test --fallback` to hide the USB temporarily and prove the Armorer passphrase still recovers the pool.
- The forge installs whichever early-boot framework you use (dracut or initramfs-tools) so the strict USB unlock fires before root mounts.
- Every forge run auto-installs the Beskar loader service/hook (when dracut is present), sets `keylocation=file:///run/beskar/<key>` (or your configured path), and forces `dracut -f`, matching the dedicated `install-dracut` command.
- During boot, the loader waits for the token, mounts it at `/run/beskar`, and feeds `zfs load-key -a`; if the key never appears, Ubuntu’s native passphrase prompt still takes over.
- Launch `--menu` ▸ *Vault Drill* after hardware or initramfs changes to rehearse unlocks on a disposable pool.
- Monitor `/var/log/beskar.log` for append-only audit entries.
- Re-run `install-units` whenever datasets, USB devices, or binary paths change; `doctor` will verify unit sanity with `systemd-analyze`.

---

## Recovery

- Missing USB media triggers a secure `systemd-ask-password` prompt at boot; enter the dataset passphrase to proceed.
- After recovery login, run `doctor` to restore checksums, units, or dracut modules.
- Use `auto-unlock --json` for scripted rescue workflows.
- Lost your Beskar token? On any Linux host with this tool installed, run `sudo zfs_beskar_key recover --dataset=<encryption_root>`, select the target USB, and enter the recorded Base32 recovery key. The command wipes the token, recreates the filesystem, and rewrites the original raw key without touching the local system.

---

## Project Details

- **Current release:** v1.8.0
- **License:** MIT (see `LICENSE`)
- **Author:** Angus J.

This is the Way.
