use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{Result, WattWardenError};

pub struct PidManager {
    pid_path: PathBuf,
}

impl PidManager {
    pub fn new() -> Self {
        let path = if Path::new("/var/run").exists() {
            PathBuf::from("/var/run/wattwarden.pid")
        } else {
            PathBuf::from("/tmp/wattwarden.pid")
        };
        Self { pid_path: path }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { pid_path: path }
    }

    pub fn is_running(&self) -> bool {
        if let Ok(content) = fs::read_to_string(&self.pid_path) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if pid > 0 {
                    let proc_path = format!("/proc/{}", pid);
                    return Path::new(&proc_path).exists();
                }
            }
        }
        false
    }

    pub fn read_pid(&self) -> Option<i32> {
        fs::read_to_string(&self.pid_path)
            .ok()?
            .trim()
            .parse::<i32>()
            .ok()
    }

    pub fn acquire(&self) -> Result<()> {
        if self.is_running() {
            return Err(WattWardenError::Config(format!(
                "WattWarden daemon is already active with PID {}",
                self.read_pid().unwrap_or(0)
            )));
        }

        let pid = std::process::id();
        fs::write(&self.pid_path, pid.to_string()).map_err(|e| WattWardenError::Io {
            path: self.pid_path.clone(),
            source: e,
        })?;
        Ok(())
    }

    pub fn release(&self) {
        let _ = fs::remove_file(&self.pid_path);
    }
}

impl Default for PidManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PidManager {
    fn drop(&mut self) {
        // Only release if the PID file contains our current process ID
        if let Some(pid) = self.read_pid() {
            if pid as u32 == std::process::id() {
                self.release();
            }
        }
    }
}
