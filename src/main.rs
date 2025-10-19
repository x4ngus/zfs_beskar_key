mod dracut;
mod ui;
mod usb;
mod zfs;

use anyhow::{Context, Result};
use ui::ForgeUI;

const POOL: &str = "rpool";
const USB_LABEL: &str = "BESKARKEY";
const KEY_DIR: &str = "/etc/zfs/keys";
const KEY_NAME: &str = "holocron.key";
const KEY_PATH: &str = "/etc/zfs/keys/holocron.key";

fn main() -> Result<()> {
    let mut forge = ForgeUI::new()?;

    // Opening cinematic
    forge.banner(r#"ZFS USB SECURITY KEY — "This is the Way.""#)?;
    forge.blast(1200)?;
    forge.step("Initializing the forge — verifying your environment")?;
    forge.blaster(5, "Checking tools and pool integrity")?;
    zfs::preflight(POOL).context("Preflight check failed: required tool or pool missing")?;
    forge.pause(1);

    // Step 1: Generate/attach encryption key
    forge.step("Forging the beskar ingot — generating your encryption key")?;
    forge.blaster(15, "Creating raw key material")?;
    zfs::ensure_raw_key(KEY_DIR, KEY_PATH, POOL).context("Failed to attach raw key to rpool")?;
    zfs::set_prop(POOL, "keylocation", "prompt")?;
    forge.pause(1);

    // Step 2: USB selection and copy
    forge.step("Tempering the forge — select your USB for the key")?;
    forge.blaster(30, "Enumerating removable devices")?;
    let dev = usb::select_usb_partition().context("USB selection failed")?;
    forge.pause(1);

    forge.step("Binding the clans — formatting USB and sealing key")?;
    forge.blaster(45, "Formatting ext4, labeling as BESKARKEY")?;
    usb::format_and_copy_key(&dev, USB_LABEL, KEY_PATH, KEY_NAME)
        .context("Failed to format or copy key to USB")?;
    forge.pause(1);

    // Step 3: Unify child datasets
    forge.step("Engraving sigils — unifying all dataset keys")?;
    forge.blaster(60, "Binding encryption roots to rpool")?;
    zfs::force_converge_children(POOL)
        .context("Datasets still independent after inheritance pass")?;
    forge.pause(1);

    // Step 4: Install Dracut hook and rebuild initramfs
    forge.step("Etching runes — integrating Dracut hook for autounlock")?;
    forge.blaster(75, "Installing module and rebuilding initramfs")?;
    dracut::install_hook(USB_LABEL, KEY_NAME)?;
    dracut::rebuild_and_verify()?;
    forge.pause(2);

    // Step 5: Test unlock and fallback
    forge.step("Testing the forge — verifying keyfile and passphrase")?;
    forge.blaster(90, "Performing non-invasive dual unlock test")?;
    zfs::self_test_dual_unlock(KEY_PATH)
        .context("Self-test failed: keyfile or passphrase invalid")?;
    forge.pause(1);

    // Completion
    forge.blaster(100, "Final inspection — armor complete")?;
    forge.done("✅ ZFS USB autounlock configured successfully!")?;
    forge.quote()?;
    forge.close()?;

    Ok(())
}
