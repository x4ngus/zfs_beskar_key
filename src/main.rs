mod ui;
mod usb;
mod zfs;
mod dracut;

use anyhow::{Context, Result};
use ui::{blaster_step, pause, banner, success, warn, info};
use dialoguer::Confirm;

const POOL: &str = "rpool";
const USB_LABEL: &str = "BESKARKEY";
const KEY_NAME: &str = "holocron.key";
const KEY_DIR: &str = "/etc/zfs/keys";
const KEY_PATH: &str = "/etc/zfs/keys/holocron.key";

fn main() -> Result<()> {
    banner(r#"ZFS USB SECURITY KEY — "This is the way.""#)?;

    // 0) Preconditions (tools + rpool)
    blaster_step(5, "Checking tools & rpool")?;
    zfs::require_tools(&["zfs","zpool","dracut","lsinitrd","udevadm"])?;
    zfs::assert_pool_exists(POOL).context("rpool not found")?;
    pause(2);

    // 1) Key creation (confirm)
    blaster_step(15, "Forge key (32-byte raw)")?;
    if Confirm::new().with_prompt("Generate a new 32-byte raw key for rpool?").interact()? {
        ui::this_is_the_way();
        zfs::create_key_raw(KEY_DIR, KEY_PATH, POOL)?;
        // keep passphrase fallback
        zfs::set_prop(POOL, "keylocation", "prompt")?;
    } else {
        ui::forge_cools_and_exit();
    }
    pause(2);

    // 2) USB selection + format (confirm)
    blaster_step(35, "Select and format USB")?;
    let dev = usb::choose_usb_device()?;
    if Confirm::new().with_prompt(format!("Format {} as {} and copy key?", &dev, USB_LABEL)).interact()? {
        ui::this_is_the_way();
        usb::format_and_copy_key(&dev, USB_LABEL, KEY_PATH, KEY_NAME)?;
    } else {
        ui::forge_cools_and_exit();
    }
    pause(2);

    // 3) Converge children to inherit rpool
    blaster_step(55, "Unify encryption roots under rpool")?;
    zfs::force_converge_children(POOL)?;
    pause(2);

    // 4) Install Dracut hook (confirm)
    blaster_step(75, "Install persistent Dracut hook")?;
    if Confirm::new().with_prompt("Install /etc/dracut/modules.d/90zfs-usbkey and rebuild initramfs?").interact()? {
        ui::this_is_the_way();
        dracut::install_hook(USB_LABEL, KEY_NAME)?;
        dracut::rebuild_initramfs()?;
        dracut::verify_initramfs_contains()?;
    } else {
        ui::forge_cools_and_exit();
    }
    pause(2);

    // 5) Non-invasive self-test (no touching rpool)
    blaster_step(90, "Run sandboxed dual-unlock self-test")?;
    zfs::self_test_dual_unlock(KEY_PATH)?;
    pause(1);

    // 6) Final output
    blaster_step(100, "Final inspection")?;
    success("Setup complete!");
    info(&format!("Key file: {}", KEY_PATH));
    info(&format!("USB label: /dev/disk/by-label/{}", USB_LABEL));
    info("Reboot with USB inserted → auto-unlock");
    info("Reboot without USB       → passphrase prompt");
    ui::armorer_quote();

    Ok(())
}
