//! Thin wrappers around the handful of external utilities the Go reference
//! implementation shells out to (`iw`, `rfkill`, `brightnessctl`).
//!
//! Go used `sh -c`; we invoke the binaries directly. All helpers are best effort:
//! a missing binary yields `""` / is ignored, exactly like Go's error-ignoring
//! `runCmd()`.

use std::process::{Command, Stdio};

/// Go `runCmd("...")`: trimmed stdout, or `""` on any failure.
///
/// Child stdio is detached from our own stdout/stderr: when this code runs
/// inside the TUI (alternate screen) or the daemon was spawned from the TUI,
/// any inherited output (e.g. `brightnessctl`'s `Updated device ...` line)
/// would corrupt the frame. Go never prints those outputs either.
pub(crate) fn run_capture(program: &str, args: &[&str]) -> String {
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

/// Fire-and-forget invocation (Go ignores the result of these too).
pub(crate) fn run_ignored(program: &str, args: &[&str]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
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
