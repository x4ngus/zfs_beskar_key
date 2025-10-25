// ============================================================================
// src/cmd/recover.rs – Recreate Beskar USB from recovery key
// ============================================================================

use crate::cmd::init::{
    derive_device_layout, dismantle_mounts, sanitize_key_name, select_usb_device, settle_udev,
    wipe_usb_token, write_key_to_usb,
};
use crate::ui::{Pace, Timing, UX};
use crate::util::recovery::decode_recovery_code;
use anyhow::{Context, Result};
use dialoguer::Password;

pub fn run_recover(ui: &UX, timing: &Timing, dataset: &str) -> Result<()> {
    ui.banner();
    ui.phase("Recovery // Tribute Recall");

    let recovery_code = Password::new()
        .with_prompt("Enter Armorer recovery sigil")
        .allow_empty_password(false)
        .interact()
        .context("read recovery key input")?;
    let raw_key = decode_recovery_code(&recovery_code)?;

    let device = select_usb_device(ui, false)?;
    let (usb_disk, usb_partition) = derive_device_layout(&device)?;

    dismantle_mounts(&usb_disk, ui)?;
    dismantle_mounts(&usb_partition, ui)?;

    ui.warn(&format!(
        "Wiping {} and {} before etching.",
        usb_disk, usb_partition
    ));
    wipe_usb_token(&usb_disk, &usb_partition, ui)?;
    settle_udev(ui)?;

    let key_filename = format!("{}.keyhex", sanitize_key_name(dataset));
    write_key_to_usb(&usb_partition, &key_filename, true, &raw_key[..], ui)?;

    ui.success("Tribute reborn on Beskar token.");
    ui.success("This is the Way.");
    timing.pace(Pace::Critical);
    Ok(())
}
