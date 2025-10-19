use anyhow::{bail, Context, Result};
use std::fs;
use std::process::Command;

const HOOK_DIR: &str = "/etc/dracut/modules.d/90zfs-usbkey";
const MODULE_SETUP: &str = r#"#!/bin/bash
check(){ return 0; }
depends(){ echo zfs; }
install(){ inst_hook pre-mount 00 "$moddir/zfs-usb-key.sh"; }
"#;

fn zfs_usb_key_sh(label: &str, keyfile: &str) -> String {
    format!(r#"#!/bin/sh
LABEL="{label}"
KEYFILE="{keyfile}"
LOG="/run/initramfs/zfs-usb-key.log"
RETRY=30

echo "[forge] init: starting key loader" >"$LOG"
modprobe usb_storage 2>/dev/null
modprobe ext4 2>/dev/null
modprobe zfs 2>/dev/null

udevadm trigger
udevadm settle --timeout=$RETRY

DEV=""
for i in $(seq 1 $RETRY); do
  DEV=$(blkid -L "$LABEL" 2>/dev/null) || true
  [ -b "$DEV" ] && break
  sleep 1
done

if [ -b "$DEV" ]; then
  mkdir -p /key
  mount -t ext4 "$DEV" /key 2>/dev/null || mount -t auto "$DEV" /key 2>/dev/null
  if [ -f "/key/$KEYFILE" ]; then
    echo "[forge] key found — loading" >>"$LOG"
    zfs load-key -a -L "file:///key/$KEYFILE" 2>>"$LOG" || echo "[forge] key load failed" >>"$LOG"
    zpool import -N rpool 2>>"$LOG" || echo "[forge] rpool import skipped" >>"$LOG"
  else
    echo "[forge] key missing — skipping" >>"$LOG"
  fi
  umount /key 2>/dev/null || true
else
  echo "[forge] usb not detected — fallback to passphrase" >>"$LOG"
fi

exit 0
"#, label=label, keyfile=keyfile)
}

pub fn install_hook(label: &str, key_name: &str) -> Result<()> {
    fs::create_dir_all(HOOK_DIR).context("mkdir hook dir")?;
    fs::write(format!("{}/module-setup.sh", HOOK_DIR), MODULE_SETUP).context("write module-setup.sh")?;
    fs::set_permissions(format!("{}/module-setup.sh", HOOK_DIR), fs::Permissions::from_mode(0o755)).ok();
    fs::write(format!("{}/zfs-usb-key.sh", HOOK_DIR), zfs_usb_key_sh(label, key_name)).context("write zfs-usb-key.sh")?;
    fs::set_permissions(format!("{}/zfs-usb-key.sh", HOOK_DIR), fs::Permissions::from_mode(0o755)).ok();
    Ok(())
}

pub fn rebuild_initramfs() -> Result<()> {
    let st = Command::new("dracut").args(["--force"]).status().context("dracut --force failed")?;
    if !st.success() { bail!("dracut rebuild failed"); }
    Ok(())
}

pub fn verify_initramfs_contains() -> Result<()> {
    // Verify the latest initramfs contains our script name
    let out = Command::new("bash").args(["-lc", "ls -1t /boot/initramfs-* 2>/dev/null | head -n1"]).output()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() { bail!("No /boot/initramfs-* found"); }
    let out = Command::new("lsinitrd").args([&path]).output()?;
    let s = String::from_utf8_lossy(&out.stdout);
    if !s.contains("zfs-usb-key.sh") { bail!("Hook not embedded in initramfs"); }
    Ok(())
}

// unix perm helpers
use std::os::unix::fs::PermissionsExt;
