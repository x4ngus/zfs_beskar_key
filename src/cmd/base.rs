// ============================================================================
// src/cmd/base.rs – Allowlisted external command runner (for system utilities)
// ============================================================================

use anyhow::{anyhow, Context, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Safe wrapper for external process execution.
/// Used for invoking allowlisted system tools like `zfs`, `systemctl`, etc.
#[derive(Debug)]
pub struct Cmd {
    pub path: String,
    pub timeout: Duration,
}

#[derive(Debug)]
pub struct OutputData {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

impl Cmd {
    /// Create a new allowlisted command runner.
    pub fn new_allowlisted<S: Into<String>>(path: S, timeout: Duration) -> Result<Self> {
        let path_str = path.into();
        // Security measure: restrict to known binaries
        let allowed = [
            "/sbin/zfs",
            "/bin/systemctl",
            "/usr/sbin/zpool",
            "/usr/bin/dracut",
        ];
        if !allowed.contains(&path_str.as_str()) {
            return Err(anyhow!("Command '{}' not in allowlist", path_str));
        }

        Ok(Self {
            path: path_str,
            timeout,
        })
    }

    /// Run command with arguments, returning `OutputData`
    pub fn run(&self, args: &[&str], input: Option<&str>) -> Result<OutputData> {
        let mut cmd = Command::new(&self.path);
        cmd.args(args);

        if let Some(input_text) = input {
            cmd.stdin(std::process::Stdio::piped());
            let mut child = cmd
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .with_context(|| format!("spawn {}", self.path))?;

            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(input_text.as_bytes())?;
            }

            let output = self.wait_with_timeout(child)?;
            return Ok(output);
        }

        let child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn {}", self.path))?;

        let output = self.wait_with_timeout(child)?;
        Ok(output)
    }

    fn wait_with_timeout(&self, mut child: std::process::Child) -> Result<OutputData> {
        let timeout = self.timeout;
        let start = std::time::Instant::now();

        loop {
            match child.try_wait()? {
                Some(status) => {
                    let output = child.wait_with_output()?;
                    return Ok(OutputData {
                        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        status: status.code().unwrap_or(-1),
                    });
                }
                None => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        return Err(anyhow!("Command timed out after {:?}", timeout));
                    }
                    thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    }
}
