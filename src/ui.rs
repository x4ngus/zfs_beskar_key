use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::{thread, time::Duration};

static mut BAR: Option<ProgressBar> = None;

pub fn banner(title: &str) -> Result<()> {
    println!("{}", title.bold().bright_white());
    println!("{}", "──────────────────────────────────────────────────────────".bright_red());
    init_bar()?;
    Ok(())
}

fn init_bar() -> Result<()> {
    let bar = ProgressBar::new(100);
    bar.set_style(
        ProgressStyle::with_template("{prefix:.bold} {bar:40.red} {pos:>3}%  {msg}")?
            .with_key("bar", |s| {
                let filled = (s.pos() * 40 / s.length().unwrap_or(100)) as usize;
                let bar = format!(
                    "{}{}{}",
                    "[=▣]".red(),
                    "━".repeat(filled).red(),
                    "·".repeat(40usize.saturating_sub(filled)).bright_black()
                );
                bar
            }),
    );
    bar.set_prefix("");
    bar.set_position(0);
    unsafe { BAR = Some(bar); }
    Ok(())
}

pub fn blaster_step(pct: u64, msg: &str) -> Result<()> {
    unsafe {
        if let Some(bar) = &BAR {
            bar.set_message(msg.to_string());
            bar.set_position(pct.min(100));
            // little “pew” pulse
            thread::sleep(Duration::from_millis(200));
        }
    }
    println!("\n{}", format!("➡️  {}", msg).bright_yellow().bold());
    Ok(())
}

pub fn pause(secs: u64) {
    thread::sleep(Duration::from_secs(secs));
}

pub fn this_is_the_way() {
    println!("{}", "This is the way...".italic().bright_cyan());
}

pub fn forge_cools_and_exit() -> ! {
    eprintln!("{}", "Operation reversed. The forge cools — no beskar was shaped today.".bright_red());
    std::process::exit(1);
}

pub fn success(msg: &str) {
    println!("{}", format!("✅ {}", msg).bright_green().bold());
}

pub fn warn(msg: &str) {
    println!("{}", format!("⚠️  {}", msg).bright_yellow());
}

pub fn info(msg: &str) {
    println!("{}", msg.bright_white());
}

pub fn armorer_quote() {
    println!();
    println!("{}", "“It is with these scraps of beskar that I forged your next piece of armor;".bright_cyan().italic());
    println!("{}", "  Mandalorian steel shall keep you safe as you grow stronger.”".bright_cyan().italic());
    println!();
    println!("{}", "                     — The Armorer".bright_white());
    println!("{}", "                         This is the way.".bright_red().bold());
}
