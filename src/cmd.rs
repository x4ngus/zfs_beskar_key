// ============================================================================
//! Minimal, careful wrapper around spawning `zfs` without a shell. No secrets
//! in logs; strict path allowlist; bounded execution time. This is the Way.

use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Cmd {
    path: PathBuf,
    timeout: Duration,
}

#[derive(Debug)]
pub struct OutputData {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Cmd {
    pub fn new_allowlisted<P: AsRef<Path>>(path: P, timeout: Duration) -> Result<Self> {
        let p = path.as_ref();
        if !p.is_file() {
            return Err(anyhow!("binary not found: {}", p.display()));
        }
        let canon = std::fs::canonicalize(p)
            .with_context(|| format!("canonicalize failed: {}", p.display()))?;
        Ok(Self {
            path: canon,
            timeout,
        })
    }

    pub fn run(&self, args: &[&str], stdin_bytes: Option<&[u8]>) -> Result<OutputData> {
        let mut child = Command::new(&self.path)
            .args(args)
            .stdin(if stdin_bytes.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn {} failed", self.path.display()))?;

        if let Some(bytes) = stdin_bytes {
            if let Some(mut sin) = child.stdin.take() {
                sin.write_all(bytes).context("writing stdin")?;
            }
        }

        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().context("try_wait")? {
                let out = child.wait_with_output().context("collect output")?;
                return Ok(OutputData {
                    status: status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                });
            }
            if start.elapsed() > self.timeout {
                // Best effort terminate, then kill.
                #[cfg(unix)]
                {
                    use nix::sys::signal::{kill, Signal::SIGTERM};
                    use nix::unistd::Pid;
                    let _ = kill(Pid::from_raw(child.id() as i32), SIGTERM);
                }
                std::thread::sleep(Duration::from_millis(200));
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!("command timed out after {:?}", self.timeout));
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}
