// ============================================================================
// src/ui.rs – Unified UI layer (messaging, pacing, banner, flicker, gradients)
// - Adaptive pacing (quiet/verbose aware)
// - Colorized messages with restrained personality
// - Flicker intro and subtle gradient accents (disabled in --quiet)
// - Centralized UX helpers so business logic stays clean
// ============================================================================

use anyhow::Result;
use console::{style, Emoji, Style};
use std::{env, io::Write, thread, time::Duration};

// --------------------------- Pacing -----------------------------------------

/// Context of a CLI action for adaptive pacing.
#[derive(Copy, Clone, Debug)]
#[allow(dead_code)] // keep ahead of compiler nags; future modes will use all variants
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
        // Optional global override
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

    /// Explicit duration when needed.
    #[allow(dead_code)]
    pub fn pause_secs(&self, secs: u64) {
        if !self.quiet_mode {
            thread::sleep(Duration::from_secs(secs));
        }
    }
}

// --------------------------- UX Facade --------------------------------------

/// Centralized UI facade for consistent look & feel.
pub struct UX {
    #[allow(unused)]
    pub verbose: bool,
    #[allow(unused)]
    pub quiet: bool,
    // Pre-baked styles for consistent visuals
    s_info: Style,
    s_warn: Style,
    s_ok: Style,
    s_err: Style,
    s_head: Style,
    s_tag: Style,
}

impl UX {
    pub fn new(verbose: bool, quiet: bool) -> Self {
        Self {
            verbose,
            quiet,
            s_info: Style::new().blue(),
            s_warn: Style::new().yellow(),
            s_ok: Style::new().green(),
            s_err: Style::new().red().bold(),
            s_head: Style::new().cyan().bold(),
            s_tag: Style::new().yellow(),
        }
    }

    /// Allow dynamic runtime toggling of verbosity
    #[allow(dead_code)]
    pub fn set_verbose(&mut self, enable: bool) {
        self.verbose = enable;
    }

    /// Future feature marker for debug tracing (silences unused warnings now)
    #[allow(dead_code)]
    pub fn trace(&self, msg: &str) {
        if self.verbose && !self.quiet {
            println!("{} {}", style("[TRACE]").dim(), msg);
        }
    }

    // ---------------------- Messaging ----------------------

    pub fn info(&self, msg: &str) {
        if self.quiet {
            return;
        }
        println!("{} {}", self.s_info.apply_to("[INFO]"), msg);
    }

    pub fn success(&self, msg: &str) {
        if self.quiet {
            return;
        }
        let check = Emoji("✅", "[OK]");
        println!("{} {}", check, self.s_ok.apply_to(msg));
    }

    pub fn warn(&self, msg: &str) {
        if self.quiet {
            return;
        }
        println!("{} {}", Emoji("⚠️", "[WARN]"), self.s_warn.apply_to(msg));
    }

    pub fn error(&self, msg: &str) {
        // Errors should be visible even in quiet mode
        eprintln!("{} {}", Emoji("❌", "[ERR]"), self.s_err.apply_to(msg));
    }
    #[allow(dead_code)]
    pub fn debug(&self, msg: &str) {
        if self.verbose && !self.quiet {
            println!("{} {}", style("[DEBUG]").dim(), msg);
        }
    }

    // ---------------------- Banner & Effects ----------------------

    /// Main banner with subtle “space-terminal” vibe.
    pub fn banner(&self) {
        if self.quiet {
            return;
        }

        let border = "══════════════════════════════════════════════════════════════════";
        println!(
            "\n{}\n{}\n{}\n{}\n",
            self.s_head.apply_to(format!("╔{}╗", border)),
            self.s_head
                .apply_to("║                ZFS  BESKAR  KEY  CONSOLE                         ║"),
            self.s_tag
                .apply_to("║             For the modern-day Bounty Hunter.                    ║"),
            self.s_head.apply_to(format!("╚{}╝", border)),
        );
        self.gradient_line(":: INITIALIZING SECURE INTERFACE ::");
    }

    /// Brief flicker effect to suggest a terminal powering up (skipped in quiet).
    pub fn banner_flicker(&self, timing: &Timing) -> Result<()> {
        if self.quiet {
            return Ok(());
        }
        let mut out = std::io::stdout();
        for _ in 0..3 {
            write!(
                out,
                "\r{}",
                style(":: INITIALIZING INTERFACE ::").green().dim()
            )?;
            out.flush()?;
            thread::sleep(Duration::from_millis(140));
            write!(out, "\r                             ")?; // clear line
            out.flush()?;
            thread::sleep(Duration::from_millis(110));
        }
        println!();
        timing.pace(Pace::Prompt);
        Ok(())
    }

    /// Subtle left-to-right color shift; restrained to avoid noise.
    pub fn gradient_line(&self, text: &str) {
        if self.quiet {
            return;
        }
        let segs = 3usize.max(text.len() / 3);
        let (c1, c2, c3) = (
            Style::new().cyan(),
            Style::new().green(),
            Style::new().yellow(),
        );
        let mut out = String::with_capacity(text.len());

        for (i, ch) in text.chars().enumerate() {
            let styled = if i % segs == 0 {
                c1.apply_to(ch.to_string()).to_string()
            } else if i % segs == 1 {
                c2.apply_to(ch.to_string()).to_string()
            } else {
                c3.apply_to(ch.to_string()).to_string()
            };
            out.push_str(&styled);
        }
        println!("{}", out);
    }
}
