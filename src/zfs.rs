use anyhow::{anyhow, Context, Result};
use getrandom::getrandom;
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use crate::ui::ForgeUI;

pub fn preflight(pool: &str) -> Result<()> {
    let ui = ForgeUI::new()?;
    ui.substep("Checking required system tools")?;

    for bin in ["zfs", "zpool", "dracut", "lsinitrd", "udevadm"] {
        let st = Command::new("bash")
            .args(["-lc", &format!("command -v {bin}")])
            .status()
            .context(format!("Could not check for {bin}"))?;
        if !st.success() {
            return Err(anyhow!("Missing required tool: {bin}"));
        }
    }

    let st = Command::new("zpool")
        .args(["list", pool])
        .status()
        .context("zpool list failed")?;
    if !st.success() {
        return Err(anyhow!("Pool {pool} not found"));
    }

    ui.substep("All tools and pool verified")?;
    Ok(())
}

pub fn set_prop(ds: &str, key: &str, val: &str) -> Result<()> {
    let ui = ForgeUI::new()?;
    ui.substep(&format!("Setting {key}={val} for {ds}"))?;

    let st = Command::new("zfs")
        .args(["set", &format!("{key}={val}"), ds])
        .status()
        .context(format!("Failed to set property {key}={val} on {ds}"))?;
    if !st.success() {
        return Err(anyhow!("zfs set {key}={val} {ds} failed"));
    }

    Ok(())
}

pub fn ensure_raw_key(key_dir: &str, key_path: &str, pool: &str) -> Result<()> {
    let ui = ForgeUI::new()?;
    ui.substep("Forging new 32-byte raw key")?;

    fs::create_dir_all(key_dir)?;

    if !std::path::Path::new(key_path).exists() {
        let mut f = File::create(key_path)?;
        let mut buf = [0u8; 32];

        // safer getrandom handling
        getrandom(&mut buf)
            .map_err(|e| anyhow!("Failed to generate random bytes for key: {e:?}"))?;

        f.write_all(&buf)?;
        fs::set_permissions(key_path, fs::Permissions::from_mode(0o000))?;
    } else {
        ui.substep("Existing key detected; reusing current key")?;
    }

    ui.substep("Attaching key to ZFS root pool")?;
    let st = Command::new("zfs")
        .args([
            "change-key",
            "-o",
            "keyformat=raw",
            "-o",
            &format!("keylocation=file://{key_path}"),
            pool,
        ])
        .status()
        .context("zfs change-key attach raw failed")?;

    if !st.success() {
        return Err(anyhow!("Failed to attach raw key to {pool}"));
    }

    Ok(())
}

pub fn force_converge_children(pool: &str) -> Result<()> {
    let ui = ForgeUI::new()?;
    ui.substep("Reconciling child dataset encryption roots")?;

    for _ in 0..4 {
        let out = Command::new("zfs")
            .args([
                "get",
                "-H",
                "-r",
                "-o",
                "name,value",
                "encryptionroot",
                pool,
            ])
            .output()
            .context("Failed to read encryptionroot state")?;
        let s = String::from_utf8_lossy(&out.stdout);

        let mut pending = Vec::new();
        for line in s.lines() {
            let mut it = line.split_whitespace();
            let name = it.next().unwrap_or_default();
            let er = it.next().unwrap_or_default();

            if name.is_empty()
                || er == pool
                || name.starts_with(&format!("{pool}/keystore"))
                || er == "-"
            {
                continue;
            }

            pending.push(name.to_string());
        }

        if pending.is_empty() {
            ui.substep("All datasets correctly bound to rpool")?;
            return Ok(());
        }

        for ds in pending.iter() {
            ui.substep(&format!("Binding child dataset: {ds}"))?;

            let _ = Command::new("zfs")
                .args(["set", "keylocation=none", ds])
                .status();

            let mut cmd = Command::new("zfs")
                .args(["change-key", "-i", ds])
                .stdin(Stdio::piped())
                .spawn()
                .with_context(|| format!("spawn change-key -i {ds}"))?;
            cmd.stdin.as_mut().unwrap().write_all(b"inherit\n")?;
            let _ = cmd.wait()?;
        }
    }

    // Final check: report only true misconfigs
    let out = Command::new("zfs")
        .args([
            "get",
            "-H",
            "-r",
            "-o",
            "name,value",
            "encryptionroot",
            pool,
        ])
        .output()
        .context("final encryptionroot check failed")?;
    let s = String::from_utf8_lossy(&out.stdout);

    let mut bad_ds = Vec::new();
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let name = it.next().unwrap_or_default();
        let er = it.next().unwrap_or_default();

        if name.starts_with(&format!("{pool}/keystore")) || er == "-" {
            continue;
        }
        if !name.is_empty() && er != pool {
            bad_ds.push(format!("{name} → root={er}"));
        }
    }

    if !bad_ds.is_empty() {
        let list = bad_ds.join(", ");
        return Err(anyhow!(
            "Datasets still independent or misconfigured: {list}"
        ));
    }

    ui.substep("Encryption roots unified successfully")?;
    Ok(())
}

pub fn self_test_dual_unlock(key_path: &str) -> Result<()> {
    let ui = ForgeUI::new()?;
    ui.substep("Running non-invasive dual unlock self-test")?;

    let vdev = "/tmp/zfstestfile";
    let _ = Command::new("bash")
        .args([
            "-lc",
            &format!("dd if=/dev/zero of={vdev} bs=1M count=8 status=none"),
        ])
        .status();

    let st = Command::new("zpool")
        .args(["create", "-f", "zfstestpool", vdev])
        .status()?;
    if !st.success() {
        return Err(anyhow!(
            "zpool create zfstestpool failed — insufficient space or permissions"
        ));
    }

    let mut child = Command::new("zfs")
        .args([
            "create",
            "-o",
            "encryption=on",
            "-o",
            "keyformat=passphrase",
            "-o",
            "keylocation=prompt",
            "zfstestpool/dummy",
        ])
        .stdin(Stdio::piped())
        .spawn()?;
    child.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = child.wait()?;

    let mut rot = Command::new("zfs")
        .args([
            "change-key",
            "-o",
            "keyformat=raw",
            "-o",
            &format!("keylocation=file://{key_path}"),
            "zfstestpool/dummy",
        ])
        .stdin(Stdio::piped())
        .spawn()?;
    rot.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = rot.wait()?;

    // unload + reload test
    Command::new("zfs")
        .args(["unload-key", "zfstestpool/dummy"])
        .status()
        .ok();

    let keyload = Command::new("zfs")
        .args([
            "load-key",
            "-L",
            &format!("file://{key_path}"),
            "zfstestpool/dummy",
        ])
        .status()?;
    if !keyload.success() {
        cleanup_zfstest(vdev);
        return Err(anyhow!("Keyfile load failed during self-test"));
    }

    // test passphrase fallback
    Command::new("zfs")
        .args(["unload-key", "zfstestpool/dummy"])
        .status()
        .ok();
    let mut loadp = Command::new("zfs")
        .args(["load-key", "zfstestpool/dummy"])
        .stdin(Stdio::piped())
        .spawn()?;
    loadp.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = loadp.wait()?;

    cleanup_zfstest(vdev);
    ui.substep("Self-test passed — both key and passphrase unlock verified")?;
    Ok(())
}

fn cleanup_zfstest(vdev: &str) {
    let _ = Command::new("zpool")
        .args(["destroy", "-f", "zfstestpool"])
        .status();
    let _ = Command::new("rm").args(["-f", vdev]).status();
}
