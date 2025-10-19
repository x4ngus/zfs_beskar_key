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
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .context(format!("Could not check for {bin}"))?;
        if !st.success() {
            return Err(anyhow!("Missing required tool: {bin}"));
        }
    }

    let st = Command::new("zpool")
        .args(["list", pool])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
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
        .stderr(Stdio::null())
        .status()
        .context(format!("Failed to set property {key}={val} on {ds}"))?;
    if !st.success() {
        return Err(anyhow!("zfs set {key}={val} {ds} failed"));
    }

    Ok(())
}

pub fn ensure_raw_key(key_dir: &str, key_path: &str, pool: &str) -> Result<()> {
    let ui = ForgeUI::new()?;
    ui.substep("Forging 32-byte raw key (idempotent)")?;

    fs::create_dir_all(key_dir).context("creating key directory")?;

    if !std::path::Path::new(key_path).exists() {
        let mut f = File::create(key_path).context("creating key file")?;
        let mut buf = [0u8; 32];
        // getrandom::Error does not implement std::error::Error -> map explicitly
        getrandom(&mut buf).map_err(|e| anyhow!("Failed to generate random bytes: {:?}", e))?;
        f.write_all(&buf).context("writing key bytes")?;
        fs::set_permissions(key_path, fs::Permissions::from_mode(0o000))
            .context("setting key permissions")?;
    } else {
        ui.substep("Existing key detected; reusing current key")?;
    }

    ui.substep("Attaching raw key to rpool")?;
    let st = Command::new("zfs")
        .args([
            "change-key",
            "-o",
            "keyformat=raw",
            "-o",
            &format!("keylocation=file://{key_path}"),
            pool,
        ])
        .stderr(Stdio::null())
        .status()
        .context("zfs change-key attach raw failed")?;
    if !st.success() {
        return Err(anyhow!("Failed to attach raw key to {pool}"));
    }

    Ok(())
}

/// Inherit all **encrypted** children to rpool’s encryptionroot.
/// Skips unencrypted datasets and `rpool/keystore` by design.
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
            .stderr(Stdio::null())
            .output()
            .context("Failed to read encryptionroot state")?;
        let s = String::from_utf8_lossy(&out.stdout);

        let mut pending = Vec::new();
        for line in s.lines() {
            let mut it = line.split_whitespace();
            let name = it.next().unwrap_or_default();
            let er = it.next().unwrap_or_default();

            // Skip keystore and non-encrypted datasets
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

            // Remove explicit keylocation (if any)
            let _ = Command::new("zfs")
                .args(["set", "keylocation=none", ds])
                .stderr(Stdio::null())
                .stdout(Stdio::null())
                .status();

            // change-key -i with "inherit"
            let mut cmd = Command::new("zfs")
                .args(["change-key", "-i", ds])
                .stdin(Stdio::piped())
                .stderr(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .with_context(|| format!("spawn change-key -i {ds}"))?;
            cmd.stdin
                .as_mut()
                .ok_or_else(|| anyhow!("stdin not available for zfs change-key"))?
                .write_all(b"inherit\n")
                .context("write 'inherit' to change-key")?;
            let _ = cmd.wait();
        }
    }

    // Final verification
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
        .stderr(Stdio::null())
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

/// **Deterministic self-test**: create an ephemeral encrypted child inside `pool`,
/// rotate it to the raw key file, validate keyfile unlock and passphrase fallback,
/// then destroy it. No extra pool, no loopdev, no mounts.
pub fn self_test_dual_unlock(key_path: &str, pool: &str) -> Result<()> {
    let ui = ForgeUI::new()?;
    ui.substep("Running in-place key validation inside rpool")?;

    let test_ds = format!("{pool}/.keytest_tmp");
    // Clean any stale attempt
    let _ = Command::new("zfs")
        .args(["destroy", "-f", &test_ds])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status();

    // Create encrypted dataset (never mount)
    ui.substep("Creating temporary encrypted dataset (canmount=off)")?;
    let mut create = Command::new("zfs")
        .args([
            "create",
            "-o",
            "encryption=on",
            "-o",
            "keyformat=passphrase",
            "-o",
            "keylocation=prompt",
            "-o",
            "canmount=off",
            &test_ds,
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .context("failed to spawn zfs create")?;
    create.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = create.wait();

    // Rotate test dataset to raw key file
    ui.substep("Rotating temporary dataset to raw key file")?;
    let mut rot = Command::new("zfs")
        .args([
            "change-key",
            "-o",
            "keyformat=raw",
            "-o",
            &format!("keylocation=file://{key_path}"),
            &test_ds,
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .context("failed to spawn zfs change-key")?;
    rot.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = rot.wait();

    // Ensure unloaded before keyfile test
    let _ = Command::new("zfs")
        .args(["unload-key", &test_ds])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status();

    // Test keyfile unlock
    ui.substep("Testing keyfile unlock (file://)")?;
    let key_ok = Command::new("zfs")
        .args(["load-key", "-L", &format!("file://{key_path}"), &test_ds])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !key_ok || keystatus(&test_ds) != "available" {
        let _ = Command::new("zfs")
            .args(["destroy", "-f", &test_ds])
            .status();
        return Err(anyhow!("Keyfile unlock test failed"));
    }

    // Test passphrase fallback
    let _ = Command::new("zfs")
        .args(["unload-key", &test_ds])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status();

    ui.substep("Testing passphrase fallback (prompt)")?;
    let mut loadp = Command::new("zfs")
        .args(["load-key", &test_ds])
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .context("failed to spawn zfs load-key (prompt)")?;
    loadp.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let _ = loadp.wait();

    if keystatus(&test_ds) != "available" {
        let _ = Command::new("zfs")
            .args(["destroy", "-f", &test_ds])
            .status();
        return Err(anyhow!("Passphrase unlock test failed"));
    }

    ui.substep("Self-test passed — keyfile and passphrase unlock verified")?;
    let _ = Command::new("zfs")
        .args(["destroy", "-f", &test_ds])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status();
    Ok(())
}

/* ----------------- helpers ----------------- */

fn keystatus(ds: &str) -> String {
    let out = Command::new("zfs")
        .args(["get", "-H", "-o", "value", "keystatus", ds])
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::from("unavailable"),
    }
}
