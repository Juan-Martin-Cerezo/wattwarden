//! Direct execution of the external utilities the macOS backend calls
//! (`pmset`, `ioreg`, `networksetup`, `defaults`, `blueutil`, `purge`).
//!
//! Binaries are invoked directly and every failure is ignored: an empty
//! string for a read, a no-op for a write. The child's stdio is detached so a
//! helper's output can never corrupt a TUI frame.
//!
//! This module is compiled on every host on purpose: it is the seam that lets the
//! macOS backend tests point `PATH` at fake binaries and run on Linux.

use std::process::{Command, Stdio};

/// Captured command execution: trimmed stdout, or `""` on any failure.
pub fn run_capture(program: &str, args: &[&str]) -> String {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

/// Fire-and-forget invocation; ignores errors.
pub fn run_ignored(program: &str, args: &[&str]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Serializes the `PATH`/env mutation of the platform mock tests.
#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_returns_empty_string() {
        assert_eq!(run_capture("ww-definitely-not-a-binary", &["x"]), "");
        // Must not panic either.
        run_ignored("ww-definitely-not-a-binary", &["x"]);
    }
}
