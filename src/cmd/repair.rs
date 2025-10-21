// ============================================================================
// src/cmd/repair.rs – Shared repair / install routines (systemd units, etc.)
// ============================================================================

use crate::cmd::Cmd;
use crate::config::ConfigFile;
use crate::ui::UX;
use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

pub const USB_MOUNT_UNIT: &str = "run-beskar.mount";
pub const USB_UNIT_PATH: &str = "/etc/systemd/system/run-beskar.mount";
pub const UNLOCK_UNIT_PATH: &str = "/etc/systemd/system/beskar-unlock.service";

pub fn install_units(ui: &UX, cfg: &ConfigFile, binary_path: &Path) -> Result<()> {
    if !binary_path.exists() {
        return Err(anyhow!(
            "zfs_beskar_key binary not found at {}",
            binary_path.display()
        ));
    }

    let sysd_path = "/etc/systemd/system";
    let usb_unit = format!("{}/{}", sysd_path, USB_MOUNT_UNIT);
    let unlock_unit = format!("{}/beskar-unlock.service", sysd_path);
    let usb_uuid = get_usb_uuid()?;
    let binary = binary_path.to_string_lossy().into_owned();

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

    let dataset = cfg
        .policy
        .datasets
        .first()
        .cloned()
        .unwrap_or_else(|| "rpool/ROOT".to_string());

    let unlock_content = format!(
        r#"[Unit]
Description=Unlock ZFS dataset with BESKAR USB key
DefaultDependencies=no
After={mount_unit} zfs-import-cache.service zfs-import.target
Requires={mount_unit}
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
ExecStart={binary} auto-unlock --config=/etc/zfs-beskar.toml --dataset={dataset}

[Install]
WantedBy=zfs-mount.service
"#,
        dataset = dataset,
        binary = binary,
        mount_unit = USB_MOUNT_UNIT
    );

    write_unit(&usb_unit, &mount_content)?;
    write_unit(&unlock_unit, &unlock_content)?;

    ui.info("Reloading systemd daemon and enabling sentry units…");
    systemctl(Duration::from_secs(5))?.run(&["daemon-reload"], None)?;
    systemctl(Duration::from_secs(5))?
        .run(&["enable", USB_MOUNT_UNIT, "beskar-unlock.service"], None)?;
    Ok(())
}

pub fn ensure_units_enabled(ui: &UX) -> Result<()> {
    let enable = systemctl(Duration::from_secs(5))?;
    enable.run(&["enable", USB_MOUNT_UNIT], None)?;
    enable.run(&["enable", "beskar-unlock.service"], None)?;
    ui.info("Systemd sentry units stand ready.");
    Ok(())
}

pub fn units_exist() -> bool {
    Path::new(USB_UNIT_PATH).exists() && Path::new(UNLOCK_UNIT_PATH).exists()
}

pub fn unit_exec_matches(binary_path: &Path) -> Result<bool> {
    let expected = binary_path.to_string_lossy();
    let content = fs::read_to_string(UNLOCK_UNIT_PATH)
        .with_context(|| format!("read {}", UNLOCK_UNIT_PATH))?;
    for line in content.lines() {
        if line.trim_start().starts_with("ExecStart=") {
            return Ok(line.contains(expected.as_ref()));
        }
    }
    Ok(false)
}

fn write_unit(path: &str, content: &str) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut f = File::create(path).with_context(|| format!("create {}", path))?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

fn get_usb_uuid() -> Result<String> {
    for candidate in ["/sbin/blkid", "/usr/sbin/blkid", "/usr/bin/blkid"] {
        if Path::new(candidate).exists() {
            let cmd = Cmd::new_allowlisted(candidate, Duration::from_secs(5))?;
            let output = cmd.run(&[], None)?;
            for line in output.stdout.lines() {
                if line.contains("BESKARKEY") {
                    if let Some(u) = line.split("UUID=\"").nth(1) {
                        return Ok(u.split('"').next().unwrap_or_default().to_string());
                    }
                }
            }
        }
    }
    Err(anyhow!("could not detect BESKARKEY UUID"))
}

fn systemctl(timeout: Duration) -> Result<Cmd> {
    for candidate in ["/bin/systemctl", "/usr/bin/systemctl"] {
        if Path::new(candidate).exists() {
            return Cmd::new_allowlisted(candidate, timeout);
        }
    }
    Err(anyhow!("systemctl not found"))
}
