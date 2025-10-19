use anyhow::{anyhow, bail, Context, Result};
use std::process::{Command, Stdio};
use std::io::Write;
use tempfile::NamedTempFile;

pub fn require_tools(cmds: &[&str]) -> Result<()> {
    for c in cmds {
        which::which(c).with_context(|| format!("Missing tool: {}", c))?;
    }
    Ok(())
}

pub fn assert_pool_exists(pool: &str) -> Result<()> {
    let st = Command::new("zpool").args(["list", pool]).status()?;
    if !st.success() { bail!("Pool {} not found", pool); }
    Ok(())
}

pub fn set_prop(ds: &str, key: &str, val: &str) -> Result<()> {
    let st = Command::new("zfs").args(["set", &format!("{}={}", key, val), ds]).status()?;
    if !st.success() { bail!("zfs set {}={} {} failed", key, val, ds); }
    Ok(())
}

pub fn get_encryptionroot_table(pool: &str) -> Result<Vec<(String,String)>> {
    let out = Command::new("zfs")
        .args(["get","-H","-r","-o","name,value","encryptionroot",pool])
        .output()?;
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.lines().filter_map(|l|{
        let mut it = l.split_whitespace();
        let name = it.next()?.to_string();
        let val  = it.next().unwrap_or("").to_string();
        Some((name,val))
    }).collect())
}

pub fn create_key_raw(key_dir: &str, key_path: &str, pool: &str) -> Result<()> {
    std::fs::create_dir_all(key_dir)?;
    // 32 bytes random
    let mut f = std::fs::File::create(key_path)?;
    let mut data = [0u8;32];
    getrandom::getrandom(&mut data).map_err(|_| anyhow!("getrandom failed"))?;
    use std::os::unix::fs::PermissionsExt;
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(&data)?;
    drop(f);
    Command::new("chmod").args(["000", key_path]).status().ok();

    // Attach to rpool (keyformat=raw; keylocation=file://)
    let st = Command::new("zfs").args(["change-key","-o","keyformat=raw","-o",&format!("keylocation=file://{}", key_path), pool]).status()?;
    if !st.success() { bail!("zfs change-key (attach key) failed"); }

    Ok(())
}

pub fn force_converge_children(pool: &str) -> Result<()> {
    for _pass in 0..5 {
        let table = get_encryptionroot_table(pool)?;
        let mut pending: Vec<String> = table.iter()
            .filter(|(ds,er)| !ds.starts_with(&format!("{}/keystore", pool)) && !er.is_empty() && er != pool)
            .map(|(ds,_)| ds.clone())
            .collect();
        if pending.is_empty() { return Ok(()); }

        for ds in pending.drain(..) {
            // clear child keylocation, then inherit via change-key -i
            let _ = set_prop(&ds, "keylocation", "none");
            // feed "inherit" into stdin
            let mut cmd = Command::new("zfs")
                .args(["change-key","-i",&ds])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .with_context(|| format!("spawn change-key -i {}", ds))?;
            cmd.stdin.as_mut().unwrap().write_all(b"inherit\n")?;
            let st = cmd.wait()?;
            if !st.success() {
                eprintln!("  ! Could not change-key now: {}", ds);
            }
        }
    }
    // re-check
    let table = get_encryptionroot_table(pool)?;
    let remain: Vec<String> = table.into_iter()
        .filter(|(ds,er)| !ds.starts_with(&format!("{}/keystore", pool)) && !er.is_empty() && er != pool)
        .map(|(ds,_er)| ds)
        .collect();
    if !remain.is_empty() { bail!("Datasets still independent: {:?}", remain); }
    Ok(())
}

pub fn self_test_dual_unlock(key_path: &str) -> Result<()> {
    // Non-invasive tiny pool using a 1 MiB file vdev
    let mut tmp = NamedTempFile::new().context("create temp vdev")?;
    tmp.as_file_mut().set_len(1<<20)?;
    let vdev = tmp.path().to_string_lossy().to_string();

    // zpool create
    let st = Command::new("zpool").args(["create","-f","zfstestpool",&vdev]).status()?;
    if !st.success() { bail!("zpool create test failed"); }

    // encrypted dataset with passphrase
    let mut cmd = Command::new("zfs")
        .args(["create","-o","encryption=on","-o","keyformat=passphrase","-o","keylocation=prompt","zfstestpool/dummy"])
        .stdin(Stdio::piped()).spawn()?;
    cmd.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let st = cmd.wait()?; if !st.success() { bail!("zfs create encrypted dummy failed"); }

    // change to raw key using our key file
    let mut cmd = Command::new("zfs")
        .args(["change-key","-o","keyformat=raw","-o",&format!("keylocation=file://{}",key_path),"zfstestpool/dummy"])
        .stdin(Stdio::piped()).spawn()?;
    cmd.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let st = cmd.wait()?; if !st.success() { bail!("zfs change-key to raw failed"); }

    // unload & test keyfile load
    Command::new("zfs").args(["unload-key","zfstestpool/dummy"]).status().ok();
    let st = Command::new("zfs").args(["load-key","-L",&format!("file://{}",key_path),"zfstestpool/dummy"]).status()?;
    if !st.success() { bail!("Keyfile load failed in self-test"); }

    // unload & test passphrase load
    Command::new("zfs").args(["unload-key","zfstestpool/dummy"]).status().ok();
    let mut cmd = Command::new("zfs")
        .args(["load-key","zfstestpool/dummy"])
        .stdin(Stdio::piped()).spawn()?;
    cmd.stdin.as_mut().unwrap().write_all(b"testpass\n")?;
    let st = cmd.wait()?; if !st.success() { bail!("Passphrase load failed in self-test"); }

    // destroy pool
    Command::new("zpool").args(["destroy","-f","zfstestpool"]).status().ok();
    Ok(())
}
