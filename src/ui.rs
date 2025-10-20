// ============================================================================
// src/config.rs – UI Module
// ============================================================================
//!
//! Progress messages are "forged" not "forced".
//! This is The Way (of good error handling).
//!
//! Key goals
//! - Works in TTY, JSON‑logging, and Quiet modes
//! - No panics on malformed terminals; graceful degradation
//! - Thread‑safe enough for simple multi‑threaded status updates
//! - Zero cost when silenced (quiet mode)

use anyhow::{anyhow, Result};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UIMode {
    /// Interactive terminal with a progress bar.
    Tty,
    /// Structured logs for automation: one JSON object per line.
    Json,
    /// No output except explicit errors.
    Quiet,
}

impl UIMode {
    fn from_env() -> Self {
        match std::env::var("BESKAR_UI")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "json" => UIMode::Json,
            "quiet" | "silent" | "off" => UIMode::Quiet,
            _ => {
                if atty::is(atty::Stream::Stderr) || atty::is(atty::Stream::Stdout) {
                    UIMode::Tty
                } else {
                    UIMode::Json
                }
            }
        }
    }
}

/// UI writes to stderr by default so stdout can stay clean for machine output.
pub struct UI {
    mode: UIMode,
    inner: Arc<Mutex<Inner>>, // serialize writes across threads
}

struct Inner {
    started: Instant,
    last_flush: Instant,
    current_pct: u8,
    current_msg: String,
}

impl Default for UI {
    fn default() -> Self {
        Self::new(UIMode::from_env())
    }
}

impl UI {
    /// Construct from explicit mode.
    pub fn new(mode: UIMode) -> Self {
        let now = Instant::now();
        UI {
            mode,
            inner: Arc::new(Mutex::new(Inner {
                started: now,
                last_flush: now,
                current_pct: 0,
                current_msg: String::new(),
            })),
        }
    }

    /// Construct from environment (BESKAR_UI = tty|json|quiet). Defaults smartly.
    pub fn from_env() -> Self {
        Self::default()
    }

    /// Primary progress reporter.
    pub fn blaster(&self, percent: u8, msg: &str) -> Result<()> {
        let mut g = self.inner.lock().map_err(|_| anyhow!("ui poisoned"))?;
        let pct = percent.min(100);
        g.current_pct = pct;
        g.current_msg.clear();
        g.current_msg.push_str(msg);

        match self.mode {
            UIMode::Quiet => Ok(()),
            UIMode::Json => self.emit_json_locked(&mut g, None),
            UIMode::Tty => self.render_tty_locked(&mut g),
        }
    }

    /// Human-readable info line.
    pub fn info(&self, msg: &str) -> Result<()> {
        self.emit_event("info", msg, None)
    }

    /// Warning line that still allows continuation.
    pub fn warn(&self, msg: &str) -> Result<()> {
        self.emit_event("warn", msg, None)
    }

    /// Fatal error line (does not exit; caller decides control flow).
    pub fn error(&self, msg: &str) -> Result<()> {
        self.emit_event("error", msg, None)
    }

    /// Mark the end of forging.
    pub fn finish(&self, msg: &str) -> Result<()> {
        let mut g = self.inner.lock().map_err(|_| anyhow!("ui poisoned"))?;
        g.current_pct = 100;
        g.current_msg = msg.to_owned();
        match self.mode {
            UIMode::Quiet => Ok(()),
            UIMode::Json => self.emit_json_locked(&mut g, Some("finish")),
            UIMode::Tty => {
                self.render_tty_locked(&mut g)?;
                eprintln!();
                Ok(())
            }
        }
    }

    /// Emit a heartbeat in long operations so logs don’t go silent.
    pub fn heartbeat(&self, _label: &str, interval: Duration) -> Result<()> {
        let mut g = self.inner.lock().map_err(|_| anyhow!("ui poisoned"))?;
        if g.last_flush.elapsed() >= interval {
            match self.mode {
                UIMode::Quiet => {}
                UIMode::Json => {
                    self.emit_json_locked(&mut g, Some("heartbeat"))?;
                }
                UIMode::Tty => {
                    // no extra TTY noise; re-draw once per interval
                    self.render_tty_locked(&mut g)?;
                }
            }
        }
        Ok(())
    }

    fn emit_event(&self, level: &str, msg: &str, extra: Option<&str>) -> Result<()> {
        let mut g = self.inner.lock().map_err(|_| anyhow!("ui poisoned"))?;
        match self.mode {
            UIMode::Quiet => Ok(()),
            UIMode::Json => {
                let obj = serde_json::json!({
                    "ts_ms": now_ms(),
                    "level": level,
                    "event": extra.unwrap_or(""),
                    "msg": msg,
                });
                writeln!(io::stderr(), "{}", obj).map_err(|e| anyhow!(e))?;
                g.last_flush = Instant::now();
                Ok(())
            }
            UIMode::Tty => {
                match level {
                    "warn" => eprintln!("⚠️  {}", msg),
                    "error" => eprintln!("❌ {}", msg),
                    _ => eprintln!("{}", msg),
                }
                g.last_flush = Instant::now();
                Ok(())
            }
        }
    }

    fn render_tty_locked(&self, g: &mut Inner) -> Result<()> {
        // Render a one-line progress bar. Keep dependencies minimal and avoid panics.
        let width = terminal_width().unwrap_or(80).max(40);
        let pct = g.current_pct as usize;
        let bar_w = (width.saturating_sub(12)).min(60); // room for brackets & pct
        let filled = bar_w * pct / 100;

        let mut line = String::with_capacity(width);
        line.push('[');
        line.push_str(&"#".repeat(filled));
        line.push_str(&"-".repeat(bar_w - filled));
        line.push(']');
        line.push(' ');
        line.push_str(&format!("{:>3}% ", pct));

        // Trim/ellipsize message to fit
        let mut msg = g.current_msg.clone();
        let remaining = width.saturating_sub(line.len());
        if msg.len() > remaining {
            if remaining > 1 {
                msg.truncate(remaining.saturating_sub(1));
                msg.push('…');
            } else {
                msg.clear();
            }
        }
        line.push_str(&msg);

        // Carriage-return update without newline
        write!(io::stderr(), "\r{}", line).map_err(|e| anyhow!(e))?;
        io::stderr().flush().map_err(|e| anyhow!(e))?;
        g.last_flush = Instant::now();
        Ok(())
    }

    fn emit_json_locked(&self, g: &mut Inner, event: Option<&str>) -> Result<()> {
        let obj = serde_json::json!({
            "ts_ms": now_ms(),
            "event": event.unwrap_or("progress"),
            "percent": g.current_pct,
            "msg": g.current_msg,
            "uptime_ms": g.started.elapsed().as_millis() as u64,
        });
        writeln!(io::stderr(), "{}", obj).map_err(|e| anyhow!(e))?;
        g.last_flush = Instant::now();
        Ok(())
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn terminal_width() -> Option<usize> {
    #[cfg(unix)]
    {
        use libc::{ioctl, winsize, STDOUT_FILENO, TIOCGWINSZ};
        unsafe {
            let mut ws: winsize = std::mem::zeroed();
            if ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
                return Some(ws.ws_col as usize);
            }
        }
    }
    None
}
