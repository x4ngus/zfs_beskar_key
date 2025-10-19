use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use dialoguer::{Select};
use std::process::Command;

fn is_usb(dev: &str) -> Result<bool> {
    // Use udev to verify it's truly USB
    let out = Command::new("udevadm")
        .args(["info", "-q", "property", "-n", dev])
        .output()
        .context("udevadm info failed")?;
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.contains("ID_BUS=usb"))
}

pub fn choose_usb_device() -> Result<String> {
    // list all /dev/sd* and /dev/nvme* partitions; then filter by udev usb
    let out = Command::new("lsblk")
        .args(["-dpno", "NAME,TYPE,SIZE,MODEL,RM"])
        .output()
        .context("lsblk failed")?;
    let list = String::from_utf8_lossy(&out.stdout);
    let mut rows: Vec<(String, String)> = vec![]; // (dev, pretty)

    for line in list.lines() {
        // e.g. "/dev/sda1 part 28.7G SanDisk 1"
        let cols: Vec<_> = line.split_whitespace().collect();
        if cols.len() < 5 { continue; }
        let name = cols[0].to_string();
        let ty = cols[1];
        let size = cols[2];
        let model = cols[3..cols.len()-1].join(" ");
        let _rm = cols.last().unwrap();

        if ty != "part" { continue; }
        // verify USB by udev (parent block dev)
        let parent = Command::new("lsblk").args(["-no","PKNAME", &name]).output()?;
        let parent = String::from_utf8_lossy(&parent.stdout).trim().to_string();
        if parent.is_empty() { continue; }
        let parent_path = format!("/dev/{}", parent);
        if !is_usb(&parent_path)? { continue; }  // only true USB

        // skip common system partitions by name
        if name.contains("nvme0n1p2") || name.contains("nvme0n1p3") || name.contains("nvme0n1p4") {
            continue;
        }

        rows.push((name.clone(), format!("{:<16} {:>7}  {}", name, size, model)));
    }

    if rows.is_empty() {
        return Err(anyhow!("No removable USB partitions detected (ID_BUS=usb). Insert a USB drive and try again."));
    }

    // present numeric menu
    let items: Vec<String> = rows.iter().map(|(_, p)| p.clone()).collect();
    println!("\n{}", "Available USB partitions (eligible to format):".bold().bright_white());
    let idx = Select::new()
        .with_prompt("Select the device to FORMAT as BESKARKEY")
        .items(&items)
        .default(0)
        .interact()
        .context("selection failed")?;

    Ok(rows[idx].0.clone())
}

pub fn format_and_copy_key(dev: &str, label: &str, key_path: &str, key_name: &str) -> Result<()> {
    // mkfs ext4
    Command::new("mkfs.ext4")
        .args(["-F", "-L", label, dev])
        .status()
        .context("mkfs.ext4 failed")?;
    Command::new("udevadm").args(["trigger"]).status().ok();
    Command::new("udevadm").args(["settle","--timeout=10"]).status().ok();

    // mount, copy key, umount
    let mountpoint = "/mnt/usb";
    std::fs::create_dir_all(mountpoint).ok();
    Command::new("mount")
        .args([&format!("/dev/disk/by-label/{}", label), mountpoint])
        .status()
        .context("mount USB failed")?;

    let dest = format!("{}/{}", mountpoint, key_name);
    std::fs::copy(key_path, &dest).with_context(|| format!("copy {} -> {}", key_path, dest))?;
    Command::new("chmod").args(["400", &dest]).status().ok();
    Command::new("sync").status().ok();
    Command::new("umount").args([mountpoint]).status().ok();

    Ok(())
}
