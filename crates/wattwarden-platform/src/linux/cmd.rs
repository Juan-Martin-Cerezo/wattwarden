//! Thin wrappers around external utilities (`iw`, `rfkill`, `brightnessctl`).
//!
//! Binaries are invoked directly without intermediate shells. All helpers are best effort:
//! a missing binary yields `""` or is ignored gracefully.

use std::process::{Command, Stdio};

/// Executes command and returns trimmed stdout, or `""` on any failure.
///
/// Child stdio is detached from our own stdout/stderr: when this code runs
/// inside the TUI (alternate screen) or the daemon was spawned from the TUI,
/// any inherited output (e.g. `brightnessctl`'s `Updated device ...` line)
/// would corrupt the frame.
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

/// Fire-and-forget command invocation with silenced output streams.
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
