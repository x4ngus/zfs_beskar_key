````markdown
# 🛡️ ZFS Beskar Key

> *“The forge fires hot.”*

---

## 🧩 Background

The goal was clear: create a **reliable and safe** method to auto-unlock ZFS-on-root using a USB key, without ever sacrificing the fallback passphrase or bricking the system.

Bash failed.  
Systemd units fought the boot order. So this was reforged in **Rust**, with precision and patience — just as the Armorer would demand.

---

## ⚙️ Purpose

ZFS Beskar Key automates secure USB-based key unlock for **ZFS-on-root systems** using **Dracut**. It ensures your encrypted pool can unlock automatically from a removable USB key, while still allowing manual passphrase unlock if the key is missing.

---

## 🧱 What It Does

1. **Generates a 32-byte raw ZFS key** and binds it to `rpool` **without** removing your passphrase.
2. **Detects removable USB partitions** via `lsblk -J` + `udevadm`; you select from a **numbered list** (no typing `/dev/*`).
3. **Formats and labels** the selected device as **`BESKARKEY`** (ext4) and copies **`holocron.key`** to its root.
4. **Installs a persistent Dracut module** at `/etc/dracut/modules.d/90zfs-usbkey` that:
   - Waits for USB enumeration (udev trigger + settle, with retries),
   - Mounts the key device by label,
   - Loads keys and imports `rpool` pre-mount.
5. **Rebuilds initramfs** and **verifies** the hook is embedded (using `lsinitrd`).
6. **Runs a non-invasive self-test** using a 1 MiB file-vdev pool to validate both **keyfile** and **passphrase** unlock paths.

---

## 🔧 Dependencies

Ubuntu **25.10** (ZFS-on-root) or equivalent.

System packages:
bash
sudo apt update
sudo apt install -y zfsutils-linux dracut-core lsblk udev mkfs.ext4
````

**Rust toolchain (≥ 1.75):**

bash
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"

---

## 🧪 Trials in the Forge

Lessons learned from the failed attempts that shaped this tool:

* **USB enumeration** can lag in initramfs; a hard 10 s wait is not enough. The hook now **triggers udev** and **waits up to 30 s** with retries.
* **Pre-importing pools** before keys load causes silent failure paths. The hook loads the key, **then** imports `rpool` with `-N`.
* **Child dataset inheritance** can drift (e.g., `rpool/ROOT/ubuntu_*`). The tool **converges encryption roots** back to `rpool`, then verifies none remain independent.
* **Fallback must be sacred.** After attaching the raw key, the tool explicitly resets `keylocation=prompt` so passphrase unlock always works.

This project survived multiple real rebuilds of Ubuntu 25.10 and initramfs recovery drills, until the boot path behaved predictably with and without the USB key.

---

## 🖥️ Build

```bash
git clone https://github.com/x4ngus/zfs_beskar_key.git
cd zfs_beskar_key
cargo build --release
```

---

## 🚀 Run

Run with privileges (formatting, mounting, dracut, and ZFS operations require sudo):

```bash
sudo ./target/release/zfs_beskar_key
```

You’ll see clear, themed steps:

* Forging beskar ingot — generating ZFS key
* Tempering the forge — detecting removable USB partitions
* Binding the clans — formatting USB and copying key
* Etching runes — installing Dracut hook & rebuilding initramfs
* Testing the forge — verifying keyfile & passphrase unlock

A single red **blaster bar** animates progress across the bottom of the terminal.

---

## 🔐 Verify

Pool bindings (post-setup):

```bash
zfs get keyformat,keylocation,keystatus rpool
```

Expected:

```
rpool  keyformat    raw
rpool  keylocation  prompt
rpool  keystatus    available
```

Hook embedded:

```bash
lsinitrd | grep zfs-usb-key.sh
```

USB by label:

```bash
sudo blkid -L BESKARKEY
```

---

## 🧯 Uninstall / Rollback

Remove the Dracut module and rebuild:

```bash
sudo rm -rf /etc/dracut/modules.d/90zfs-usbkey
sudo dracut --force
```

Passphrase unlock remains available by design.

---

## 🛠️ Tested Environment

| Component | Version                | Notes                           |
| --------: | :--------------------- | :------------------------------ |
|    Ubuntu | 25.10                  | ZFS-on-root install path        |
|    Dracut | 059                    | Hook verified in initramfs      |
|       ZFS | 2.2.x                  | change-key + inheritance tested |
|      Rust | ≥ 1.75                 | Built with stable toolchain     |
|  Hardware | Desktop, NVMe, USB 3.x | Race-condition tolerant boot    |

---

## 🧭 Roadmap

* `--dry-run` (preview without changes)
* `--uninstall` (clean rollback)
* Initramfs backup/restore safeguard
* Systemd post-boot verification unit
* Debian packaging (`.deb`)

---

## 🪪 License

MIT — see [LICENSE](LICENSE).

---

## 👤 Author

**Angus Jones**
Technical Account Manager – OT Cybersecurity
Perth, Western Australia
GitHub: [x4ngus](https://github.com/x4ngus)

---

> *“The forge remembers every flame. Each boot, each unlock, each successful load — proof that strength lies not in perfection, but in persistence.”*
> **This is the way.**
