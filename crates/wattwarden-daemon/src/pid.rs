//! Single-instance lock for the daemon.
//!
//! The pid file doubles as an `flock(2)` anchor: the kernel frees the lock the
//! instant the holding process dies — clean exit, SIGKILL or zombie alike — so
//! a stale file can never wedge the service. (The old PID-probe counted
//! zombies as alive and kept the systemd unit crash-looping on
//! "already active with PID <defunct>".) The PID written inside is advisory:
//! used for signalling and display only, never for liveness.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use wattwarden_core::{Result, WattWardenError};

/// Guard owning the instance lock; dropping it releases the lock.
#[cfg(unix)]
type LockGuard = nix::fcntl::Flock<File>;
#[cfg(not(unix))]
type LockGuard = File;

/// Tries to take the exclusive, non-blocking lock. `Ok(guard)` means this
/// process now holds it (released when the guard drops); `Err` means a live
/// process does.
#[cfg(unix)]
fn try_lock(file: File) -> std::io::Result<LockGuard> {
    nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock)
        .map_err(|(_, e)| std::io::Error::from_raw_os_error(e as i32))
}

#[cfg(not(unix))]
fn try_lock(file: File) -> std::io::Result<LockGuard> {
    Ok(file)
}

pub struct PidManager {
    pid_path: PathBuf,
    /// Held for as long as this process owns the instance lock.
    lock: Option<LockGuard>,
}

impl PidManager {
    pub fn new() -> Self {
        let path = if Path::new("/var/run").exists() {
            PathBuf::from("/var/run/wattwarden.pid")
        } else {
            PathBuf::from("/tmp/wattwarden.pid")
        };
        Self {
            pid_path: path,
            lock: None,
        }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self {
            pid_path: path,
            lock: None,
        }
    }

    /// Opens (creating if needed) the lock anchor.
    fn open_anchor(&self) -> std::io::Result<File> {
        if let Some(parent) = self.pid_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.pid_path)
    }

    /// True when a live daemon instance holds the lock. Crash remnants, stale
    /// files and zombies all report `false` — the kernel knows better than a
    /// PID probe. The probe guard drops (and so unlocks) immediately.
    pub fn is_running(&self) -> bool {
        match self.open_anchor().map(try_lock) {
            Ok(Ok(_guard)) => false,
            Ok(Err(_)) => true,
            Err(_) => false,
        }
    }

    /// Advisory: the PID recorded at acquire time (not a liveness check).
    pub fn read_pid(&self) -> Option<i32> {
        fs::read_to_string(&self.pid_path)
            .ok()?
            .trim()
            .parse::<i32>()
            .ok()
    }

    /// Takes the instance lock and records this process' PID in it.
    pub fn acquire(&mut self) -> Result<()> {
        let file = self.open_anchor().map_err(|e| WattWardenError::Io {
            path: self.pid_path.clone(),
            source: e,
        })?;
        let mut guard = try_lock(file).map_err(|_| {
            WattWardenError::Config(format!(
                "WattWarden daemon is already active with PID {}",
                self.read_pid().unwrap_or(0)
            ))
        })?;
        guard.set_len(0).map_err(|e| WattWardenError::Io {
            path: self.pid_path.clone(),
            source: e,
        })?;
        let _ = write!(guard, "{}", std::process::id());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.pid_path, fs::Permissions::from_mode(0o644));
        }
        self.lock = Some(guard);
        Ok(())
    }

    /// Drops the instance lock. The anchor file is left in place on purpose:
    /// unlinking it would let two processes lock different inodes of the
    /// "same" path and run side by side.
    pub fn release(&mut self) {
        self.lock.take();
    }
}

impl Default for PidManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_anchor(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ww_pid_{}_{}.pid", tag, std::process::id()))
    }

    /// Two managers, one lock: the second cannot acquire while the first lives,
    /// and the lock frees the moment the holder drops it (as process death does).
    #[test]
    fn lifecycle_locks_and_unlocks() {
        let path = tmp_anchor("life");
        let _ = fs::remove_file(&path);

        let mut mgr = PidManager::with_path(path.clone());
        assert!(!mgr.is_running());

        mgr.acquire().expect("should acquire the instance lock");
        assert!(mgr.is_running(), "lock held = daemon running");
        assert_eq!(mgr.read_pid(), Some(std::process::id() as i32));

        let mut second = PidManager::with_path(path.clone());
        assert!(
            second.acquire().is_err(),
            "a second instance must be refused"
        );
        assert!(second.is_running(), "probe sees the other holder");

        mgr.release();
        assert!(!mgr.is_running(), "lock released = daemon gone");

        let _ = fs::remove_file(&path);
    }

    /// The regression that wedged the systemd service: a stale anchor naming a
    /// dead/zombie PID must NOT block a fresh daemon (the old code probed
    /// `/proc/<pid>` and zombies looked alive forever).
    #[test]
    fn stale_pid_content_never_blocks_acquisition() {
        let path = tmp_anchor("stale");
        let _ = fs::remove_file(&path);
        // A leftover file naming a PID that is certainly not running.
        fs::write(&path, "999999999").unwrap();

        let mut mgr = PidManager::with_path(path.clone());
        assert!(
            !mgr.is_running(),
            "unlocked file with dead PID = not running"
        );
        mgr.acquire()
            .expect("stale content must not block the lock");
        assert!(mgr.is_running());
        assert_eq!(mgr.read_pid(), Some(std::process::id() as i32));

        mgr.release();
        let _ = fs::remove_file(&path);
    }
}
