//! Direct execution of the external utilities the Go macOS/Windows backends call
//! (`pmset`, `ioreg`, `networksetup`, `defaults`, `blueutil`, `powercfg`,
//! `powershell`, `netsh`, `typeperf`, `purge`).
//!
//! Binaries are invoked directly — never through `sh -c` (AGENTS.md) — and every
//! failure is ignored exactly like Go's `runCmd`/`runMacCmd`/`runWinCmd`: an empty
//! string for a read, a no-op for a write. The child's stdio is detached so a
//! helper's output (e.g. `brightnessctl`'s "Updated device …") can never corrupt a
//! TUI frame; Go never printed those either.
//!
//! This module is compiled on every host on purpose: it is the seam that lets the
//! macOS/Windows backend tests point `PATH` at fake binaries and run here on Linux.

use std::process::{Command, Stdio};

/// Go `runMacCmd`/`runWinCmd`: trimmed stdout, or `""` on any failure.
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

/// Fire-and-forget invocation; Go ignores the result of these too.
pub fn run_ignored(program: &str, args: &[&str]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Serializes the `PATH`/env mutation of the platform mock tests. Only defined where
/// one of those tests exists: macOS (`cfg(unix)`) and the Windows mock (`cfg(not(windows))`).
#[cfg(all(test, any(unix, not(target_os = "windows"))))]
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
