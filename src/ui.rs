use colored::*;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    terminal::{Clear, ClearType},
    ExecutableCommand,
};
use std::io::{stdout, Write};
use std::thread;
use std::time::Duration;

/// Persistent Mandalorian UI — draws the blaster bar and thematic messages
pub struct BlasterUI {
    total_steps: usize,
    current_step: usize,
    width: usize,
}

impl BlasterUI {
    pub fn new(total_steps: usize) -> Self {
        let mut stdout = stdout();
        let _ = stdout.execute(Hide);
        Self {
            total_steps,
            current_step: 0,
            width: 50,
        }
    }

    pub fn section(&mut self, title: &str, subtitle: &str, color: &str) {
        let mut stdout = stdout();
        let banner = match color {
            "red" => format!("🔥 {}", title.bold().red()),
            "blue" => format!("💠 {}", title.bold().blue()),
            "yellow" => format!("⚙️  {}", title.bold().yellow()),
            _ => title.bold().to_string(),
        };

        stdout.execute(Clear(ClearType::FromCursorDown)).unwrap();
        println!("\n{}", banner);
        println!("{}", subtitle.dimmed());
        thread::sleep(Duration::from_secs(2));
    }

    /// Show a brief system log or step confirmation
    pub fn log(&self, msg: &str) {
        println!("{} {}", "▸".bright_black(), msg.white());
        stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(400));
    }

    /// Animate the blaster beam progress bar
    pub fn progress(&mut self, label: &str) {
        self.current_step += 1;
        let pct = ((self.current_step as f32 / self.total_steps as f32) * 100.0) as usize;
        let filled = (self.width * pct) / 100;
        let empty = self.width - filled;

        let beam = format!(
            "[{}{}▶] {:>3}%  {}",
            "=".repeat(filled).red(),
            "·".repeat(empty).bright_black(),
            pct,
            label
        );

        let mut stdout = stdout();
        stdout.execute(MoveTo(0, 30)).unwrap();
        print!("{}", beam.bold());
        stdout.flush().unwrap();
        thread::sleep(Duration::from_millis(800));
    }

    /// Prompt user with colorized input
    pub fn prompt(&self, question: &str) -> String {
        use std::io::{self, BufRead};

        print!(
            "\n{} {} {} ",
            "💬".cyan(),
            question.bold().white(),
            "[y/n]".dimmed()
        );
        stdout().flush().unwrap();

        let stdin = io::stdin();
        let mut answer = String::new();
        stdin.lock().read_line(&mut answer).unwrap();
        let trimmed = answer.trim().to_lowercase();

        if trimmed == "y" {
            println!("{}", "This is the way.".italic().blue());
        } else {
            println!("{}", "As you wish, bounty hunter.".italic().red());
        }

        trimmed
    }

    /// Show final completion message
    pub fn complete(&self) {
        let mut stdout = stdout();
        stdout.execute(MoveTo(0, 32)).unwrap();
        println!(
            "{}",
            "[██████████████████████████████████████████████████] 100%  Forge Complete"
                .bold()
                .green()
        );
        println!(
            "\n{}\n",
            "\"It is with these scraps of beskar that I forged your next piece of armor; \
             Mandalorian steel shall keep you safe as you grow stronger.\""
                .italic()
                .white()
        );
        let _ = stdout.execute(Show);
    }
}
