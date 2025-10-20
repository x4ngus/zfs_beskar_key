use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::process::Command;

pub fn install_hook(label: &str, key_name: &str) -> Result<()> {
    let module_dir = "/usr/lib/dracut/modules.d/95zfs-usbkey";
    let script_path = format!("{module_dir}/zfs-usbkey.sh");
    let setup_path = format!("{module_dir}/module-setup.sh");

    fs::create_dir_all(module_dir).context("Failed to create dracut module dir")?;

    // --- module-setup.sh: registers the hook
    let setup = r#"#!/bin/bash
check() {
    [[ $hostonly ]] || return 0
    require_binaries zfs zpool blkid mount || return 1
}
depends() { echo zfs; }
install() {
    inst_multiple zfs zpool blkid mount
    inst_hook pre-mount 10 "$moddir/zfs-usbkey.sh"
}"#
    .to_string();

    // --- zfs-usbkey.sh: mounts the USB and loads the key
    let script = format!(
        r##"#!/bin/bash
info "=== ZFS USB Key Unlock ==="
LABEL="{label}"
KEYFILE="{key_name}"
MNT=/run/usbkey

# settle devices
udevadm trigger; udevadm settle

DEV=$(blkid -L "$LABEL" 2>/dev/null)
if [ -n "$DEV" ]; then
    mkdir -p "$MNT"
    mount -t ext4 -o ro "$DEV" "$MNT" 2>/dev/null
    if [ -f "$MNT/$KEYFILE" ]; then
        info "Found USB key, attempting ZFS key load..."
        zfs load-key -L "file://$MNT/$KEYFILE" -a && {{
            info "ZFS pool unlocked via USB key."
            umount "$MNT" 2>/dev/null
            exit 0
        }}
    else
        warn "Keyfile not found on USB. Path: $MNT/$KEYFILE"
    fi
else
    warn "No USB device labeled '$LABEL' found."
fi

warn "Falling back to interactive passphrase prompt..."
zfs load-key -a
"##
    );

    File::create(&setup_path)?.write_all(setup.as_bytes())?;
    File::create(&script_path)?.write_all(script.as_bytes())?;

    Command::new("chmod")
        .args(["+x", &setup_path, &script_path])
        .status()
        .context("Failed to make dracut scripts executable")?;

    Ok(())
}

pub fn rebuild_and_verify() -> Result<()> {
    // Include zpool.cache and force full rebuild
    let st = Command::new("dracut")
        .args([
            "--force",
            "--add",
            "zfs",
            "--add-drivers",
            "nvme sd_mod",
            "--include",
            "/etc/zfs/zpool.cache",
            "/etc/zfs/zpool.cache",
        ])
        .status()
        .context("Failed to run dracut")?;

    if !st.success() {
        return Err(anyhow!("dracut rebuild failed"));
    }

    // verify the image exists
    let out = Command::new("bash")
        .args(["-lc", "ls /boot/initrd.img-* | tail -n 1 || echo 'none'"])
        .output()
        .context("Failed to verify initramfs presence")?;
    let result = String::from_utf8_lossy(&out.stdout);
    if result.contains("none") {
        return Err(anyhow!("No /boot/initrd.img-* found after dracut rebuild"));
    }

    Ok(())
}
