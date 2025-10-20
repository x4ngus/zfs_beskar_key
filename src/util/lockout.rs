// ============================================================================
// src/util/lockout.rs – Adaptive lockout / cooldown control
// ============================================================================

use std::thread;
use std::time::{Duration, Instant};

use crate::ui::{Pace, Timing, UX};
use crate::util::audit::audit_log;

/// Adaptive exponential back-off after repeated failures.
///
/// Each failed authentication or unlock attempt doubles the delay up to a
/// maximum, helping to slow brute-force attempts without punishing
/// occasional human mistakes.
#[derive(Debug)]
pub struct Lockout {
    start: Option<Instant>,
    delay: Duration,
    max_delay: Duration,
}

impl Lockout {
    /// Create a new lockout tracker with a base delay of 5 s and a max of 60 s.
    pub fn new() -> Self {
        Self {
            start: None,
            delay: Duration::from_secs(5),
            max_delay: Duration::from_secs(60),
        }
    }

    /// Register a failed attempt and increase cooldown time.
    pub fn register_failure(&mut self, ui: &UX, timing: &Timing) {
        self.start = Some(Instant::now());
        self.delay = std::cmp::min(self.delay * 2, self.max_delay);

        ui.warn(&format!(
            "Authentication failure. Next retry available in {:.1}s.",
            self.delay.as_secs_f32()
        ));
        audit_log(
            "LOCKOUT_FAIL",
            &format!("Delay escalated to {:?}", self.delay),
        );
        timing.pace(Pace::Error);
    }

    /// Wait out the cooldown if active, giving consistent UX feedback.
    pub fn wait_if_needed(&self, ui: &UX, timing: &Timing) {
        if let Some(start) = self.start {
            let elapsed = start.elapsed();
            if elapsed < self.delay {
                let remain = self.delay - elapsed;
                ui.info(&format!(
                    "Cooling down for {:.1}s before next attempt...",
                    remain.as_secs_f32()
                ));
                audit_log("LOCKOUT_WAIT", &format!("Cooling down for {:.1?}", remain));
                timing.pace(Pace::Error);
                thread::sleep(remain);
            }
        }
    }

    /// Reset lockout after a successful authentication.
    pub fn reset(&mut self, ui: &UX, timing: &Timing) {
        self.start = None;
        self.delay = Duration::from_secs(5);
        ui.success("Authentication successful. Lockout reset.");
        audit_log("LOCKOUT_RESET", "Delay reset to base interval (5 s)");
        timing.pace(Pace::Info);
    }
}
