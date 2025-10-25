// ============================================================================
// src/ui.rs – Mandalorian-inspired CLI experience with security-forward tone
// ============================================================================

use anyhow::Result;
use chrono::Local;
use console::Style;
use std::{
    env,
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

pub const BANNER_BODY_WIDTH: usize = 100;
const LOG_BODY_WIDTH: usize = BANNER_BODY_WIDTH;
const DEFAULT_CURSOR_DELAY_MS: u64 = 6;
const CYBER_FLICKER_PALETTE: [u8; 6] = [208, 214, 220, 178, 142, 202];
const CYBER_FLICKER_DELAY_MS: u64 = 14;

#[derive(Clone)]
struct Theme {
    info: Style,
    warn: Style,
    ok: Style,
    err: Style,
    accent: Style,
    muted: Style,
    security: Style,
    banner: Style,
    log_border: Style,
}

impl Theme {
    fn default() -> Self {
        Self {
            info: Style::new().color256(214).bold(),
            warn: Style::new().color256(208).bold(),
            ok: Style::new().color256(221).bold(),
            err: Style::new().color256(196).bold(),
            accent: Style::new().color256(178).bold(),
            muted: Style::new().color256(246),
            security: Style::new().color256(202).bold(),
            banner: Style::new().color256(202).bold(),
            log_border: Style::new().color256(208),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum LogLevel {
    Info,
    Success,
    Warn,
    Error,
    Security,
    Note,
    Trace,
    Phase,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Success => "SUCCESS",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Security => "SECURE",
            LogLevel::Note => "NOTE",
            LogLevel::Trace => "TRACE",
            LogLevel::Phase => "PHASE",
        }
    }

    fn style<'a>(self, theme: &'a Theme) -> &'a Style {
        match self {
            LogLevel::Info => &theme.info,
            LogLevel::Success => &theme.ok,
            LogLevel::Warn => &theme.warn,
            LogLevel::Error => &theme.err,
            LogLevel::Security => &theme.security,
            LogLevel::Note => &theme.muted,
            LogLevel::Trace => &theme.muted,
            LogLevel::Phase => &theme.accent,
        }
    }
}

// --------------------------- Pacing -----------------------------------------

/// Context of a CLI action for adaptive pacing.
#[derive(Copy, Clone, Debug)]
pub enum Pace {
    /// Major success, completion, or irreversible action — let it breathe.
    Critical,
    /// Standard informational update.
    Info,
    /// Waiting for user input or a quick transition.
    Prompt,
    /// Error message or invalid input — keep it snappy.
    Error,
}

/// Adaptive timing controller.
/// - Skips delays entirely in quiet mode
/// - Halves delays in verbose mode
pub struct Timing {
    base_delay: Duration,
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
        };

        let effective = if self.verbose_mode {
            duration / 2
        } else {
            duration
        };

        thread::sleep(effective);
    }
}

// --------------------------- UX Facade --------------------------------------

/// Centralized UX facade for Mandalorian-flavored messaging.
pub struct UX {
    pub verbose: bool,
    pub quiet: bool,
    theme: Theme,
    frame_drawn: AtomicBool,
    log_header_drawn: AtomicBool,
    app_version: &'static str,
    operator: String,
    cursor_delay: Duration,
}

impl UX {
    pub fn new(verbose: bool, quiet: bool) -> Self {
        let operator = env::var("USER")
            .or_else(|_| env::var("LOGNAME"))
            .unwrap_or_else(|_| "unknown-operator".to_string());
        let cursor_delay = env::var("BESKAR_CURSOR_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(DEFAULT_CURSOR_DELAY_MS));

        Self {
            verbose,
            quiet,
            theme: Theme::default(),
            frame_drawn: AtomicBool::new(false),
            log_header_drawn: AtomicBool::new(false),
            app_version: env!("CARGO_PKG_VERSION"),
            operator,
            cursor_delay,
        }
    }

    fn trim_to_width(text: &str, width: usize) -> String {
        let mut buffer = String::with_capacity(width);
        let mut count = 0;
        for ch in text.chars() {
            if count >= width {
                break;
            }
            buffer.push(ch);
            count += 1;
        }
        buffer
    }

    fn cyber_tint(&self, text: &str, shift: usize) -> String {
        text.chars()
            .enumerate()
            .map(|(idx, ch)| {
                if ch.is_whitespace() {
                    ch.to_string()
                } else {
                    let color = CYBER_FLICKER_PALETTE[(idx + shift) % CYBER_FLICKER_PALETTE.len()];
                    Style::new().color256(color).bold().apply_to(ch).to_string()
                }
            })
            .collect()
    }

    fn emit_line(&self, text: &str, slow: bool) {
        if self.quiet {
            return;
        }

        if slow {
            let mut out = io::stdout();
            for ch in text.chars() {
                let _ = write!(out, "{}", ch);
                let _ = out.flush();
                thread::sleep(self.cursor_delay);
            }
            let _ = writeln!(out);
            let _ = out.flush();
        } else {
            println!("{}", text);
        }
    }

    fn box_line(&self, content: &str, style: &Style) -> String {
        let trimmed = Self::trim_to_width(content, LOG_BODY_WIDTH);
        let padded = format!("║ {:<width$} ║", trimmed, width = LOG_BODY_WIDTH);
        style.apply_to(padded).to_string()
    }

    fn wrap_text(&self, text: &str, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current = String::new();

        for word in text.split_whitespace() {
            if current.is_empty() {
                if word.chars().count() <= width {
                    current.push_str(word);
                } else {
                    for chunk in word.chars().collect::<Vec<_>>().chunks(width) {
                        lines.push(chunk.iter().collect());
                    }
                }
            } else {
                let needed = current.chars().count() + 1 + word.chars().count();
                if needed <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    lines.push(current);
                    current = String::new();
                    if word.chars().count() <= width {
                        current.push_str(word);
                    } else {
                        let chars = word.chars().collect::<Vec<_>>();
                        let mut idx = 0;
                        while idx < chars.len() {
                            let chunk: String =
                                chars[idx..(idx + width).min(chars.len())].iter().collect();
                            if chunk.chars().count() == width && idx + width < chars.len() {
                                lines.push(chunk);
                            } else {
                                current = chunk;
                            }
                            idx += width;
                        }
                    }
                }
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }

        if lines.is_empty() {
            lines.push(String::new());
        }

        lines
    }

    fn ensure_log_header(&self) {
        if self.quiet {
            return;
        }

        if self.log_header_drawn.swap(true, Ordering::SeqCst) {
            return;
        }

        let span = "═".repeat(LOG_BODY_WIDTH + 2);
        let top = self
            .theme
            .log_border
            .apply_to(format!("╔{}╗", span))
            .to_string();
        self.emit_line(&top, false);

        let title = format!(
            "ARMORER'S ARCHIVE // v{} // Operative {}",
            self.app_version, self.operator
        );
        let title_line = self
            .theme
            .log_border
            .apply_to(format!(
                "║ {:<width$} ║",
                Self::trim_to_width(&title, LOG_BODY_WIDTH),
                width = LOG_BODY_WIDTH
            ))
            .to_string();
        self.emit_line(&title_line, false);

        let divider = self
            .theme
            .log_border
            .apply_to(format!("╠{}╣", span))
            .to_string();
        self.emit_line(&divider, false);

        self.log_line(
            LogLevel::Note,
            "Cadence: Temper ▸ Drill ▸ Diagnose ▸ Deploy.",
        );
        self.log_line(
            LogLevel::Note,
            "Armorer: Tribute received. State your need.",
        );
    }

    fn log_line(&self, level: LogLevel, message: &str) {
        if self.quiet {
            return;
        }

        self.ensure_log_header();

        if level == LogLevel::Trace && !self.verbose {
            return;
        }

        let timestamp = Local::now().format("%H:%M:%S");
        let mut payload = message.trim().to_string();
        if !matches!(level, LogLevel::Trace)
            && !payload.starts_with("Armorer")
            && !payload.starts_with("System")
            && !payload.starts_with("Operator")
        {
            payload = format!("Armorer: {}", payload);
        }
        let base = format!("[{} :: {}] {}", timestamp, level.label(), payload);
        for segment in self.wrap_text(&base, LOG_BODY_WIDTH) {
            let line = self.box_line(&segment, level.style(&self.theme));
            self.emit_line(&line, true);
        }
    }

    pub fn info(&self, msg: &str) {
        self.log_line(LogLevel::Info, msg);
    }

    pub fn success(&self, msg: &str) {
        self.log_line(LogLevel::Success, msg);
    }

    pub fn warn(&self, msg: &str) {
        self.log_line(LogLevel::Warn, msg);
    }

    pub fn error(&self, msg: &str) {
        self.log_line(LogLevel::Error, msg);
    }

    pub fn security(&self, msg: &str) {
        self.log_line(LogLevel::Security, msg);
    }

    pub fn note(&self, msg: &str) {
        self.log_line(LogLevel::Note, msg);
    }

    pub fn phase(&self, title: &str) {
        let normalized = title.trim().to_uppercase();
        self.divider();
        self.log_line(LogLevel::Phase, &format!("{} initialized", normalized));
        self.divider();
    }

    pub fn divider(&self) {
        if self.quiet {
            return;
        }
        self.ensure_log_header();
        let hatch = format!("╟{}╢", "─".repeat(LOG_BODY_WIDTH + 2));
        let styled = self.theme.log_border.apply_to(hatch).to_string();
        self.emit_line(&styled, false);
    }

    pub fn data_panel(&self, title: &str, rows: &[(&str, String)]) {
        if self.quiet {
            return;
        }
        self.ensure_log_header();
        let label_width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        let header_line = format!("{} manifest", title.to_uppercase());
        let header = self.box_line(&header_line, &self.theme.accent);
        self.emit_line(&header, true);
        for (idx, (label, value)) in rows.iter().enumerate() {
            let label_formatted = format!("{:>width$}", label, width = label_width);
            let payload = format!("{} ⇢ {}", label_formatted, value);
            let style = if idx % 2 == 0 {
                &self.theme.info
            } else {
                &self.theme.muted
            };
            let line = self.box_line(&payload, style);
            self.emit_line(&line, true);
        }
        self.divider();
    }

    fn render_banner_frame(&self) {
        if self.quiet {
            return;
        }

        let span = "═".repeat(BANNER_BODY_WIDTH + 2);
        let top = self
            .theme
            .banner
            .apply_to(format!("╔{}╗", span))
            .to_string();
        self.emit_line(&top, false);

        const CREST: [&str; 20] = [
            "⠀⠀⠀⠀⠀⠀⠀⠀⢀⣤⣶⡄⢠⣶⣶⣶⣶⣶⣶⣾⡆⠀⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⠀⢿⣿⣿⣄⠙⣿⣿⣿⣿⣿⣿⣿⠇⠀⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⠀⡈⢿⣿⣿⣴⣿⣿⣿⣿⡿⠿⠋⣰⡇⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⣼⡇⠀⠈⠙⢿⣿⣿⡿⠋⠀⠀⢀⣿⣧⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠰⣿⣧⡀⠀⠀⠸⣿⣯⠀⠀⢀⣠⣾⣿⡿⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⢿⣿⣿⣷⣤⡀⢻⣿⢠⣾⣿⣿⣿⠋⢀⣶⣦⡀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⢀⣤⣄⡈⠻⢿⣿⣿⣧⣼⣿⣾⣿⣿⣿⠏⢀⣿⣿⣿⣿⣦⡀⠀⠀⠀",
            "⠀⠀⠀⣴⣿⣿⣿⣿⣷⡄⠈⣿⣿⣿⣿⣿⣿⡟⠉⠀⠘⢿⣿⣿⣿⣿⣿⣄⠀⠀",
            "⠀⢀⣾⣿⣿⣿⣿⠿⠋⠀⠀⣿⡿⣿⣿⣿⢿⣷⠀⠀⠀⠀⠙⢿⣿⣿⣿⣿⣆⠀",
            "⢀⣾⣿⣿⣿⠟⠁⠀⠀⠀⠀⢸⡇⠈⣿⠁⢸⡿⠀⠀⠀⠀⠀⠀⠙⢿⣿⣿⣿⡆",
            "⢸⣿⣿⡿⠁⠀⠀⠀⠀⠀⠀⢸⣿⡄⢻⢀⣿⠇⠀⠀⠀⠀⠀⠀⠀⠈⢿⣿⣿⣷",
            "⣾⣿⣿⠁⠀⠀⠀⠀⠀⠀⠀⢸⣿⣧⠈⢸⣿⡀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⣿⡟",
            "⢹⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠈⠻⣿⡀⢸⣿⡇⠀⠀⠀⠀⠀⠀⠀⢀⣾⣿⡿⠃",
            "⠘⢿⣿⣷⣄⣀⡀⠀⠀⠀⠀⢰⣦⢹⡇⢸⡏⣴⠀⠀⠀⠀⠲⠶⠾⠿⠟⠋⠀⠀",
            "⠀⠈⠙⠛⠿⠿⠟⠋⠁⠀⠀⢸⡟⢸⡇⢸⡇⢹⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⢸⡇⢸⡇⢸⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⡇⢸⡇⢸⠇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣼⡇⢸⣇⠘⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⡇⢸⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
            "⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠃⠸⠟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
        ];
        let motif = ["╳", "╂", "╋", "╂"];
        let border_palette = [208, 214, 178, 202];
        let inner_width = BANNER_BODY_WIDTH;

        for (idx, line) in CREST.iter().enumerate() {
            let trimmed = Self::trim_to_width(line, inner_width);
            let padded = format!("{:^width$}", trimmed, width = inner_width);
            let tinted_body = self.cyber_tint(&padded, idx * 3);

            let color_idx = idx % border_palette.len();
            let color_right = (idx + 1) % border_palette.len();

            let left_edge = Style::new()
                .color256(border_palette[color_idx])
                .bold()
                .apply_to("║")
                .to_string();
            let right_edge = Style::new()
                .color256(border_palette[color_right])
                .bold()
                .apply_to("║")
                .to_string();

            let motif_left = Style::new()
                .color256(border_palette[color_idx])
                .bold()
                .apply_to(motif[idx % motif.len()])
                .to_string();
            let motif_right = Style::new()
                .color256(border_palette[color_right])
                .bold()
                .apply_to(motif[(idx + 2) % motif.len()])
                .to_string();

            let decorated = format!("{}{}{}", motif_left, tinted_body, motif_right);
            let entry = format!("{}{}{}", left_edge, decorated, right_edge);
            self.emit_line(&entry, false);
            thread::sleep(Duration::from_millis(CYBER_FLICKER_DELAY_MS));
        }

        let bottom = self
            .theme
            .banner
            .apply_to(format!("╚{}╝", span))
            .to_string();
        self.emit_line(&bottom, false);
    }

    pub fn banner(&self) {
        if self.quiet {
            return;
        }
        let first = !self.frame_drawn.swap(true, Ordering::SeqCst);
        if first {
            self.render_banner_frame();
        }
        self.ensure_log_header();
    }

    pub fn banner_flicker(&self, timing: &Timing) -> Result<()> {
        if self.quiet {
            return Ok(());
        }

        let mut out = io::stdout();
        let sequences = [
            "Banking forge vents so the beskar runs true …",
            "Retrieving covert schematics from the archive vault …",
            "Binding clan cipher seals across the plating …",
        ];

        for seq in sequences.iter() {
            let pulse = self
                .theme
                .accent
                .apply_to(format!("◉ {}   ", seq))
                .to_string();
            write!(out, "\r{}", pulse)?;
            out.flush()?;
            thread::sleep(Duration::from_millis(140));
            write!(out, "\r{}", " ".repeat(86))?;
            out.flush()?;
            thread::sleep(Duration::from_millis(90));
        }
        println!();
        timing.pace(Pace::Prompt);
        Ok(())
    }
}
