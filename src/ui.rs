use anyhow::Result;
use crossterm::{
    cursor::{MoveTo, Show},
    execute,
    style::{Color, Print, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::{stdout, Write};
use std::{thread, time::Duration};

const BAR_W: u16 = 42;

fn draw_bar(pct: u64, msg: &str) -> Result<()> {
    let mut out = stdout();
    let (_cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let row = rows.saturating_sub(2);

    // Build bar
    let filled = ((pct.min(100) as u16) * BAR_W / 100) as usize;
    let empty = (BAR_W as usize).saturating_sub(filled);
    let head = "[=▣]".to_string();
    let beam = "━".repeat(filled);
    let space = "·".repeat(empty);

    // Clear bottom lines
    execute!(out, MoveTo(0, row), Clear(ClearType::CurrentLine))?;
    execute!(
        out,
        SetForegroundColor(Color::Red),
        Print(format!("{head}{beam}{space} ▶ {:>3}%  ", pct.min(100)))
    )?;
    execute!(
        out,
        SetForegroundColor(Color::White),
        Print(msg.to_string())
    )?;

    // One line above for step label area (kept clean)
    execute!(
        out,
        MoveTo(0, row.saturating_sub(1)),
        Clear(ClearType::CurrentLine)
    )?;
    out.flush()?;
    Ok(())
}

pub fn banner(title: &str) -> Result<()> {
    let mut out = stdout();
    execute!(out, Clear(ClearType::All), MoveTo(0, 0))?;
    execute!(
        out,
        SetForegroundColor(Color::White),
        Print(format!("{title}\n"))
    )?;
    execute!(
        out,
        SetForegroundColor(Color::Red),
        Print("──────────────────────────────────────────────────────────\n")
    )?;
    Ok(())
}

pub fn progress(pct: u64, msg: &str) -> Result<()> {
    draw_bar(pct, msg)?;
    thread::sleep(Duration::from_millis(200)); // subtle "pew"
    Ok(())
}

pub fn step(msg: &str) {
    let mut out = stdout();
    let _ = execute!(
        out,
        SetForegroundColor(Color::Yellow),
        Print(format!("\n➡️  {msg}\n"))
    );
}

pub fn pause(secs: u64) {
    thread::sleep(Duration::from_secs(secs));
}

pub fn done(msg: &str) {
    let mut out = stdout();
    let _ = execute!(
        out,
        SetForegroundColor(Color::Green),
        Print(format!("\n✅ {msg}\n"))
    );
    let _ = execute!(out, Show);
}

pub fn quote() {
    let mut out = stdout();
    let _ = execute!(
        out,
        SetForegroundColor(Color::Cyan),
        Print("\n“It is with these scraps of beskar that I forged your next piece of armor;\n"),
        Print("  Mandalorian steel shall keep you safe as you grow stronger.”\n"),
        SetForegroundColor(Color::White),
        Print("\n                     — The Armorer\n"),
        SetForegroundColor(Color::Red),
        Print("                         This is the way.\n")
    );
}
