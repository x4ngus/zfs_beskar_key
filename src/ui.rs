// ============================================================================
// src/ui.rs – Mandalorian-inspired CLI experience with security-forward tone
// ============================================================================

use anyhow::Result;
use console::Style;
use std::{env, io::Write, thread, time::Duration};

const DEFAULT_RULE_WIDTH: usize = 64;

#[derive(Clone)]
struct Glyphs {
    info: &'static str,
    ok: &'static str,
    warn: &'static str,
    err: &'static str,
    security: &'static str,
    note: &'static str,
    trace: &'static str,
}

impl Glyphs {
    fn default() -> Self {
        Self {
            info: "✶",
            ok: "⛨",
            warn: "⚠",
            err: "✖",
            security: "🛡",
            note: "⋄",
            trace: "⌁",
        }
    }
}

#[derive(Clone)]
struct Theme {
    glyphs: Glyphs,
    info: Style,
    warn: Style,
    ok: Style,
    err: Style,
    accent: Style,
    muted: Style,
    security: Style,
    banner_edge: Style,
    banner_fill: Style,
}

impl Theme {
    fn default() -> Self {
        Self {
            glyphs: Glyphs::default(),
            info: Style::new().color256(110),
            warn: Style::new().color256(208).bold(),
            ok: Style::new().color256(114).bold(),
            err: Style::new().color256(196).bold(),
            accent: Style::new().color256(45).bold(),
            muted: Style::new().color256(244),
            security: Style::new().color256(39).bold(),
            banner_edge: Style::new().color256(45).bold(),
            banner_fill: Style::new().color256(37),
        }
    }

    fn rule(&self, width: usize) -> String {
        let clamped = width.clamp(18, 96);
        let pattern = ["╼", "─", "╾", "─"];
        let mut buf = String::with_capacity(clamped * 2);
        for i in 0..clamped {
            buf.push_str(pattern[i % pattern.len()]);
        }
        buf
    }
}

// --------------------------- Pacing -----------------------------------------

/// Context of a CLI action for adaptive pacing.
#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub enum Pace {
    /// Major success, completion, or irreversible action — let it breathe.
    Critical,
    /// Standard informational update.
    Info,
    /// Waiting for user input or a quick transition.
    Prompt,
    /// Error message or invalid input — keep it snappy.
    Error,
    /// Rapid fire debug/trace output.
    Verbose,
}

/// Adaptive timing controller.
/// - Skips delays entirely in quiet mode
/// - Halves delays in verbose mode
pub struct Timing {
    base_delay: Duration,
    #[allow(dead_code)]
    fast_delay: Duration,
    slow_delay: Duration,
    pub verbose_mode: bool,
    pub quiet_mode: bool,
}

impl Timing {
    pub fn new(verbose: bool, quiet: bool) -> Self {
        let base = env::var("BESKAR_UI_DELAY_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(2);

        Self {
            base_delay: Duration::from_secs(base),
            fast_delay: Duration::from_millis(500),
            slow_delay: Duration::from_secs(3),
            verbose_mode: verbose,
            quiet_mode: quiet,
        }
    }

    /// Adaptive pause chosen by pace type.
    pub fn pace(&self, p: Pace) {
        if self.quiet_mode {
            return;
        }

        let duration = match p {
            Pace::Critical => self.slow_delay,
            Pace::Info => self.base_delay,
            Pace::Prompt => Duration::from_millis(700),
            Pace::Error => Duration::from_millis(300),
            Pace::Verbose => Duration::from_millis(200),
        };

        let effective = if self.verbose_mode {
            duration / 2
        } else {
            duration
        };

        thread::sleep(effective);
    }

    #[allow(dead_code)]
    pub fn pause_secs(&self, secs: u64) {
        if !self.quiet_mode {
            thread::sleep(Duration::from_secs(secs));
        }
    }
}

// --------------------------- UX Facade --------------------------------------

/// Centralized UX facade for Mandalorian-flavored messaging.
pub struct UX {
    pub verbose: bool,
    pub quiet: bool,
    theme: Theme,
}

impl UX {
    pub fn new(verbose: bool, quiet: bool) -> Self {
        Self {
            verbose,
            quiet,
            theme: Theme::default(),
        }
    }

    #[allow(dead_code)]
    pub fn set_verbose(&mut self, enable: bool) {
        self.verbose = enable;
    }

    #[allow(dead_code)]
    pub fn trace(&self, msg: &str) {
        if self.verbose && !self.quiet {
            println!(
                "{} {}",
                self.theme.muted.apply_to(self.theme.glyphs.trace),
                self.theme.muted.apply_to(msg)
            );
        }
    }

    pub fn info(&self, msg: &str) {
        if self.quiet {
            return;
        }
        println!(
            "{} {}",
            self.theme.info.apply_to(self.theme.glyphs.info),
            msg
        );
    }

    pub fn success(&self, msg: &str) {
        if self.quiet {
            return;
        }
        println!(
            "{} {}",
            self.theme.ok.apply_to(self.theme.glyphs.ok),
            self.theme.ok.apply_to(msg)
        );
    }

    pub fn warn(&self, msg: &str) {
        if self.quiet {
            return;
        }
        println!(
            "{} {}",
            self.theme.warn.apply_to(self.theme.glyphs.warn),
            msg
        );
    }

    pub fn error(&self, msg: &str) {
        eprintln!(
            "{} {}",
            self.theme.err.apply_to(self.theme.glyphs.err),
            self.theme.err.apply_to(msg)
        );
    }

    pub fn security(&self, msg: &str) {
        if self.quiet {
            return;
        }
        println!(
            "{} {}",
            self.theme.security.apply_to(self.theme.glyphs.security),
            self.theme.security.apply_to(msg)
        );
    }

    pub fn note(&self, msg: &str) {
        if self.quiet {
            return;
        }
        println!(
            "{} {}",
            self.theme.muted.apply_to(self.theme.glyphs.note),
            self.theme.muted.apply_to(msg)
        );
    }

    pub fn phase(&self, title: &str) {
        if self.quiet {
            return;
        }
        let normalized = title.trim().to_uppercase();
        println!(
            "{}",
            self.theme.accent.apply_to(format!("╔═ {} ═╗", normalized))
        );
        self.divider();
    }

    pub fn divider(&self) {
        if self.quiet {
            return;
        }
        println!(
            "{}",
            self.theme
                .muted
                .apply_to(self.theme.rule(DEFAULT_RULE_WIDTH))
        );
    }

    pub fn data_panel(&self, title: &str, rows: &[(&str, String)]) {
        if self.quiet {
            return;
        }
        let label_width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        println!(
            "{}",
            self.theme
                .accent
                .apply_to(format!("╟─ {} ─╢", title.to_uppercase()))
        );
        for (label, value) in rows {
            println!(
                "{} {:>width$} {} {}",
                self.theme.muted.apply_to("✦"),
                label,
                self.theme.muted.apply_to("::"),
                value,
                width = label_width
            );
        }
        println!(
            "{}",
            self.theme
                .muted
                .apply_to(self.theme.rule(DEFAULT_RULE_WIDTH))
        );
    }

    pub fn banner(&self) {
        if self.quiet {
            return;
        }
        const BODY: usize = 58;
        let border = "═".repeat(BODY + 2);
        println!(
            "{}",
            self.theme.banner_edge.apply_to(format!("╔{}╗", border))
        );

        let crest = [
            "            /\\        /\\        /\\",
            "           /  \\      /  \\      /  \\",
            "          / /\\ \\    / /\\ \\    / /\\ \\",
            "         / ____ \\  / ____ \\  / ____ \\",
            "        /_/    \\_\\/_/    \\_\\/_/    \\_\\",
        ];

        for line in crest {
            let clipped = line.chars().take(BODY).collect::<String>();
            println!(
                "{}",
                self.theme
                    .banner_fill
                    .apply_to(format!("║ {:<width$} ║", clipped, width = BODY))
            );
        }

        let lines = [
            "Beskar Codex Forge Terminal — designed for defense.",
            "Creed: Temper keys, simulate failure, safeguard the clan.",
            "Discipline: Vault drill → Init → Dracut → Self-test.",
        ];

        for line in lines {
            let clipped = line.chars().take(BODY).collect::<String>();
            println!(
                "{}",
                self.theme
                    .banner_fill
                    .apply_to(format!("║ {:<width$} ║", clipped, width = BODY))
            );
        }

        println!(
            "{}",
            self.theme.banner_edge.apply_to(format!("╚{}╝", border))
        );
        self.note("Secure channel sealed. The Armorer watches this forge.");
        self.divider();
    }

    pub fn banner_flicker(&self, timing: &Timing) -> Result<()> {
        if self.quiet {
            return Ok(());
        }

        let mut out = std::io::stdout();
        let sequences = [
            "Kindling plasma furnace",
            "Calibrating vibro-press harmonics",
            "Inscribing the Resol'nare creed",
        ];

        for seq in sequences.iter() {
            write!(
                out,
                "\r{}",
                self.theme.accent.apply_to(format!(":: {} ::", seq))
            )?;
            out.flush()?;
            thread::sleep(Duration::from_millis(140));
            write!(out, "\r{}", " ".repeat(64))?;
            out.flush()?;
            thread::sleep(Duration::from_millis(90));
        }
        println!();
        timing.pace(Pace::Prompt);
        Ok(())
    }
}
