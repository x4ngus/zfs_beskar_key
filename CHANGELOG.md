# ZFS_BESKAR_KEY Changelog

---

## v1.0.0 — Production-Stable Release (The Way)
**Date:** 2025-10-20  
**Codename:** *Beskar Forged Edition*

This marks the first **major stable release** of `zfs_beskar_key`, hardened and battle-tested for real-world deployments.  
The project now supports full USB-first auto-unlock for ZFS datasets with a self-validating cryptographic verification layer — ensuring that even in the chaos of reboots and system updates, your pool remains guarded.

### New Features
- **USB Key Self-Test:**  
  Added robust logic to verify and sanitize the `/run/beskar/key.hex` contents.  
  Automatically validates key length (64 hex chars) and reports structured errors if malformed.

- **SHA-256 Integrity Check:**  
  Introduced cryptographic checksum verification for USB keys.  
  Each USB key’s raw bytes are hashed and validated against `expected_sha256` in the config.  
  If missing, the system now **auto-generates and writes** this value to `/etc/zfs-beskar.toml`.

- **Config Auto-Provisioning:**  
  Automatically creates `/etc/zfs-beskar.toml` with secure permissions and sensible defaults  
  if missing — eliminating user confusion and bootstrap errors.

- **USB-First, Passphrase Fallback:**  
  Improved reliability and sequence for auto-unlock flow — USB key preferred, passphrase fallback available  
  with `systemd-ask-password` integration.

- **Systemd Unit Generator:**  
  Hardened creation of `beskar-usb.mount` and `beskar-unlock.service` units with  
  minimal privileges, readonly mounts, and security directives aligned to systemd best practices.

- **End-to-End SelfTest Command:**  
  New `self-test` command performs a non-destructive simulation of the entire unlock flow,  
  verifying configuration, ZFS command behavior, and USB validity in one go.

### Improvements
- Simplified and sanitized USB key parsing logic (`is_ascii_hexdigit`-filtered read).  
- Removed redundant references to deprecated ForgeUI and CmdOutput structs.  
- Improved stdout consistency and readability for JSON vs TTY modes.  
- Default timeouts now enforce a sane floor of 10 seconds for cryptographic operations.  
- Clearer, lore-consistent CLI messages (“This is the Way.” when operations succeed).  
- Added robust permission handling for config file creation (`chmod 600`).

### Removed / Deprecated
- Old raw `read()`-based USB reader removed — replaced with UTF-8 safe canonical reader.  
- Removed unused imports and redundant checksum paths.  
- Eliminated all private method calls in `ui.rs` and unreferenced helpers.

### Technical Notes
- Built and tested on Ubuntu with `/usr/sbin/zfs` binary.  
- ZFS operations run through `Cmd` abstraction layer for security and auditability.  
- Compatible with systemd environments — ideal for headless boot unlock on encrypted systems.  
- Written in Rust 1.80+ with zero unsafe code.

---

### Upgrade Notes
- If upgrading from an earlier prototype version:
  1. Delete any malformed `/run/beskar/key.hex` files.
  2. Regenerate your USB key using:
     ```bash
     openssl rand -hex 32 | sudo tee /run/beskar/key.hex
     sudo chmod 600 /run/beskar/key.hex
     ```
  3. Run once:
     ```bash
     sudo /usr/local/bin/zfs_beskar_key auto-unlock --config=/etc/zfs-beskar.toml
     ```
     This will generate and record the expected SHA-256 automatically.
  4. Verify integrity with:
     ```bash
     sudo /usr/local/bin/zfs_beskar_key self-test
     ```

---

**“This is the Way.”**  
— v1.0.0: Beskar Forged and Battle-Ready.

---

## [v0.3.0] — 2025-10-20
### Added
- **Bootstrap installer (`scripts/bootstrap.sh`)**  
  - Automates full setup: USB partitioning, key generation, configuration file creation, and systemd unit installation.  
  - Includes subtle Mandalorian-themed forge messages for clarity and style.  
  - Safe, idempotent execution — all destructive steps explicitly confirmed.
- **README.md overhaul**  
  - Added one-liner bootstrap command using GitHub raw URL.  
  - Detailed step-by-step manual setup instructions.  
  - Integrated configuration, fallback logic, and diagnostic sections.  
  - Updated tone and formatting for production documentation quality.
- **Systemd auto-install command (`install-units`)**  
  - Generates and enables `beskar-usb.mount` and `beskar-unlock.service` dynamically.  
  - Uses config-driven injection for dataset, USB UUID, and paths.  
  - Enables unattended boot unlock while preserving passphrase fallback.

### Changed
- **Main CLI architecture**
  - Added `install-units` subcommand to simplify operational deployment.
  - Improved argument handling and dataset parsing.
- **UI module (`ui.rs`)**
  - Cleaned up syntax, added structured logging, ensured error propagation consistency.
- **ZFS integration (`zfs.rs`)**
  - Removed deprecated `ForgeUI` references for compatibility with refactored UI interface.

### Fixed
- Resolved multiple compiler errors (`E0382`, `E0433`, etc.) in `main.rs` and `ui.rs`.  
- Corrected `cli` borrow issue in `dataset_from()` logic.  
- Stabilized ZFS module imports and JSON output handling.

---

## [v0.2.0] — 2025-10-15
### Added
- **Refactored UI framework**
  - Implemented unified progress display and JSON log emitter.
  - Introduced `blaster`, `warn`, and `error` message helpers for CLI output.
- **Improved CLI parsing**
  - Added `--json` and `--dataset` flags for controlled output and dataset targeting.

### Changed
- Restructured `main.rs` for production layout (modules: `ui`, `zfs`, `config`).  
- Replaced experimental prototypes with hardened error handling and result propagation.

---

## [v0.1.0] — 2025-10-10
### Added
- Initial proof-of-concept release for **ZFS_BESKAR_KEY**.  
- Implemented core commands:
  - `forge-key` — generates 32-byte encryption key.  
  - `unlock` — loads HEX key into target ZFS dataset.  
  - `lock` — unloads key securely from memory.  
- Configurable via TOML and designed for future systemd integration.

---

### Project Philosophy

> Each iteration refines the balance between automation and recoverability.  
> The forge remains focused on precision, restraint, and purpose — not flash.
> *“This is the Way”*  

---

### Maintainer
Angus J. — [@x4ngus](https://github.com/x4ngus)

