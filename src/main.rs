mod ui;
mod usb;
mod zfs;
mod dracut;

use anyhow::{Context, Result};
use ui::ForgeUI;

const POOL: &str = "rpool";
const USB_LABEL: &str = "BESKARKEY";
const KEY_DIR: &str = "/etc/zfs/keys";
const KEY_NAME: &str = "holocron.key";
const KEY_PATH: &str = "/etc/zfs/keys/holocron.key";

fn main() -> Result<()> {
    let mut ui = ForgeUI::new()?;

    // Opening cinematic
    ui.banner(r#"ZFS USB SECURITY KEY — "This is the Way.""#)?;
    ui.blast(900)?;
    ui.step("Initializing the forge — verifying your environment")?;
    ui.blaster(5, "Checking tools and pool integrity")?;
    zfs::preflight(POOL)
        .context("Preflight check failed: required tool or pool missing")?;
    ui.pause(1);

    // Step 1: Generate/attach key (keep passphrase fallback)
    ui.step("Forging the beskar ingot — generating your encryption key")?;
    ui.blaster(15, "Creating raw key material & attaching to rpool")?;
    zfs::ensure_raw_key(KEY_DIR, KEY_PATH, POOL)
        .context("Failed to attach raw key to rpool")?;
    // Keep passphrase fallback explicit for safety.
    zfs::set_prop(POOL, "keylocation", "prompt")
        .context("Failed to set keylocation=prompt on rpool")?;
    ui.pause(1);

    // Step 2: USB selection & copy
    ui.step("Tempering the forge — choose your USB courier")?;
    ui.blaster(30, "Enumerating removable devices")?;
    let dev = usb::select_usb_partition().context("USB selection failed")?;
    ui.pause(1);

    ui.step("Binding the clans — format USB and seal the key")?;
    ui.blaster(45, "Formatting ext4, labeling as BESKARKEY, copying key")?;
    usb::format_and_copy_key(&dev, USB_LABEL, KEY_PATH, KEY_NAME)
        .context("Failed to format/copy key to USB")?;
    ui.pause(1);

    // Step 3: Unify child datasets
    ui.step("Engraving sigils — unifying all dataset keys")?;
    ui.blaster(60, "Binding encryption roots to rpool")?;
    zfs::force_converge_children(POOL)
        .context("Datasets still independent after inheritance pass")?;
    ui.pause(1);

    // Step 4: Install Dracut hook & rebuild initramfs
    ui.step("Etching runes — integrating Dracut hook for autounlock")?;
    ui.blaster(75, "Installing module and rebuilding initramfs")?;
    dracut::install_hook(USB_LABEL, KEY_NAME)
        .context("Failed to install Dracut hook/module")?;
    dracut::rebuild_and_verify()
        .context("Initramfs rebuild/verification failed")?;
    ui.pause(1);

    // Step 5: **Deterministic** self-test inside rpool
    ui.step("Testing the forge — verifying keyfile and passphrase")?;
    ui.blaster(90, "Validating key operations in a temporary dataset")?;
    zfs::self_test_dual_unlock(KEY_PATH, POOL)
        .context("Self-test failed (keyfile or passphrase validation)")?;
    ui.pause(1);

    // Completion
    ui.blaster(100, "Final inspection — armor complete")?;
    ui.done("ZFS USB autounlock configured successfully!")?;
    ui.quote()?;
    ui.close()?;
    Ok(())
}
