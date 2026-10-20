//! Best-effort privilege escalation for one-shot hardware writes (charge
//! limit, calibration). Used when a control node needs root and the current
//! process is not root.
//!
//! Order: cached-credential `sudo -n` (silent, TUI-safe) → interactive `sudo`
//! (only when there is a terminal to prompt on) → `pkexec` (graphical agent).

use crate::exec::has_program;
use std::io::IsTerminal;
use std::process::{Command, Stdio};

/// Outcome of an escalation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Escalation {
    /// The elevated command ran and exited with this code.
    Ran(i32),
    /// The user dismissed the graphical prompt.
    Cancelled,
    /// No escalator was available or usable.
    Unavailable,
}

/// Re-runs `argv[0]` with `argv[1..]` through an available escalator.
/// `interactive` allows a terminal password prompt; without it the escalation
/// can never consume stdin or disturb a TUI frame.
pub fn rerun_elevated(argv: &[String], interactive: bool) -> Escalation {
    let Some((program, args)) = argv.split_first() else {
        return Escalation::Unavailable;
    };

    let mut tried_any = false;

    // 1) Silent sudo: instant when credentials are cached, never prompts.
    if has_program("sudo") {
        tried_any = true;
        let status = Command::new("sudo")
            .arg("-n")
            .arg(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Ok(st) = status {
            if st.success() {
                return Escalation::Ran(0);
            }
            if st.code() != Some(1) {
                // Refusals exit 1 (and fall through to the next escalator);
                // anything else is the command's own exit code.
                return Escalation::Ran(st.code().unwrap_or(1));
            }
        }
    }

    // 2) Interactive sudo, only where a terminal exists to type the password.
    if interactive && has_program("sudo") && std::io::stdout().is_terminal() {
        if let Ok(st) = Command::new("sudo").arg(program).args(args).status() {
            return Escalation::Ran(st.code().unwrap_or(if st.success() { 0 } else { 1 }));
        }
    }

    // 3) Graphical escalation through the desktop's polkit agent.
    if has_program("pkexec") {
        let status = Command::new("pkexec")
            .arg(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return match status {
            Ok(st) if st.success() => Escalation::Ran(0),
            // pkexec exits 126/127 when the user dismisses the auth dialog.
            Ok(st) if matches!(st.code(), Some(126) | Some(127)) => Escalation::Cancelled,
            Ok(st) => Escalation::Ran(st.code().unwrap_or(1)),
            Err(_) => Escalation::Unavailable,
        };
    }

    if tried_any {
        Escalation::Ran(1)
    } else {
        Escalation::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No escalator in a stripped PATH: reports Unavailable, never panics.
    #[test]
    fn missing_escalators_report_unavailable() {
        let _lock = crate::exec::env_lock();
        let argv = vec!["/nonexistent/wattwarden".to_string(), "x".to_string()];
        // With a PATH that contains nothing, no escalator can be found.
        let old = std::env::var_os("PATH");
        std::env::set_var("PATH", "/nonexistent-dir");
        let result = rerun_elevated(&argv, false);
        match old {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        assert!(matches!(result, Escalation::Unavailable));
    }

    #[test]
    fn empty_argv_is_unavailable() {
        assert_eq!(rerun_elevated(&[], true), Escalation::Unavailable);
    }
}
