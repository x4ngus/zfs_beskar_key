// ============================================================================
// src/menu.rs – Interactive console menu (discoverability & flow control)
// ============================================================================

use console::Style;
use std::io::{self, Write};

use crate::ui::{Pace, Timing, UX};

#[derive(Debug, Clone)]
pub enum MenuChoice {
    Init,
    InitSafe,
    VaultDrill,
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
    // ------------------------------------------------------------------------
    // Menu items
    // ------------------------------------------------------------------------
    let entries = [
        (
            MenuChoice::Init,
            "TEMPER THE TRIBUTE  —  Reforge the offered beskar key from first principles",
            "The crucible ignites. Present the device and dataset without hesitation.",
        ),
        (
            MenuChoice::InitSafe,
            "TEMPER THE TRIBUTE (GUIDED)  —  Pause before each irreversible strike",
            "We proceed in measured beats; confirm each action as I name it.",
        ),
        (
            MenuChoice::VaultDrill,
            "VAULT DRILL  —  Rehearse the unlock path within a holoforge simulation",
            "Your clan trains safely here; follow the sequence and observe the results.",
        ),
        (
            MenuChoice::Doctor,
            "ARMORER'S AUDIT  —  Inspect defenses and prescribe repairs",
            "I will walk the perimeter and call out any plates that ring hollow.",
        ),
        (
            MenuChoice::Quit,
            "BANK THE COALS  —  Withdraw from the forge console",
            "The embers hold. Return when a new tribute is ready.",
        ),
    ];
    let motif = ["╳", "╂", "╋", "╂"];
    let inner_width = crate::ui::BANNER_BODY_WIDTH.saturating_sub(2);
    let frame_span = "═".repeat(crate::ui::BANNER_BODY_WIDTH + 2);
    let frame_style = Style::new().color256(202).bold();
    let divider_style = Style::new().color256(208).bold();
    let divider_span = "═".repeat(crate::ui::BANNER_BODY_WIDTH + 2);
    let header_inner = format!(
        "{:^width$}",
        "SELECT NEXT FORGE DIRECTIVE",
        width = crate::ui::BANNER_BODY_WIDTH + 2
    );

    println!("{}", frame_style.apply_to(format!("╔{}╗", frame_span)));
    println!("{}", frame_style.apply_to(format!("║{}║", header_inner)));
    println!("{}", divider_style.apply_to(format!("╠{}╣", divider_span)));

    let row_style = Style::new().color256(221).bold();
    for (idx, (_choice, text, _ack)) in entries.iter().enumerate() {
        let label = format!("{:>2}. {}", idx + 1, text);
        let centered = format!("{:^width$}", label, width = inner_width);
        let left_edge = frame_style.apply_to("║").to_string();
        let right_edge = frame_style.apply_to("║").to_string();
        let left_motif = Style::new()
            .color256(208)
            .bold()
            .apply_to(motif[idx % motif.len()])
            .to_string();
        let right_motif = Style::new()
            .color256(214)
            .bold()
            .apply_to(motif[(idx + 2) % motif.len()])
            .to_string();
        let body = row_style.apply_to(centered).to_string();
        println!(
            "{}{}{}{}{}",
            left_edge, left_motif, body, right_motif, right_edge
        );
    }

    println!("{}", frame_style.apply_to(format!("╚{}╝", frame_span)));
    println!();

    let mut selection: Option<MenuChoice> = None;
    let mut selected_idx: Option<usize> = None;
    while selection.is_none() {
        print!(
            "{}",
            Style::new()
                .color256(221)
                .bold()
                .apply_to("Directive [1-5 or Q to withdraw]: ")
        );
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            ui.warn("Input unreadable — try again.");
            continue;
        }
        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("q") {
            selection = Some(MenuChoice::Quit);
            selected_idx = None;
            break;
        }
        match trimmed.parse::<usize>() {
            Ok(n) if (1..=entries.len()).contains(&n) => {
                selection = Some(entries[n - 1].0.clone());
                selected_idx = Some(n - 1);
            }
            _ => {
                ui.warn("Invalid choice — choose a menu number or 'Q'.");
            }
        }
    }

    let choice = selection.unwrap_or(MenuChoice::Quit);

    if let Some(idx) = selected_idx {
        ui.info(entries[idx].2);
    } else {
        ui.info("The forge rests in silence until you bring another charge.");
    }
    timing.pace(Pace::Prompt);
    Some(choice)
}
