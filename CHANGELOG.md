# CHANGELOG

All notable changes to **ZFS_BESKAR_KEY** will be documented in this file.  
This project adheres to [Semantic Versioning](https://semver.org/).

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

