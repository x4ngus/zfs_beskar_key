use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::io::{self, Write};
use std::process::Command;

#[derive(Debug, Deserialize)]
struct Lsblk {
    blockdevices: Vec<BlockDev>,
}
#[derive(Debug, Deserialize)]
struct BlockDev {
    name: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    children: Option<Vec<BlockDev>>,
}

fn is_usb_disk(dev: &str) -> bool {
    // Check udev for ID_BUS=usb on parent disk
    let out = Command::new("udevadm")
        .args(["info", "-q", "property", "-n", dev])
        .output();
    if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout);
        return s.contains("ID_BUS=usb");
    }
    false
}

pub fn select_usb_partition() -> Result<String> {
    // Parse JSON for reliability
    let out = Command::new("lsblk")
        .args(["-J", "-o", "NAME,TYPE,RM,SIZE,MODEL,MOUNTPOINT"])
        .output()
        .context("lsblk failed")?;
    let parsed: Lsblk = serde_json::from_slice(&out.stdout).context("lsblk JSON parse")?;

    // Collect partitions whose parent disk is USB
    let mut candidates: Vec<(String, String)> = Vec::new(); // (devpath, label)
    for dev in &parsed.blockdevices {
        // Only look at "disk" entries; iterate their children parts
        let disk_path = format!("/dev/{}", dev.name);
        if !is_usb_disk(&disk_path) {
            continue;
        }
        if let Some(children) = &dev.children {
            for part in children {
                let devpath = format!("/dev/{}", part.name);
                let size = part.size.clone().unwrap_or_default();
                let model = dev.model.clone().unwrap_or_default();
                let mnt = part.mountpoint.clone().unwrap_or("-".into());
                let label = format!("{:<14} {:>7}  {:<20}  {}", part.name, size, model, mnt);
                candidates.push((devpath, label));
            }
        }
    }

    if candidates.is_empty() {
        return Err(anyhow!(
            "No removable USB partitions detected. Insert a USB and try again."
        ));
    }

    println!("\nAvailable USB partitions:");
    for (i, (_, label)) in candidates.iter().enumerate() {
        println!(" {:>2}) {}", i + 1, label);
    }

    print!("Select the # to FORMAT as BESKARKEY: ");
    io::stdout().flush().ok();
    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    let idx: usize = choice
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid number"))?;
    let (devpath, _) = candidates
        .get(idx - 1)
        .ok_or_else(|| anyhow!("Choice out of range"))?;
    Ok(devpath.clone())
}

pub fn format_and_copy_key(dev: &str, label: &str, key_path: &str, key_name: &str) -> Result<()> {
    // Defensive unmount (best-effort)
    let _ = Command::new("umount").arg(dev).output();

    // Format as ext4 + label
    let st = Command::new("mkfs.ext4")
        .args(["-F", "-L", label, dev])
        .status()
        .context("mkfs.ext4 failed")?;
    if !st.success() {
        return Err(anyhow!("mkfs.ext4 returned non-zero"));
    }

    // udev settle
    let _ = Command::new("udevadm").args(["trigger"]).status();
    let _ = Command::new("udevadm")
        .args(["settle", "--timeout=10"])
        .status();

    // Mount by label
    let mnt = "/mnt/usb";
    let _ = Command::new("mkdir").args(["-p", mnt]).status();
    let st = Command::new("mount")
        .args([&format!("/dev/disk/by-label/{}", label), mnt])
        .status()
        .context("mount failed")?;
    if !st.success() {
        return Err(anyhow!("mount returned non-zero"));
    }

    // Copy key
    let dest = format!("{}/{}", mnt, key_name);
    let st = Command::new("cp").args([key_path, &dest]).status()?;
    if !st.success() {
        let _ = Command::new("umount").arg(mnt).status();
        return Err(anyhow!("copy key failed"));
    }
    let _ = Command::new("chmod").args(["400", &dest]).status();
    let _ = Command::new("sync").status();

    // Unmount
    let _ = Command::new("umount").arg(mnt).status();
    Ok(())
}
