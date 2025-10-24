[![Forge Verification](https://github.com/x4ngus/zfs_beskar_key/actions/workflows/rust.yml/badge.svg)](https://github.com/x4ngus/zfs_beskar_key/actions)

# **ZFS_BESKAR_KEY**

<img width="860" height="430" alt="image" src="https://github.com/user-attachments/assets/309192cc-9f2b-42ac-b36a-918083e472ef" />

A USB-first ZFS unlock companion forged for dependable, unattended boots. Tribute ▸ Temper ▸ Drill ▸ Diagnose ▸ Deploy.

---

## Overview

`zfs_beskar_key` unlocks encrypted ZFS datasets from a dedicated USB key while keeping a secured passphrase fallback online. Configuration lives in `/etc/zfs-beskar.toml`; commands default to strict permissions and atomic writes.

### Release highlights (v1.6.4)

- Dracut assets now ship from `src/dracut/templates`, guaranteeing consistent module generation.
- `zfs_beskar_key init` automatically runs the same installer path as `install-dracut` and immediately calls `dracut -f`, so new keys are baked into initramfs without extra operator steps.
- Doctor repairs surface actionable guidance (`install-dracut` ▸ `dracut -f`) whenever the module needs a refresh.

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
   /usr/local/bin/zfs_beskar_key forge-key | sudo tee /mnt/beskar/rpool.keyhex >/dev/null
   sudo chmod 0400 /mnt/beskar/rpool.keyhex
   sudo umount /mnt/beskar
   ```
2. **Normalize policy and checksums**:
   ```bash
   sudo /usr/local/bin/zfs_beskar_key init --dataset=rpool/ROOT
   ```
   `init` records the dataset list, USB path, SHA-256 fingerprint, and binary location, backing up any existing config.
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
systemctl status run-beskar.mount
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
- Every forge run auto-installs the dracut module (when present) and forces `dracut -f`, matching the dedicated `install-dracut` command.
- Launch `--menu` ▸ *Vault Drill* after hardware or initramfs changes to rehearse unlocks on a disposable pool.
- Monitor `/var/log/beskar.log` for append-only audit entries.
- Re-run `install-units` whenever datasets, USB devices, or binary paths change; `doctor` will verify unit sanity with `systemd-analyze`.

---

## Recovery

- Missing USB media triggers a secure `systemd-ask-password` prompt at boot; enter the dataset passphrase to proceed.
- After recovery login, run `doctor` to restore checksums, units, or dracut modules.
- Use `auto-unlock --json` for scripted rescue workflows.

---

## Project Details

- **Current release:** v1.6.4
- **License:** MIT (see `LICENSE`)
- **Author:** Angus J.

This is the Way.
