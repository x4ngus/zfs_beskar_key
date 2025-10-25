[![Forge Verification](https://github.com/x4ngus/zfs_beskar_key/actions/workflows/rust.yml/badge.svg)](https://github.com/x4ngus/zfs_beskar_key/actions)

# **ZFS_BESKAR_KEY**

<img width="860" height="430" alt="image" src="https://github.com/user-attachments/assets/309192cc-9f2b-42ac-b36a-918083e472ef" />

A USB-first ZFS unlock companion forged for dependable, unattended boots. Tribute ▸ Temper ▸ Drill ▸ Diagnose ▸ Deploy.

---

## Overview

`zfs_beskar_key` unlocks encrypted ZFS datasets from a dedicated USB key while keeping a secured passphrase fallback online. Configuration lives in `/etc/zfs-beskar.toml`; commands default to strict permissions and atomic writes.

### Release highlights (v1.8.0)

- **USB recovery forge** – A new `zfs_beskar_key recover` command (and menu item) rebuilds a Beskar token on any compatible Linux host using only the recorded Base32 recovery key. The command wipes the selected USB, recreates the filesystem, and writes the original raw key without touching the local system.
- **Base32 recovery keys** – `init` now encodes the 32-byte raw key directly (instead of generating a separate alphanumeric code), guaranteeing perfect reconstruction while remaining copy/paste friendly.
- **Narrative polish** – All UI/menu/bootstrap text now follows the concise bounty-hunter cadence: Armorer statements remain ceremonial but runtime logs are short, direct, and battle-ready. Every core command still ends with “This is the Way.”
- **Raw-key enforcement everywhere** – Legacy hex flows were removed from unlock, doctor, simulation, and bootstrap. Any lingering hex files are converted automatically, and the initramfs loader refuses to proceed unless the key file is exactly 32 bytes.

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
sudo sudo zfs_beskar_key
```

---

## Configuration

1. **Prepare USB manually** (skip if the bootstrap script already handled it):
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
   `init` records the dataset list, USB path, SHA-256 fingerprint, and binary location, backing up any existing config. It also prints a Base32 recovery key—store it offline so you can rebuild the USB later.
3. **Optional guided mode**:
   ```bash
   sudo /usr/local/bin/zfs_beskar_key --menu
   ```
   The menu surfaces every command with prompts for first-time operators.

---

## Validation

```bash
sudo /usr/local/bin/zfs_beskar_key doctor --dataset=rpool/ROOT
sudo /usr/local/bin/zfs_beskar_key self-test --dataset=rpool/ROOT
```

`doctor` verifies USB presence, key integrity, config permissions, dracut modules, and systemd units. `self-test` simulates the boot unlock sequence end-to-end.

---

## Deployment

```bash
sudo /usr/local/bin/zfs_beskar_key install-units --config=/etc/zfs-beskar.toml
systemctl status beskar-load-key.service
systemctl status beskar-unlock.service
```

Confirm the unlock path without rebooting:

```bash
sudo /usr/local/bin/zfs_beskar_key auto-unlock --dataset=rpool/ROOT --config=/etc/zfs-beskar.toml
```

---

## Operations

- Rotate the key with `init --safe`, confirm prompts, rerun `doctor`, then replace the USB.
- Auto-unlock now cascades across the encryption root and its descendants (e.g., `rpool/ROOT/ubuntu_*`), retrying stubborn children with the same key to ensure the stack unlocks together.
- Use `auto-unlock --strict-usb` on a running system to mirror initramfs behaviour and confirm the USB token alone can restore the pool.
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

- **Current release:** v1.7.2
- **License:** MIT (see `LICENSE`)
- **Author:** Angus J.

This is the Way.
