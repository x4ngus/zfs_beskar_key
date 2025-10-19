mod ui;
mod usb;
mod zfs;
mod dracut;

use anyhow::{Context, Result};
use ui::{banner, progress, step, pause, done, quote};

const POOL: &str = "rpool";
const USB_LABEL: &str = "BESKARKEY";
const KEY_DIR: &str = "/etc/zfs/keys";
const KEY_NAME: &str = "holocron.key";
const KEY_PATH: &str = "/etc/zfs/keys/holocron.key";

fn main() -> Result<()> {
    banner(r#"ZFS USB SECURITY KEY — "This is the way.""#)?;

    // 0) Preconditions
    progress(5, "Checking environment")?;
    zfs::preflight(POOL)
        .context("Preflight check failed (zfs/zpool/dracut/lsinitrd/udevadm or rpool missing)")?;
    pause(1);

    // 1) Ensure key exists & attach raw key to rpool (keep passphrase fallback)
    step("Forging beskar ingot — generating/attaching key");
    progress(15, "Generating 32-byte key & binding to rpool")?;
    zfs::ensure_raw_key(KEY_DIR, KEY_PATH, POOL)
        .context("Failed to create/attach raw key to rpool")?;
    // Keep passphrase fallback explicitly.
    zfs::set_prop(POOL, "keylocation", "prompt")?;
    pause(1);

    // 2) Pick USB and prepare it
    step("Tempering the forge — detect removable USB partitions");
    progress(30, "Enumerating USB partitions")?;
    let dev = usb::select_usb_partition()?;
    pause(1);

    step("Binding the clans — format USB and copy key");
    progress(45, "Formatting ext4, labeling, copying key")?;
    usb::format_and_copy_key(&dev, USB_LABEL, KEY_PATH, KEY_NAME)
        .context("USB format/copy failed")?;
    pause(1);

    // 3) Converge children to inherit rpool’s key
    step("Engraving sigils — unify ZFS encryption roots");
    progress(60, "Inheriting child dataset keys from rpool")?;
    zfs::force_converge_children(POOL)
        .context("One or more datasets still not inheriting rpool’s key")?;
    pause(1);

    // 4) Install Dracut hook and rebuild
    step("Etching runes — installing Dracut hook");
    progress(75, "Writing module and rebuilding initramfs")?;
    dracut::install_hook(USB_LABEL, KEY_NAME)?;
    dracut::rebuild_and_verify()?;
    pause(1);

    // 5) Non-invasive self-test
    step("Testing the forge — verifying keyfile & passphrase unlock");
    progress(90, "Running sandboxed dual-unlock self-test")?;
    zfs::self_test_dual_unlock(KEY_PATH)
        .context("Self-test failed (keyfile or passphrase path broken)")?;
    pause(1);

    progress(100, "Final inspection")?;
    done("Setup complete! USB autounlock + passphrase fallback verified.");
    quote();
    Ok(())
}
