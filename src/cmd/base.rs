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
            // zfs binary locations (must stay in sync with zfs::Zfs::discover)
            "/sbin/zfs",
            "/usr/sbin/zfs",
            "/usr/local/sbin/zfs",
            "/bin/zfs",
            // systemctl is typically in /bin or /usr/bin
            "/bin/systemctl",
            "/usr/bin/systemctl",
            // zpool helper binaries in common locations
            "/sbin/zpool",
            "/usr/sbin/zpool",
            "/usr/local/sbin/zpool",
            // dracut (optional bootstrap step)
            "/usr/bin/dracut",
            "/usr/sbin/dracut",
            // block device provisioning utilities for init workflow
            "/sbin/parted",
            "/usr/sbin/parted",
            "/usr/bin/parted",
            "/sbin/mkfs.ext4",
            "/usr/sbin/mkfs.ext4",
            "/usr/bin/mkfs.ext4",
            "/sbin/blkid",
            "/usr/sbin/blkid",
            "/usr/bin/blkid",
            "/bin/mount",
            "/usr/bin/mount",
            "/bin/umount",
            "/usr/bin/umount",
            "/bin/lsblk",
            "/usr/bin/lsblk",
            "/sbin/udevadm",
            "/usr/sbin/udevadm",
            "/usr/bin/udevadm",
            "/bin/systemd-ask-password",
            "/usr/bin/systemd-ask-password",
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
    pub fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<OutputData> {
        let mut cmd = Command::new(&self.path);
        cmd.args(args);

        if let Some(input_bytes) = input {
            cmd.stdin(std::process::Stdio::piped());
            let mut child = cmd
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .with_context(|| format!("spawn {}", self.path))?;

            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(input_bytes)?;
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

#[cfg(test)]
mod tests {
    use super::Cmd;
    use std::time::Duration;

    #[test]
    fn zfs_discover_paths_are_allowlisted() {
        let zfs_paths = [
            "/sbin/zfs",
            "/usr/sbin/zfs",
            "/usr/local/sbin/zfs",
            "/bin/zfs",
        ];

        for path in zfs_paths {
            assert!(
                Cmd::new_allowlisted(path, Duration::from_secs(1)).is_ok(),
                "expected {path} to be allowlisted"
            );
        }
    }
}
