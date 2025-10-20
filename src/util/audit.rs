// ============================================================================
// src/util/audit.rs – Minimal append-only audit trail
// ============================================================================

use chrono::Local;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

/// Append a timestamped event to /var/log/beskar.log (0600 permissions).
/// Silent failure if log is unwritable – avoids blocking main logic.
pub fn audit_log(event: &str, detail: &str) {
    let path = "/var/log/beskar.log";
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    {
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{ts}] {event}: {detail}");
    }
}
