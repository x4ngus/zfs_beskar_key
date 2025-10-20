// ============================================================================
// src/menu.rs – Interactive console menu (discoverability & flow control)
// ============================================================================

use console::{style, Style};
use dialoguer::{theme::ColorfulTheme, Select};

use crate::ui::{Pace, Timing, UX};

#[derive(Debug, Clone)]
pub enum MenuChoice {
    Init,
    Unlock,
    Lock,
    Status,
    Doctor,
    Quit,
}

/// Display the interactive Beskar menu (unless quiet mode is enabled).
/// Returns a `MenuChoice` or None if user quits.
pub fn show_main_menu(ui: &UX, timing: &Timing) -> Option<MenuChoice> {
    if ui.quiet {
        return None;
    }

    ui.banner();
    let _ = ui.banner_flicker(timing);

    // ------------------------------------------------------------------------
    // Theme setup — consistent with CLI look & feel
    // ------------------------------------------------------------------------
    let theme = ColorfulTheme {
        // Active / inactive items
        active_item_style: Style::new().cyan().bold(),
        inactive_item_style: Style::new().dim(),

        // Prefixes: need StyledObject<String>, so convert &str → String
        active_item_prefix: style("⟶".to_string()).cyan(),
        inactive_item_prefix: style(" ".to_string()),

        // Prompt style expects a plain Style, not a StyledObject
        prompt_style: Style::new().yellow().bold(),

        // Neutral list text
        values_style: Style::new(),

        ..Default::default()
    };

    // ------------------------------------------------------------------------
    // Menu items
    // ------------------------------------------------------------------------
    let options = vec![
        "Temper Beskar Key       [Forge USB token]",
        "Unseal the Vault        [USB-first unlock]",
        "Reseal the Vault        [Lock dataset]",
        "Clan Status Scan        [Diagnostics]",
        "Summon the Armorer      [Doctor checks]",
        "Leave the Covert",
    ];

    let selection = Select::with_theme(&theme)
        .with_prompt("Choose your next forge action")
        .items(&options)
        .default(0)
        .interact()
        .unwrap_or(options.len() - 1);

    let choice = match selection {
        0 => MenuChoice::Init,
        1 => MenuChoice::Unlock,
        2 => MenuChoice::Lock,
        3 => MenuChoice::Status,
        4 => MenuChoice::Doctor,
        _ => MenuChoice::Quit,
    };

    ui.info(&format!(
        "Operation queued: {} // The Armorer nods.",
        options[selection]
    ));
    timing.pace(Pace::Prompt);
    Some(choice)
}
