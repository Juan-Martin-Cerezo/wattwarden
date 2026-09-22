use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{Result, WattWardenError};

/// Go `service.IsProcessAlive`: does the PID belong to a live process?
///
/// Each platform is asked the way it can answer: procfs on Linux, signal 0 on the
/// other Unices (macOS has no `/proc` — assuming it there made the daemon look dead
/// on a machine where it was running) and `OpenProcess` on Windows, which is what
/// Go's `os.FindProcess` rejects a stale PID with.
#[cfg(target_os = "linux")]
fn process_alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Signal 0 delivers nothing and only reports whether the PID exists
/// (`process.Signal(syscall.Signal(0))` in Go).
#[cfg(all(unix, not(target_os = "linux")))]
fn process_alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

#[cfg(windows)]
fn process_alive(pid: i32) -> bool {
    // PROCESS_QUERY_LIMITED_INFORMATION: the least access that still opens a process
    // we may not own.
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn CloseHandle(object: *mut std::ffi::c_void) -> i32;
    }

    // SAFETY: both calls take plain scalars, the returned handle is only tested for
    // null and then closed exactly once — it is never dereferenced.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
        if handle.is_null() {
            return false;
        }
        CloseHandle(handle);
        true
    }
}

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
        match self.read_pid() {
            Some(pid) if pid > 0 => process_alive(pid),
            _ => false,
        }
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

        if let Some(parent) = self.pid_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let pid = std::process::id();
        fs::write(&self.pid_path, pid.to_string()).map_err(|e| WattWardenError::Io {
            path: self.pid_path.clone(),
            source: e,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.pid_path, fs::Permissions::from_mode(0o644));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_manager_lifecycle() {
        let tmp_path = std::env::temp_dir().join(format!("ww_pid_test_{}.pid", std::process::id()));
        let mgr = PidManager::with_path(tmp_path.clone());

        assert!(!mgr.is_running());
        assert_eq!(mgr.read_pid(), None);

        // Acquire
        mgr.acquire().expect("should acquire PID lock");
        assert!(mgr.is_running());
        assert_eq!(mgr.read_pid(), Some(std::process::id() as i32));

        // Cannot acquire again while running
        let second_mgr = PidManager::with_path(tmp_path.clone());
        assert!(second_mgr.acquire().is_err());

        // Release
        mgr.release();
        assert!(!mgr.is_running());
        assert_eq!(mgr.read_pid(), None);

        let _ = fs::remove_file(tmp_path);
    }
}
