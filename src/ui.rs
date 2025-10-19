use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    style::{Color, Stylize},
    terminal::{size, Clear, ClearType},
    ExecutableCommand,
};
use std::{
    io::{self, Write},
    thread,
    time::{Duration, Instant},
};

/// Struct to handle unified, cinematic UI control.
pub struct ForgeUI {
    width: u16,
    height: u16,
    progress: u8,
}

impl ForgeUI {
    pub fn new() -> Result<Self> {
        let (width, height) = size()?;
        io::stdout().execute(Hide)?;
        Ok(Self {
            width,
            height,
            progress: 0,
        })
    }

    /// Draws a centered Mandalorian banner.
    pub fn banner(&mut self, title: &str) -> Result<()> {
        let mut stdout = io::stdout();
        stdout.execute(Clear(ClearType::All))?;
        let center = (self.width.saturating_sub(title.len() as u16)) / 2;
        stdout.execute(MoveTo(center, 2))?;
        println!("{}", format!("💠 {title} 💠").bold().cyan());
        stdout.flush()?;
        Ok(())
    }

    /// Prints a forge narrative step line.
    pub fn step(&mut self, msg: &str) -> Result<()> {
        println!("\n{}\n", format!("▸ {msg}").bold().white());
        Ok(())
    }

    /// Updates the bottom-anchored blaster progress bar.
    pub fn blaster(&mut self, percent: u8, msg: &str) -> Result<()> {
        self.progress = percent;
        let mut stdout = io::stdout();
        let bar_y = self.height.saturating_sub(2);
        let filled = (percent as usize * 50) / 100;
        let empty = 50 - filled;
        let beam = "━".repeat(filled).with(Color::Red);
        let rest = "·".repeat(empty).dark_grey();

        stdout.execute(MoveTo(0, bar_y))?;
        stdout.execute(Clear(ClearType::CurrentLine))?;
        write!(
            stdout,
            "[{}{}] {:>3}%  {}",
            beam,
            rest,
            percent,
            msg.bold().white()
        )?;
        stdout.flush()?;
        Ok(())
    }

    /// Emits a blaster-rifle pulse animation.
    pub fn blast(&mut self, duration_ms: u64) -> Result<()> {
        let start = Instant::now();
        while start.elapsed().as_millis() < duration_ms as u128 {
            let phase = (start.elapsed().as_millis() % 400) as u16;
            let color = if phase < 200 {
                Color::Red
            } else {
                Color::DarkRed
            };
            let mut stdout = io::stdout();
            stdout.execute(MoveTo(0, self.height.saturating_sub(1)))?;
            stdout.execute(Clear(ClearType::CurrentLine))?;
            write!(stdout, "{}", "⚡".with(color))?;
            stdout.flush()?;
            thread::sleep(Duration::from_millis(60));
        }
        Ok(())
    }

    /// Pause to let the user absorb the step visually.
    pub fn pause(&self, seconds: u64) {
        thread::sleep(Duration::from_secs(seconds));
    }

    /// Displays a green completion message.
    pub fn done(&self, msg: &str) -> Result<()> {
        println!("\n{}\n", format!("✅ {msg}").bold().green());
        Ok(())
    }

    /// Epic Mandalorian outro quote.
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

    /// Cleanly restore cursor and terminal state.
    pub fn close(&self) -> Result<()> {
        io::stdout().execute(Show)?;
        Ok(())
    }
}
