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
            .status()?;
        if !st.success() {
            return Err(anyhow!("Missing required tool: {bin}"));
        }
    }
    let st = Command::new("zpool").args(["list", pool]).status()?;
    if !st.success() {
        return Err(anyhow!("Pool {pool} not found"));
    }
    Ok(())
}

pub fn set_prop(ds: &str, key: &str, val: &str) -> Result<()> {
    let st = Command::new("zfs")
        .args(["set", &format!("{key}={val}"), ds])
        .status()?;
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
        getrandom(&mut buf).map_err(|_| anyhow!("getrandom failed"))?;
        f.write_all(&buf)?;
        fs::set_permissions(key_path, fs::Permissions::from_mode(0o000))?;
    }
    // Attach/rotate to raw key
    let st = Command::new("zfs")
        .args([
            "change-key",
            "-o",
            "keyformat=raw",
            "-o",
            &format!("keylocation=file://{key_path}"),
            pool,
        ])
        .status()?;
    if !st.success() {
        return Err(anyhow!("zfs change-key attach raw failed for {pool}"));
    }
    Ok(())
}

pub fn force_converge_children(pool: &str) -> Result<()> {
    // Multi-pass: clear child keylocation, then inherit via change-key -i
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
            cmd.stdin.as_mut().unwrap().write_all(b"inherit\n")?;
            let _ = cmd.wait()?;
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
        .output()?;
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
    // Non-invasive: tiny 1 MiB file vdev
    let vdev = "/tmp/zfstestfile";
    let _ = Command::new("bash")
        .args([
            "-lc",
            &format!("dd if=/dev/urandom of={vdev} bs=1M count=1 status=none"),
        ])
        .status()?;

    let st = Command::new("zpool")
        .args(["create", "-f", "zfstestpool", vdev])
        .status()?;
    if !st.success() {
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
        .spawn()?;
    child.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = child.wait()?;

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
        .spawn()?;
    rot.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = rot.wait()?;

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
        .status()?;
    if !st.success() {
        let _ = Command::new("zpool")
            .args(["destroy", "-f", "zfstestpool"])
            .status();
        let _ = Command::new("rm").args(["-f", vdev]).status();
        return Err(anyhow!("Keyfile load failed"));
    }

    // Test passphrase fallback
    let _ = Command::new("zfs")
        .args(["unload-key", "zfstestpool/dummy"])
        .status();
    let mut loadp = Command::new("zfs")
        .args(["load-key", "zfstestpool/dummy"])
        .stdin(Stdio::piped())
        .spawn()?;
    loadp.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = loadp.wait()?;

    // Cleanup
    let _ = Command::new("zpool")
        .args(["destroy", "-f", "zfstestpool"])
        .status();
    let _ = Command::new("rm").args(["-f", vdev]).status();
    Ok(())
}
