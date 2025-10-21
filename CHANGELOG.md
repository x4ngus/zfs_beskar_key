# ZFS_BESKAR_KEY Changelog

---

## v1.5.0 — Lean Beskar
**Date:** 2025-10-22  
**Codename:** *Tribute Sync*

### Highlights

- **Lean dependency set**  
  - Removed unused crates (`serde_json`, `colored`, `tracing`, `qrcodegen`, etc.) for a smaller build and reduced supply-chain exposure.
- **Focused UX pacing**  
  - Simplified the timing layer and UI helpers so interactive flows stay responsive without dormant debug hooks.
- **First-time operator guide**  
  - Rebuilt the README around the Tribute ▸ Temper ▸ Drill ▸ Diagnose ▸ Deploy cadence, leading new users through installation, rehearsal, and deployment.

### Fixes & Maintenance

- Tightened test utilities to align with the streamlined ZFS trait.
- Trimmed dead helpers in atomic write utilities and ZFS wrappers.
- Renamed the USB mount unit to `run-beskar.mount`, eliminating systemd `bad unit file setting` errors during boot.
- Doctor now re-verifies Beskar units with `systemd-analyze` and auto-reinstalls them when verification fails, guarding unattended boots.

---

## v1.4.0 — Beskar Keepsakes
**Date:** 2025-10-21  
**Codename:** *Fallback Mandate*

### Highlights

- **Forge flow defaults to full reforging**  
  - `init` now wipes and relabels tokens automatically while still offering a safe-mode path with operator confirmations.  
  - Every successful forge remounts the USB at `/run/beskar`, so `doctor`, `self-test`, and boot-time unlocks see the new key immediately.
- **Hardened fallback armour**  
  - If the USB key is missing or corrupt, unlock automatically pivots to `systemd-ask-password` (or an interactive passphrase) without leaving the system unbootable.  
  - The command runner now streams raw key bytes, resolving `zfs load-key` length errors and keeping the vault drill realistic.
- **Bootstrap parity with the Rust forge**  
  - `scripts/bootstrap.sh` mirrors the new workflow: it mounts the freshly forged token under `/run/beskar` and installs the updated units before offering an initramfs rebuild.

### Fixes & Maintenance

- Allowed `systemd-ask-password` and binary stdin in the allowlisted command executor.  
- Routed the `auto-unlock` CLI path through the same resilient unlock routine used by systemd units.  
- Smoothed unmount/back-off logic to eliminate lingering `target is busy` errors during key forging and rehearsal drills.

---

## v1.3.0 — Armorer's Choice
**Date:** 2025-10-20  
**Codename:** *Clan Selector*

### Highlights

- **Guided USB selection**  
  - The init flow now enumerates every removable disk and partition, defaults to existing `BESKARKEY` media, and still allows manual entry.
  - Automatic unmount/force-unmount handling ensures reused Beskar tokens are wiped safely before reforging.

- **Vault drill resilience**  
  - Simulation preflight now shares the improved device logic, keeping rehearsals stable when tokens are recycled.

- **Documentation pass**  
  - README reflects the updated selection workflow and post-forge drill routine.

### Fixes & Maintenance

- Hardened unmount routines call `umount -l/-f` fallbacks, removing “target busy” failures during init.
- Device parsing no longer relies on third-party crates; bespoke parser handles `lsblk -P` output.

---

## v1.2.0 — Tempered Without Force
**Date:** 2025-10-20  
**Codename:** *Covert Steward*

### Highlights

- **Resilient `init` flow**  
  - Automatically detects an existing `/etc/zfs-beskar.toml`, journals a timestamped backup, and refreshes only the Mandalorian-critical fields (dataset, key path, checksum, askpass).  
  - Eliminates the need for `--force` in day-two operations while guaranteeing the config always reflects the freshly forged USB key.

- **Bootstrap script reforged**  
  - Simplified prompts for device selection and dataset naming, with defensive checks that prevent mounted disks from being wiped.  
  - Generates dataset-scoped key filenames, verifies the forged hex, calculates SHA-256 fingerprints, and writes the config using the new resilient logic before installing systemd units.

- **Narrative fully restored**  
  - `init.rs`, `ui.rs`, and `menu.rs` now echo the creed: forge phases, clan briefings, and Beskar metaphors guide every step so the CLI matches the project’s thematic mission.

- **Vault drill simulation**  
  - The interactive menu now offers a single “Vault Drill” that spins up an encrypted, ephemeral ZFS pool to rehearse both unlock and reseal without touching live datasets.  
  - On failure it delivers a detailed remediation checklist so operators can fix `zfs load-key` conditions safely.

- **Holistic doctor workflow**  
  - `zfs_beskar_key doctor` now preflights binaries, validates config + keys, regenerates dracut modules, repairs systemd units, and can trigger `dracut -f` for a fully automated recovery drill.

- **Automated initramfs rebuilds**  
  - `zfs_beskar_key init` and the bootstrapper now offer an immediate `dracut -f` run, ensuring the Beskar module is baked into recovery images after every forge.

- **Holoforge UI refresh**  
  - Reworked banners, dividers, and glyphs to deliver a mission-focused, Mandalorian-inspired command experience.

- **Documentation refresh**  
  - README now calls out the interactive menu, recounts the forge workflow, and explains how the tool safeguards existing configs.

### Fixes & Maintenance

- Tightened config normalization to ensure `policy.datasets[0]` always matches the freshly forged target and that `zfs_path` defaults correctly.
- Added defensive backups with strict 0600 permissions for every config rewrite.
- Ensured bootstrap-created configs align with the new checksum workflow and recovered narrative tone.

---

## v1.1.0 — For the Modern-Day Bounty Hunter
**Date:** 2025-10-20  
**Codename:** *Beskar Forged Edition*

### New Features

**Menu-Driven CLI Experience**
- Added an interactive terminal interface (`--menu` flag or default with no subcommand).
- Introduced colourized “space-terminal” banner, gradient lines, and flicker animation.
- Provides intuitive navigation between key commands:
  - `ForgeKey`, `Unlock`, `Lock`, `Doctor`, and `Init`.
- Menu choices map directly to the same CLI command logic for consistency.

**Adaptive UX Layer (`ui.rs`)**
- Introduced unified `UX` and `Timing` structs for messaging, pacing, and styling.
- Added adaptive pacing via the `Pace` enum (`Info`, `Critical`, `Error`, `Prompt`, `Verbose`).
- Supports `--quiet` and future `--verbose` modes.
- Implemented “terminal flicker” and gradient visuals that automatically disable in quiet mode.
- Added future debug hooks (`trace()`, `set_verbose()`).

**Config System Refactor**
- Replaced `Config` with `ConfigFile` for stronger typing and clearer semantics.
- All submodules now reference `ConfigFile` consistently (`unlock.rs`, `init.rs`, etc.).
- Automatically generates `/etc/zfs-beskar.toml` if missing.

**Atomic Initialization Workflow**
- Added `cmd/init.rs` with full start-to-finish sequence:
  - USB key generation and checksum provisioning.
  - Config creation and permission hardening.
  - ZFS key binding and systemd unit installation for auto-unlock.

**ZFS + Command Dispatch Enhancements**
- `main.rs` now supports both CLI subcommands and interactive menu flow.
- Unified `dispatch_command()` and `dispatch_menu_choice()` functions ensure shared backend logic.
- Improved progress messages and consistent “This is the Way” success responses.

---

### Improvements & Fixes

- Fixed `UI::from_env()` dependency by migrating to new `UX` interface.
- Fixed partial move borrow of `cli` in `main.rs`.
- Removed redundant re-exports and dead-code warnings in `config.rs` and `ui.rs`.
- Suppressed compiler warnings for future verbose/debug features.
- Improved checksum provisioning logic for USB keys.
- Retained full compatibility with `install_units`, `auto_unlock_flow`, and `doctor` diagnostics.

---

### Developer Notes

- Future `--verbose` flag will enable debug and trace logging via `ui.trace()` and adaptive pacing.
- All timing and styling logic now centralized; business logic remains clean and testable.
- Project now ready for **v1.1 release tagging** and **release packaging** with menu-driven UX.

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
