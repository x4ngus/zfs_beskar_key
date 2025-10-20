use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

/// JSON struct for parsing `lsblk -J -O`
#[derive(Debug, Deserialize, Clone)]
struct Device {
    name: Option<String>,

    #[serde(default, deserialize_with = "deserialize_boolish")]
    rm: Option<u8>,

    #[serde(default, deserialize_with = "deserialize_boolish")]
    hotplug: Option<u8>,

    #[serde(default)]
    tran: Option<String>,

    #[serde(default)]
    model: Option<String>,

    #[serde(default)]
    size: Option<String>,

    #[serde(default, rename = "type")]
    kind: Option<String>,

    #[serde(default)]
    children: Option<Vec<Device>>,
}

/// Deserialize true/false/0/1/"true"/"false" → Option<u8>
fn deserialize_boolish<'de, D>(deserializer: D) -> std::result::Result<Option<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Boolish {
        Bool(bool),
        Num(u64),
        Str(String),
        Null,
    }

    let parsed = Boolish::deserialize(deserializer)?;
    let val = match parsed {
        Boolish::Bool(b) => Some(if b { 1 } else { 0 }),
        Boolish::Num(n) => Some((n != 0) as u8),
        Boolish::Str(s) => {
            let s = s.to_lowercase();
            if s == "true" || s == "1" {
                Some(1)
            } else if s == "false" || s == "0" {
                Some(0)
            } else {
                None
            }
        }
        Boolish::Null => None,
    };
    Ok(val)
}

/// Run a shell command and return trimmed stdout
fn sh_out(cmd: &str) -> Result<String> {
    let out = Command::new("bash").args(["-lc", cmd]).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Flatten devices recursively
fn flatten_devices(devs: &[Device], list: &mut Vec<Device>) {
    for d in devs {
        list.push(d.clone());
        if let Some(ch) = &d.children {
            flatten_devices(ch, list);
        }
    }
}

/// Select USB target interactively
pub fn select_usb_partition() -> Result<String> {
    println!("🔍 Scanning connected removable USB devices...\n");

    let output = Command::new("lsblk")
        .args(["-J", "-O"])
        .output()
        .context("failed to run lsblk")?;

    let json_str = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).context("failed to parse lsblk JSON")?;
    let root = parsed
        .get("blockdevices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("invalid lsblk JSON: no blockdevices field"))?;

    let devices: Vec<Device> = serde_json::from_value(root.clone().into())
        .context("failed to deserialize lsblk devices")?;
    let mut flat = Vec::new();
    flatten_devices(&devices, &mut flat);

    let usb_list: Vec<Device> = flat
        .into_iter()
        .filter(|d| {
            let is_usb = d.tran.as_deref() == Some("usb");
            let rm = d.rm.unwrap_or(0) == 1;
            let hot = d.hotplug.unwrap_or(0) == 1;
            let kind = d.kind.as_deref() == Some("disk") || d.kind.as_deref() == Some("part");
            is_usb || ((rm || hot) && kind)
        })
        .collect();

    if usb_list.is_empty() {
        return Err(anyhow!("No removable USB devices found."));
    }

    println!("⚙️  Select a device to dedicate as BESKARKEY:\n");
    for (i, d) in usb_list.iter().enumerate() {
        let name = d.name.clone().unwrap_or_default();
        let model = d.model.clone().unwrap_or_default();
        let size = d.size.clone().unwrap_or_else(|| "?".to_string());
        println!("  [{}] /dev/{:<8}  {:>8}  {:<20}", i + 1, name, size, model);
    }

    print!("\nEnter number: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let idx: usize = buf.trim().parse().context("invalid number")?;
    if idx == 0 || idx > usb_list.len() {
        return Err(anyhow!("Selection out of range."));
    }

    let selected = usb_list[idx - 1].name.clone().unwrap_or_default();
    let dev_path = format!("/dev/{}", selected);
    Ok(dev_path)
}

/// Ensure no mounts left
fn unmount_any(dev: &str) {
    let _ = Command::new("bash")
        .args([
            "-lc",
            &format!(
                "for t in $(findmnt -rn -S {dev} -o TARGET); do umount -lf \"$t\"; done; true"
            ),
        ])
        .status();
}

/// Partition, format, label, and copy the ZFS key to USB
pub fn format_and_copy_key(
    dev_or_part: &str,
    label: &str,
    key_path: &str,
    key_name: &str,
) -> Result<()> {
    if !Path::new(key_path).exists() {
        return Err(anyhow!("Key file not found at {key_path}"));
    }

    println!("⚠️  About to format {dev_or_part} — all data will be erased!");
    print!("Proceed? [y/N]: ");
    io::stdout().flush()?;
    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm)?;
    if confirm.trim().to_lowercase() != "y" {
        return Err(anyhow!("User aborted formatting."));
    }

    unmount_any(dev_or_part);

    Command::new("wipefs")
        .args(["-a", dev_or_part])
        .status()
        .context("Failed to wipe target")?;

    Command::new("mkfs.ext4")
        .args(["-F", "-L", label, dev_or_part])
        .status()
        .context("Failed to format ext4")?;

    let mnt = "/mnt/beskartmp";
    fs::create_dir_all(mnt)?;
    Command::new("mount")
        .args([dev_or_part, mnt])
        .status()
        .context("Failed to mount partition")?;

    let dest = format!("{mnt}/{key_name}");
    fs::copy(key_path, &dest).context("Failed to copy keyfile to USB")?;

    let src_sum = sh_out(&format!("sha256sum {src} | cut -d' ' -f1", src = key_path))?;
    let dst_sum = sh_out(&format!("sha256sum {dst} | cut -d' ' -f1", dst = dest))?;
    if src_sum != dst_sum {
        let _ = Command::new("umount").args([mnt]).status();
        return Err(anyhow!("Key integrity verification failed"));
    }

    let _ = Command::new("chmod").args(["600", &dest]).status();
    let _ = Command::new("sync").status();
    let _ = Command::new("umount").args([mnt]).status();

    println!("\n✅ USB key prepared and labeled {label}. Path verified: {dev_or_part}");
    Ok(())
}
