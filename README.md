[![Forge Verification](https://github.com/x4ngus/zfs_beskar_key/actions/workflows/rust.yml/badge.svg)](https://github.com/x4ngus/zfs_beskar_key/actions)  

# **ZFS_BESKAR_KEY**

<img width="860" height="430" alt="image" src="https://github.com/user-attachments/assets/309192cc-9f2b-42ac-b36a-918083e472ef" />

## Background

The goal was clear: create a **reliable and safe** method to auto-unlock ZFS encryption using a USB key, equipping you with additional armor for the modern Linux environment.

Through trial and error, Bash failed. Systemd units fought the boot order. 

So this project was reforged in **Rust**, with precision and patience — just as the Armorer would demand.

---

## Purpose

A hardened, key management utility for ZFS encrypted pools. It supports USB-based boot-time decryption, JSON logging, and resilient fallback to passphrase-based unlock.  
All logic is self-contained and can automatically install its own systemd units for unattended boot unlock.

---

## What It Does

**ZFS_BESKAR_KEY** provides:

- Secure generation and management of ZFS dataset encryption keys.  
- USB-based key loading at boot (“Beskar key” token).  
- Optional fallback to your existing ZFS passphrase using `systemd-ask-password`.  
- JSON or quiet modes for integration with provisioning tools.  
- Self-installing systemd units (`beskar-usb.mount` and `beskar-unlock.service`).  
- An Armorer-guided interactive menu (`--menu`) that wraps every command in the full Beskar narrative.

If the USB key is missing or corrupted, **ZFS_BESKAR_KEY** gracefully falls back to your passphrase prompt.  
This ensures **you never end up locked out of your filesystem again.**

---

## 🚀 Quick Start

### 0. Summon the Forge (optional)

Prefer a guided experience? Launch the interactive console and let the Armorer walk you through each task:

```bash
sudo /usr/local/bin/zfs_beskar_key --menu
```

This menu surfaces the same commands as the CLI (`init`, `unlock`, `doctor`, etc.) but wrapped in the full Beskar narrative UX.  
The unlock/lock entries now rehearse the flow on an ephemeral encrypted pool so your live datasets stay untouched while you test the forge.

### 1. Clone & Build

```bash
git clone https://github.com/x4ngus/zfs_beskar_key.git
cd zfs_beskar_key
cargo build --release
sudo cp target/release/zfs_beskar_key /usr/local/bin/
```

### 2. Bootstrap Installation
 Set up everything — configuration file, USB key, and systemd units — in a single command.
 
```bash
curl -fsSL https://raw.githubusercontent.com/x4ngus/zfs_beskar_key/main/scripts/bootstrap.sh | sudo bash
```

This script will:

1. Detect removable disks and confirm the target before formatting.  
2. Weld a single ext4 partition labeled `BESKARKEY`, then forge a dataset-specific key file (for example `rpool_root.keyhex`).  
3. Hash the forged key (SHA-256), inscribe `/etc/zfs-beskar.toml`, and journal a timestamped backup if the file already exists.  
4. Install and enable `beskar-usb.mount` and `beskar-unlock.service` through the hardened installer built into the Rust CLI, then offer to run `dracut -f` immediately so the rescue image learns the new module.
> Tip: You can inspect or edit the scripts/bootstrap.sh script before running — it’s fully commented and self-documenting.

### 3. Manual Setup (Optional)

If you prefer to configure manually instead of running the bootstrap script:

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

> Adjust the filename (`rpool.keyhex`) if your dataset name differs; matching the dataset keeps the forge logs tidy.

After forging the key, run:

```bash
sudo /usr/local/bin/zfs_beskar_key init --dataset=rpool/ROOT
```

The init workflow now:

- Creates a timestamped backup if `/etc/zfs-beskar.toml` already exists.  
- Updates the dataset list, key path, and checksum without disturbing other manual edits.  
- Installs the dracut hooks and systemd units needed for unattended unlock, then offers to rebuild initramfs on the spot.  
- Prompts you to pick the Beskar carrier (auto-highlighting any stick already labeled `BESKARKEY`) so repeat forges stay safe.

---

## Vault Drill Simulation

Before you trust a new Beskar token in battle, rehearse the unlock/lock sequence in a disposable environment:

```bash
sudo /usr/local/bin/zfs_beskar_key --menu
```
Select **“Vault Drill Simulation”** and the forge will:

1. Create an encrypted, file-backed ZFS pool with fresh key material.  
2. Attempt a USB-first unlock using the forged key.  
3. Reseal the vault and present remediation steps if anything fails.  
4. Tear everything down, leaving your real pools untouched, and remind you to rerun `init`, `dracut -f`, and `self-test` on your production key.

> Tip: rotate or reuse USB sticks freely—the selector remembers previously stamped Beskar media and wipes them safely before the next forge.

Use this drill any time you rotate keys or rebuild initramfs to make sure the system is still battle-ready.

---

## Auto-install Systemd Units

Once your configuration and USB key are ready, you can automatically create and enable the required systemd units with a single command:

```bash
sudo /usr/local/bin/zfs_beskar_key install-units --config=/etc/zfs-beskar.toml
```

This command:

- Creates the following files:
-- /etc/systemd/system/beskar-usb.mount
-- /etc/systemd/system/beskar-unlock.service
- Dynamically injects your dataset name, configuration path, and USB UUID.
- Reloads and enables the units automatically via systemctl.

Verify the installation:

```bash
systemctl status beskar-usb.mount
systemctl status beskar-unlock.service
```

If you ever need to uninstall or reconfigure:
```bash
sudo systemctl disable beskar-unlock.service beskar-usb.mount
sudo rm /etc/systemd/system/beskar-{usb.mount,unlock.service}
sudo systemctl daemon-reload
```
>Tip: Re-run install-units any time you change your dataset, USB device, or configuration file. The command is safe and idempotent.

---

## Day-Two Safety Routines

- Re-running `zfs_beskar_key init` now creates a `.bak-<timestamp>` copy of your existing `/etc/zfs-beskar.toml`, updates only the required forge fields, and preserves any custom tuning.  
- The bootstrap script uses the same logic, so rebuilding a USB token never leaves your system without a valid config.  
- Keys forged through the CLI are hashed and recorded automatically, letting the doctor/self-test commands confirm authenticity at any time.
- Run the **Vault Drill Simulation** whenever you rotate media or rebuild initramfs so the Armorer can rehearse the unlock path before the next reboot.

---

## Fallback Logic

During boot or manual unlock, **ZFS_BESKAR_KEY** uses a tiered recovery process to ensure your filesystem remains accessible even if the USB key is lost, damaged, or not detected.

1. **Primary unlock path** — The tool first attempts to read the HEX key from the mounted USB device path (`/run/beskar/rpool.keyhex`).  
2. **Fallback unlock path** — If the USB key is missing or invalid, it triggers a secure passphrase prompt using `systemd-ask-password`.  
3. **Fail-safe mode** — If the passphrase prompt fails or no input is provided, the service exits gracefully. Boot continues without mounting encrypted datasets, allowing you to log in and unlock manually.

This design prevents complete lockout and eliminates the “unbootable” state common in older automated unlock methods.

> **Operational advice:**  
> Keep a secondary passphrase configured in ZFS for your root dataset. This can be the same passphrase used during your OS installation steps. The fallback path only activates if the USB is absent, ensuring the system remains recoverable while still supporting unattended unlock when the Beskar key is present.

---

## Example Boot Flow

Below is the typical startup sequence once **ZFS_BESKAR_KEY** is installed and configured.

1. **Boot initialization** — BIOS/UEFI hands off control to the Linux kernel.  
2. **USB mount** — The `beskar-usb.mount` systemd unit runs early, mounting your USB token under `/run/beskar`.  
3. **Automatic unlock** — `zfs_beskar_key auto-unlock` executes via `beskar-unlock.service`.  
   - If `/run/beskar/rpool.keyhex` is available, the ZFS pool unlocks silently.  
   - If the USB key is missing, the tool invokes `systemd-ask-password` for a manual passphrase entry.  
4. **Dataset import and mount** — Once unlocked, the pool is imported and all datasets mount normally.  
5. **Fallback confirmation** — If both USB and passphrase fail, the system continues booting without the encrypted datasets, leaving the console accessible for manual recovery.

This process ensures that your system **always boots cleanly**, whether or not the USB key is present, and without compromising security.

> The fallback mechanism only activates if the automatic unlock fails, preserving the integrity of the unattended boot path while preventing a full lockout scenario.

---

## Diagnostic Tools

**ZFS_BESKAR_KEY** includes several commands to help verify your configuration and ensure the unlock process works as intended.

### Test the unlock process manually

```bash
sudo /usr/local/bin/zfs_beskar_key auto-unlock --dataset=rpool/ROOT --config=/etc/zfs-beskar.toml
```

This simulates the automatic unlock that runs at boot, allowing you to confirm your USB key and configuration function correctly.

### Check dataset and key status

```bash
sudo /usr/local/bin/zfs_beskar_key doctor --dataset=rpool/ROOT
```
The doctor command validates:
- USB presence and mount status
- Key file readability
- ZFS binary path and permissions
- Encryption key load state
- Dracut module installation and initramfs freshness
- Systemd units (`beskar-usb.mount` / `beskar-unlock.service`) and their enablement status
- Rewrites missing config checksums and can regenerate modules/units automatically

If anything is amiss, the doctor applies safe repairs (reinstalls units, refreshes dracut modules, updates checksums) and summarizes the remaining actions needed to keep the forge battle-ready.

### Re-install units after configuration changes

If you modify the dataset, USB device, or configuration file path, rerun:
```bash
sudo /usr/local/bin/zfs_beskar_key install-units --config=/etc/zfs-beskar.toml
```
This safely re-generates and re-enables the systemd units without requiring manual edits.

>These tools can be used both on a running system and within a rescue environment, making recovery or diagnostics consistent across all scenarios.

---

## Mandalorian Oath

**ZFS_BESKAR_KEY** protects your encrypted ZFS root pool with a balance of automation and resilience.  
If the USB Beskar key is present, the system unlocks silently and securely.  
If not, fallback paths ensure you still reach your data without risking corruption or lockout.

>**This is the Way**

---

## License

MIT License © 2025 Angus J.
You are free to use, modify, and distribute this software under the terms of the MIT license.  
See the `LICENSE` file for full details.
