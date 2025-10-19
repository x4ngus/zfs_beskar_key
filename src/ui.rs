use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    style::{Color, Stylize},
    terminal::{size, Clear, ClearType},
    ExecutableCommand,
};
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

/// Unified cinematic UI controller (Mandalorian forge theme).
pub struct ForgeUI {
    width: u16,
    height: u16,
    progress: u8,
}

impl ForgeUI {
    pub fn new() -> Result<Self> {
        let (w, h) = size()?;
        io::stdout().execute(Hide)?;
        Ok(Self {
            width: w,
            height: h.max(3),
            progress: 0,
        })
    }

    /// Title banner.
    pub fn banner(&mut self, title: &str) -> Result<()> {
        let mut out = io::stdout();
        out.execute(Clear(ClearType::All))?;
        let center = (self.width.saturating_sub(title.len() as u16 + 4)) / 2;
        out.execute(MoveTo(center, 1))?;
        println!("{}", format!("💠 {title} 💠").bold().cyan());
        out.flush()?;
        Ok(())
    }

    /// Major step (bold, prominent).
    pub fn step(&self, msg: &str) -> Result<()> {
        println!("\n{}", format!("▸ {msg}").bold().white());
        Ok(())
    }

    /// Sub-step (indented, subtle).
    pub fn substep(&self, msg: &str) -> Result<()> {
        println!(
            "{} {}",
            "    ↳".with(Color::DarkGrey),
            msg.with(Color::DarkCyan)
        );
        Ok(())
    }

    /// Bottom-anchored blaster progress bar.
    pub fn blaster(&mut self, percent: u8, msg: &str) -> Result<()> {
        self.progress = percent.min(100);
        let mut out = io::stdout();
        let width_units = 50usize;
        let filled = (self.progress as usize * width_units) / 100;
        let empty = width_units - filled;

        let beam = "━".repeat(filled).with(Color::Red);
        let rest = "·".repeat(empty).with(Color::DarkGrey);

        let y = self.height.saturating_sub(2);
        out.execute(MoveTo(0, y))?;
        out.execute(Clear(ClearType::CurrentLine))?;
        write!(
            out,
            "[{}{}] {:>3}%  {}",
            beam,
            rest,
            self.progress,
            msg.bold().white()
        )?;
        out.flush()?;
        Ok(())
    }

    /// Quick “muzzle flash” pulse effect (cosmetic).
    pub fn blast(&self, duration_ms: u64) -> Result<()> {
        let start = Instant::now();
        let mut out = io::stdout();
        while start.elapsed().as_millis() < duration_ms as u128 {
            let phase = (start.elapsed().as_millis() % 400) as u16;
            let color = if phase < 200 {
                Color::Red
            } else {
                Color::DarkRed
            };
            let y = self.height.saturating_sub(1);
            out.execute(MoveTo(0, y))?;
            out.execute(Clear(ClearType::CurrentLine))?;
            write!(out, "{}", "⚡".with(color))?;
            out.flush()?;
            thread::sleep(Duration::from_millis(60));
        }
        // clear flash line
        let y = self.height.saturating_sub(1);
        out.execute(MoveTo(0, y))?;
        out.execute(Clear(ClearType::CurrentLine))?;
        Ok(())
    }

    /// Small delay for pacing.
    pub fn pause(&self, seconds: u64) {
        thread::sleep(Duration::from_secs(seconds));
    }

    /// Completion tick.
    pub fn done(&self, msg: &str) -> Result<()> {
        println!("\n{}", format!("✅ {msg}").bold().green());
        Ok(())
    }

    /// Mandalorian outro quote.
    pub fn quote(&self) -> Result<()> {
        println!(
            "\n{}\n",
            "\"It is with these scraps of beskar that I forged your next piece of armor; \
             Mandalorian steel shall keep you safe as you grow stronger.\""
                .italic()
                .white()
        );
        Ok(())
    }

    /// Restore terminal state.
    pub fn close(&self) -> Result<()> {
        io::stdout().execute(Show)?;
        Ok(())
    }
}
