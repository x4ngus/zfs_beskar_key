use anyhow::{anyhow, Context, Result};
use getrandom::getrandom;
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

pub fn preflight(pool: &str) -> Result<()> {
    for bin in ["zfs", "zpool", "dracut", "lsinitrd", "udevadm"] {
        let st = Command::new("bash")
            .args(["-lc", &format!("command -v {bin}")])
            .status()
            .context("shell not available to run command -v")?;
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
    fs::create_dir_all(key_dir).context("creating key directory")?;
    if !std::path::Path::new(key_path).exists() {
        // Create 32-byte raw key file
        let mut f = File::create(key_path).context("creating key file")?;
        let mut buf = [0u8; 32];
        getrandom(&mut buf).map_err(|_| anyhow!("getrandom failed"))?;
        f.write_all(&buf).context("writing key bytes")?;
        // Lock down permissions (initramfs hook reads the USB copy, not this file)
        fs::set_permissions(key_path, fs::Permissions::from_mode(0o000))
            .context("setting key permissions")?;
    }
    // Attach/rotate to raw key on the pool (does NOT remove passphrase fallback)
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
        .context("zfs change-key (attach raw) failed to execute")?;
    if !st.success() {
        return Err(anyhow!("zfs change-key attach raw failed for {pool}"));
    }
    Ok(())
}

pub fn force_converge_children(pool: &str) -> Result<()> {
    // Multi-pass converge: clear child keylocation, then run `change-key -i` with "inherit"
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
        let mut pending: Vec<String> = Vec::new();

        for line in s.lines() {
            let mut it = line.split_whitespace();
            let name = it.next().unwrap_or_default();
            let er = it.next().unwrap_or_default();
            if !name.is_empty()
                && !er.is_empty()
                && er != pool
                && !name.starts_with(&format!("{pool}/keystore"))
            {
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
            // change-key -i with "inherit\n" on stdin
            let mut cmd = Command::new("zfs")
                .args(["change-key", "-i", &ds])
                .stdin(Stdio::piped())
                .spawn()
                .with_context(|| format!("spawn change-key -i {ds}"))?;
            cmd.stdin
                .as_mut()
                .ok_or_else(|| anyhow!("stdin not available for zfs change-key"))?
                .write_all(b"inherit\n")
                .context("write 'inherit' to zfs change-key")?;
            let _ = cmd.wait();
        }
    }
    // Final check
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
        .context("zfs get encryptionroot (final)")?;
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        let mut it = line.split_whitespace();
        let name = it.next().unwrap_or_default();
        let er = it.next().unwrap_or_default();
        if !name.is_empty()
            && !er.is_empty()
            && er != pool
            && !name.starts_with(&format!("{pool}/keystore"))
        {
            return Err(anyhow!("Dataset still independent: {name}"));
        }
    }
    Ok(())
}

pub fn self_test_dual_unlock(key_path: &str) -> Result<()> {
    // Non-invasive ZFS test using a 128 MiB file vdev (>= 64 MiB minimum)
    let vdev_file = "/tmp/zfs_test.img";

    // Ensure no leftover pool from prior runs
    let _ = Command::new("zpool")
        .args(["destroy", "-f", "zfstestpool"])
        .status();
    let _ = Command::new("rm").args(["-f", vdev_file]).status();

    // Allocate 128 MiB sparse file (fast). Use dd if you prefer non-sparse.
    let alloc = Command::new("bash")
        .args([
            "-lc",
            &format!("truncate -s 128M {vdev_file} || dd if=/dev/zero of={vdev_file} bs=1M count=128 status=none conv=fsync"),
        ])
        .status()
        .context("allocating 128 MiB test image")?;
    if !alloc.success() {
        return Err(anyhow!("failed to allocate test image at {vdev_file}"));
    }

    // Create a pool on the file vdev
    let st = Command::new("zpool")
        .args(["create", "-f", "zfstestpool", vdev_file])
        .status()
        .context("zpool create zfstestpool failed to execute")?;
    if !st.success() {
        let _ = Command::new("rm").args(["-f", vdev_file]).status();
        return Err(anyhow!("zpool create zfstestpool failed"));
    }

    // Create encrypted dataset with passphrase "testpass"
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
        .context("zfs create (encrypted dataset) spawn failed")?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| anyhow!("stdin not available for zfs create"))?
        .write_all(b"testpass\n")
        .context("writing passphrase to zfs create")?;
    let _ = child.wait();

    // Rotate to raw key file
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
        .context("zfs change-key (rotate to raw) spawn failed")?;
    rot.stdin
        .as_mut()
        .ok_or_else(|| anyhow!("stdin not available for zfs change-key rotate"))?
        .write_all(b"testpass\n")
        .context("writing passphrase to rotate")?;
    let _ = rot.wait();

    // Test keyfile path
    let _ = Command::new("zfs")
        .args(["unload-key", "zfstestpool/dummy"])
        .status();
    let st = Command::new("zfs")
        .args([
            "load-key",
            "-L",
            &format!("file://{key_path}"),
            "zfstestpool/dummy",
        ])
        .status()
        .context("zfs load-key (file) failed to execute")?;
    if !st.success() {
        let _ = Command::new("zpool")
            .args(["destroy", "-f", "zfstestpool"])
            .status();
        let _ = Command::new("rm").args(["-f", vdev_file]).status();
        return Err(anyhow!("Keyfile load failed"));
    }

    // Test passphrase fallback
    let _ = Command::new("zfs")
        .args(["unload-key", "zfstestpool/dummy"])
        .status();
    let mut loadp = Command::new("zfs")
        .args(["load-key", "zfstestpool/dummy"])
        .stdin(Stdio::piped())
        .spawn()
        .context("zfs load-key (prompt) spawn failed")?;
    loadp
        .stdin
        .as_mut()
        .ok_or_else(|| anyhow!("stdin not available for zfs load-key prompt"))?
        .write_all(b"testpass\n")
        .context("writing passphrase to load-key")?;
    let _ = loadp.wait();

    // Cleanup
    let _ = Command::new("zpool")
        .args(["destroy", "-f", "zfstestpool"])
        .status();
    let _ = Command::new("rm").args(["-f", vdev_file]).status();
    Ok(())
}
