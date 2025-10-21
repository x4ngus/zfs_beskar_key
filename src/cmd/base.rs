// ============================================================================
// src/cmd/base.rs – Allowlisted external command runner (for system utilities)
// ============================================================================

use anyhow::{anyhow, Context, Result};
use std::io::{Read, Write};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
            "/bin/systemd-analyze",
            "/usr/bin/systemd-analyze",
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
        let mut command = Command::new(&self.path);
        command.args(args);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        if input.is_some() {
            command.stdin(Stdio::piped());
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("spawn {}", self.path))?;

        if let Some(input_bytes) = input {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(input_bytes)?;
                stdin.flush().ok();
            }
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        self.wait_with_timeout(child, stdout, stderr)
    }

    fn wait_with_timeout(
        &self,
        mut child: Child,
        stdout_pipe: Option<ChildStdout>,
        stderr_pipe: Option<ChildStderr>,
    ) -> Result<OutputData> {
        let timeout = self.timeout;
        let start = Instant::now();
        let stdout_handle = Self::spawn_output_reader(stdout_pipe);
        let stderr_handle = Self::spawn_output_reader(stderr_pipe);
        let mut exit_status = None;
        let mut timed_out = false;

        loop {
            match child.try_wait()? {
                Some(status) => {
                    exit_status = Some(status);
                    break;
                }
                None => {
                    if start.elapsed() > timeout {
                        timed_out = true;
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }

        let stdout = stdout_handle
            .join()
            .map_err(|_| anyhow!("stdout reader thread panicked"))??;
        let stderr = stderr_handle
            .join()
            .map_err(|_| anyhow!("stderr reader thread panicked"))??;

        if timed_out {
            return Err(anyhow!("Command timed out after {:?}", timeout));
        }

        let status = exit_status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

        Ok(OutputData {
            stdout,
            stderr,
            status,
        })
    }

    fn spawn_output_reader<R>(pipe: Option<R>) -> thread::JoinHandle<Result<String>>
    where
        R: Read + Send + 'static,
    {
        thread::spawn(move || -> Result<String> {
            if let Some(mut reader) = pipe {
                let mut buf = Vec::new();
                reader
                    .read_to_end(&mut buf)
                    .context("read child process pipe")?;
                Ok(String::from_utf8_lossy(&buf).to_string())
            } else {
                Ok(String::new())
            }
        })
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
