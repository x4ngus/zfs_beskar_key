mod dracut;
mod ui;
mod usb;
mod zfs;

use anyhow::{Context, Result};
use ui::BlasterUI;

const POOL: &str = "rpool";
const USB_LABEL: &str = "BESKARKEY";
const KEY_DIR: &str = "/etc/zfs/keys";
const KEY_NAME: &str = "holocron.key";
const KEY_PATH: &str = "/etc/zfs/keys/holocron.key";

fn main() -> Result<()> {
    // Initialize Mandalorian Blaster UI
    let mut ui = BlasterUI::new(7);

    // ─────────────────────────────
    // 0. Preflight checks
    // ─────────────────────────────
    ui.section(
        "Checking Environment",
        "Scanning for ZFS binaries and dracut modules",
        "red",
    );
    ui.log("/usr/sbin/zfs, /usr/sbin/zpool, /usr/bin/dracut verified");
    zfs::preflight(POOL)
        .context("Preflight check failed: missing zfs/zpool/dracut/lsinitrd/udevadm or rpool")?;
    ui.progress("Checking environment");

    // ─────────────────────────────
    // 1. Key creation / attach to rpool
    // ─────────────────────────────
    ui.section(
        "Forging Beskar Ingot",
        "Generating and attaching encryption key",
        "yellow",
    );
    ui.log("Generating 32-byte key & binding to rpool...");
    zfs::ensure_raw_key(KEY_DIR, KEY_PATH, POOL)
        .context("Failed to create or attach raw key to rpool")?;
    zfs::set_prop(POOL, "keylocation", "prompt")?;
    ui.progress("Binding encryption key");

    // ─────────────────────────────
    // 2. USB detection and formatting
    // ─────────────────────────────
    ui.section(
        "Tempering the Forge",
        "Detecting removable USB partitions",
        "blue",
    );
    ui.log("Enumerating connected USB drives...");
    let dev = usb::select_usb_partition().context("No suitable USB device found")?;
    ui.progress("Enumerating USB");

    let confirm = ui.prompt("Proceed with formatting the selected USB drive?");
    if confirm != "y" {
        println!("Aborted by user. The forge cools silently...");
        return Ok(());
    }

    ui.log("Formatting ext4 filesystem, labeling, and engraving key...");
    usb::format_and_copy_key(&dev, USB_LABEL, KEY_PATH, KEY_NAME)
        .context("Failed to format and copy key to USB")?;
    ui.progress("Formatting and copying key");

    // ─────────────────────────────
    // 3. Unify encryption inheritance
    // ─────────────────────────────
    ui.section(
        "Engraving Sigils",
        "Unifying encryption roots across child datasets",
        "yellow",
    );
    ui.log("Forcing all children of rpool to inherit keylocation from rpool");
    zfs::force_converge_children(POOL)
        .context("Some child datasets are not inheriting rpool’s key")?;
    ui.progress("Unifying encryption roots");

    // ─────────────────────────────
    // 4. Install Dracut hook and rebuild initramfs
    // ─────────────────────────────
    ui.section(
        "Etching Runes",
        "Installing Dracut hook and rebuilding initramfs",
        "red",
    );
    ui.log("Installing /usr/lib/dracut/modules.d/90zfs-usbkey...");
    dracut::install_hook(USB_LABEL, KEY_NAME)?;
    ui.log("Rebuilding initramfs with Dracut...");
    dracut::rebuild_and_verify()?;
    ui.progress("Embedding hooks");

    // ─────────────────────────────
    // 5. Run non-invasive verification test
    // ─────────────────────────────
    ui.section(
        "Testing the Forge",
        "Running non-invasive dual unlock self-test",
        "blue",
    );
    ui.log("Testing both keyfile unlock and passphrase fallback...");
    zfs::self_test_dual_unlock(KEY_PATH)
        .context("Self-test failed: keyfile or passphrase path broken")?;
    ui.progress("Self-test verification");

    // ─────────────────────────────
    // 6. Final completion sequence
    // ─────────────────────────────
    ui.section(
        "Final Blessing",
        "ZFS autounlock via USB keyfile verified",
        "yellow",
    );
    ui.log("rpool and all child datasets bound to inherited encryption root.");
    ui.progress("Forge Complete");
    ui.complete();

    Ok(())
}
