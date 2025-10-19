use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::Write;
use std::process::Command;

/// Install the Dracut hook that will load the ZFS key from the USB
pub fn install_hook(label: &str, key_name: &str) -> Result<()> {
    let module_dir = "/etc/dracut/modules.d/90zfs-usbkey";
    fs::create_dir_all(module_dir).context("Unable to create Dracut module directory")?;

    // Create module-setup.sh
    let setup_path = format!("{}/module-setup.sh", module_dir);
    let mut setup = fs::File::create(&setup_path)?;
    writeln!(
        setup,
        r#"#!/bin/bash
check() {{
    return 0
}}

depends() {{
    echo zfs
}}

install() {{
    inst_hook pre-mount 01 "$moddir/zfs-usb-key.sh"
}}"#
    )?;
    Command::new("chmod").arg("+x").arg(&setup_path).status()?;

    // Create zfs-usb-key.sh
    let key_script_path = format!("{}/zfs-usb-key.sh", module_dir);
    let mut script = fs::File::create(&key_script_path)?;
    writeln!(
        script,
        r#"#!/bin/sh
# Dracut hook: attempt to load ZFS key from USB labeled {label}
echo "[dracut:zfs-usbkey] 🔑 Waiting for USB key '{label}'..."
/bin/udevadm trigger
/bin/udevadm settle --timeout=30
if blkid -L {label} >/dev/null 2>&1; then
    mkdir -p /mnt
    mount -o ro "$(blkid -L {label})" /mnt && echo "[dracut:zfs-usbkey] USB mounted."
    if [ -f "/mnt/{key_name}" ]; then
        zfs load-key -L "file:///mnt/{key_name}" rpool && echo "[dracut:zfs-usbkey] Key loaded."
    else
        echo "[dracut:zfs-usbkey] Key file not found: /mnt/{key_name}"
    fi
    umount /mnt
else
    echo "[dracut:zfs-usbkey] USB device '{label}' not found — will fall back to passphrase."
fi
"#
    )?;
    Command::new("chmod")
        .arg("+x")
        .arg(&key_script_path)
        .status()?;

    println!("🧩 Dracut hook installed at {module_dir}");
    Ok(())
}

/// Rebuild initramfs to include the new module
pub fn rebuild_initramfs() -> Result<()> {
    println!("🔥 Rebuilding initramfs via Dracut...");
    let status = Command::new("dracut")
        .args(["--force"])
        .status()
        .context("Failed to invoke dracut")?;
    if !status.success() {
        return Err(anyhow!("dracut failed to rebuild initramfs"));
    }
    Ok(())
}

/// Verify that a boot image exists and contains the hook
pub fn verify_initramfs_contains() -> Result<()> {
    // Ubuntu uses initrd.img-*, Fedora uses initramfs-*
    let output = Command::new("bash")
        .arg("-c")
        .arg("ls /boot/initramfs-* /boot/initrd.img-* 2>/dev/null | head -n1")
        .output()?;

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if path.is_empty() {
        return Err(anyhow!(
            "❌ No /boot/initramfs-* or /boot/initrd.img-* found. Check your Dracut build."
        ));
    }

    let verify = Command::new("bash")
        .arg("-c")
        .arg(format!("lsinitrd {} | grep -q zfs-usb-key.sh", path))
        .status()?;

    if verify.success() {
        println!(
            "\n✅ Verified: Dracut hook embedded in {}\nThe forge burns clean — the key will ignite the pool.",
            path
        );
        Ok(())
    } else {
        Err(anyhow!(
            "⚠️  Found {}, but zfs-usb-key.sh not detected inside.",
            path
        ))
    }
}

/// Combined convenience call used by main.rs
pub fn rebuild_and_verify() -> Result<()> {
    rebuild_initramfs().context("Failed to rebuild initramfs with Dracut")?;
    verify_initramfs_contains().context("Initramfs verification failed")?;
    Ok(())
}
