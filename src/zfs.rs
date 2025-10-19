use anyhow::{anyhow, Context, Result};
use getrandom::getrandom;
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::{thread, time::Duration};

pub fn preflight(pool: &str) -> Result<()> {
    for bin in ["zfs", "zpool", "dracut", "lsinitrd", "udevadm"] {
        let st = Command::new("bash")
            .args(["-lc", &format!("command -v {bin}")])
            .status()
            .context("shell not available")?;
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
    Ok(())
}

pub fn set_prop(ds: &str, key: &str, val: &str) -> Result<()> {
    let st = Command::new("zfs")
        .args(["set", &format!("{key}={val}"), ds])
        .status()
        .context("zfs set failed")?;
    if !st.success() {
        return Err(anyhow!("zfs set {key}={val} {ds} failed"));
    }
    Ok(())
}

pub fn ensure_raw_key(key_dir: &str, key_path: &str, pool: &str) -> Result<()> {
    fs::create_dir_all(key_dir)?;

    if !std::path::Path::new(key_path).exists() {
        let mut f = File::create(key_path)?;
        let mut buf = [0u8; 32];

        // FIX: map getrandom::Error manually into anyhow::Error
        getrandom(&mut buf).map_err(|e| anyhow!("Failed to generate random bytes: {:?}", e))?;

        f.write_all(&buf)?;
        fs::set_permissions(key_path, fs::Permissions::from_mode(0o000))?;
    }

    // Attach the raw key to the ZFS pool
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
        .map_err(|e| anyhow!("Failed to execute zfs command: {:?}", e))?;

    if !st.success() {
        return Err(anyhow!("zfs change-key attach raw failed for {pool}"));
    }

    Ok(())
}

pub fn force_converge_children(pool: &str) -> Result<()> {
    for _ in 0..5 {
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
            .context("zfs get encryptionroot")?;
        let s = String::from_utf8_lossy(&out.stdout);
        let mut pending = Vec::new();

        for line in s.lines() {
            let mut it = line.split_whitespace();
            let name = it.next().unwrap_or_default();
            let er = it.next().unwrap_or_default();
            if !name.is_empty() && !er.is_empty() && er != pool {
                pending.push(name.to_string());
            }
        }
        if pending.is_empty() {
            return Ok(());
        }
        for ds in pending {
            let _ = Command::new("zfs")
                .args(["set", "keylocation=none", &ds])
                .status();
            let mut cmd = Command::new("zfs")
                .args(["change-key", "-i", &ds])
                .stdin(Stdio::piped())
                .spawn()
                .with_context(|| format!("spawn change-key -i {ds}"))?;
            cmd.stdin.as_mut().unwrap().write_all(b"inherit\n")?;
            let _ = cmd.wait();
        }
    }

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
        .output()?;
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let name = it.next().unwrap_or_default();
        let er = it.next().unwrap_or_default();
        if !name.is_empty() && !er.is_empty() && er != pool {
            return Err(anyhow!("Dataset still independent: {name}"));
        }
    }
    Ok(())
}

pub fn self_test_dual_unlock(key_path: &str) -> Result<()> {
    let vdev_file = "/tmp/zfs_test.img";
    let _ = Command::new("zpool")
        .args(["destroy", "-f", "zfstestpool"])
        .status();
    let _ = Command::new("rm").args(["-f", vdev_file]).status();

    // Create test vdev
    Command::new("bash")
        .args([
            "-lc",
            &format!("truncate -s 128M {vdev_file} || dd if=/dev/zero of={vdev_file} bs=1M count=128 status=none conv=fsync"),
        ])
        .status()
        .context("allocating test image")?;

    let st = Command::new("zpool")
        .args(["create", "-f", "zfstestpool", vdev_file])
        .status()
        .context("zpool create failed")?;
    if !st.success() {
        return Err(anyhow!("zpool create zfstestpool failed"));
    }

    // Wait for the test pool to register
    thread::sleep(Duration::from_secs(2));
    let _ = Command::new("zpool")
        .args(["import", "zfstestpool"])
        .status();

    // Create encrypted dataset
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
        .spawn()
        .context("zfs create spawn failed")?;
    child.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = child.wait();

    // Rotate to raw key
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
        .spawn()
        .context("rotate to raw failed")?;
    rot.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = rot.wait();

    // Keyfile test
    Command::new("zfs")
        .args(["unload-key", "zfstestpool/dummy"])
        .status()
        .ok();
    thread::sleep(Duration::from_millis(500));

    let st = Command::new("zfs")
        .args([
            "load-key",
            "-L",
            &format!("file://{key_path}"),
            "zfstestpool/dummy",
        ])
        .status()
        .context("load-key (file)")?;
    if !st.success() {
        cleanup_test();
        return Err(anyhow!("Keyfile load failed"));
    }

    // Passphrase fallback
    Command::new("zfs")
        .args(["unload-key", "zfstestpool/dummy"])
        .status()
        .ok();
    thread::sleep(Duration::from_millis(500));

    let mut loadp = Command::new("zfs")
        .args(["load-key", "zfstestpool/dummy"])
        .stdin(Stdio::piped())
        .spawn()
        .context("load-key prompt failed")?;
    loadp.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = loadp.wait();

    cleanup_test();
    Ok(())
}

fn cleanup_test() {
    let _ = Command::new("zpool")
        .args(["destroy", "-f", "zfstestpool"])
        .status();
    let _ = Command::new("rm")
        .args(["-f", "/tmp/zfs_test.img"])
        .status();
}
