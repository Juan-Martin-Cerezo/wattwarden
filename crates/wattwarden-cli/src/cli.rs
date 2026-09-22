//! Byte-for-byte CLI dispatch mirroring `main.go` from the Go reference (`master`).
//!
//! The Go binary is the specification (`PARITY.md` §3). Notably, an unknown flag is
//! **not** a parse error: Go falls through to the root check and emits the generic
//! "administrator/root privileges" message with exit code 1. Reproducing that is why
//! this dispatcher is hand-written instead of going through `clap`.
//!
//! The `CliRuntime` trait isolates every effectful operation so the string/exit-code
//! contract can be golden-tested without root, hardware, systemd or a terminal.

use std::io::Write;

/// Side effects the dispatcher needs. The production implementation talks to the
/// real platform/daemon/TUI; tests use an in-memory fake.
pub trait CliRuntime {
    fn is_root(&self) -> bool;
    /// `service.IsDaemonActive()`: PID file alive, or `systemctl is-active` == `active`.
    fn daemon_active(&self) -> bool;
    fn auto_brightness(&self) -> bool;
    /// Go `SetAutoBrightness`: toggles the in-memory flag and persists the config.
    fn set_auto_brightness(&self, enabled: bool);
    /// Go `SyncInstalledBinary`.
    fn sync_installed_binary(&self);
    /// Go `StartBackgroundDaemon`.
    fn start_background_daemon(&self) -> Result<(), String>;
    /// Go `StopBackgroundDaemon`.
    fn stop_background_daemon(&self);
    fn install_service(&self) -> Result<(), String>;
    fn uninstall_service(&self) -> Result<(), String>;
    /// Go `service.RunDaemon` (blocking foreground loop).
    fn run_daemon(&self) -> Result<(), String>;
    /// Go `ui.StartDashboard` (blocking interactive dashboard).
    fn launch_dashboard(&self) -> Result<(), String>;
}

/// Exact Go help text (`main.go` `--help` case).
pub fn print_help(out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "⚡ WattWarden - Hardware Power & Auto-Brightness Management"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage:");
    let _ = writeln!(
        out,
        "  sudo wattwarden                   Launch interactive TUI Dashboard"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --daemon          Run background auto-tuning daemon in foreground"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --start           Start background daemon (persists after closing terminal)"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --stop            Stop background daemon"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --status          Check background daemon status"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --brightness <on|off> Configure auto-brightness setting"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --install-service Install & enable auto-start system service"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --uninstall-service Remove system service"
    );
}

/// Runs the CLI and returns the process exit code. Mirrors `main.go` argument order:
/// only `args[1]` selects a command (and `args[2]` for `--brightness`), anything else
/// falls through to the root check followed by the TUI.
pub fn run(args: &[String], out: &mut dyn Write, rt: &dyn CliRuntime) -> i32 {
    if let Some(cmd) = args.get(1).map(String::as_str) {
        match cmd {
            "--daemon" | "daemon" => {
                if !rt.is_root() {
                    let _ = writeln!(
                        out,
                        "Error: Administrator/root privileges are required to run the daemon."
                    );
                    return 1;
                }
                if let Err(e) = rt.run_daemon() {
                    let _ = writeln!(out, "Error running daemon: {e}");
                    return 1;
                }
                return 0;
            }

            "--install-service" | "install-service" => {
                if !rt.is_root() {
                    let _ = writeln!(
                        out,
                        "Error: Administrator/root privileges are required to install the service."
                    );
                    return 1;
                }
                if let Err(e) = rt.install_service() {
                    let _ = writeln!(out, "Error installing service: {e}");
                    return 1;
                }
                let _ = writeln!(
                    out,
                    "✅ WattWarden background service installed and started successfully."
                );
                return 0;
            }

            "--uninstall-service" | "uninstall-service" => {
                if !rt.is_root() {
                    let _ = writeln!(
                        out,
                        "Error: Administrator/root privileges are required to uninstall the service."
                    );
                    return 1;
                }
                if let Err(e) = rt.uninstall_service() {
                    let _ = writeln!(out, "Error uninstalling service: {e}");
                    return 1;
                }
                let _ = writeln!(out, "✅ WattWarden background service uninstalled.");
                return 0;
            }

            "--start" | "start" => {
                if !rt.is_root() {
                    let _ = writeln!(out, "Error: Administrator/root privileges are required.");
                    return 1;
                }
                if let Err(e) = rt.start_background_daemon() {
                    let _ = writeln!(out, "Error starting daemon: {e}");
                    return 1;
                }
                let _ = writeln!(out, "⚡ WattWarden background daemon started.");
                return 0;
            }

            "--stop" | "stop" => {
                if !rt.is_root() {
                    let _ = writeln!(out, "Error: Administrator/root privileges are required.");
                    return 1;
                }
                rt.stop_background_daemon();
                let _ = writeln!(out, "🛑 WattWarden background daemon stopped.");
                return 0;
            }

            "--status" | "status" => {
                if rt.daemon_active() {
                    let _ = writeln!(
                        out,
                        "WattWarden Daemon Status: [ACTIVE] (Running in background)"
                    );
                } else {
                    let _ = writeln!(out, "WattWarden Daemon Status: [INACTIVE]");
                }
                return 0;
            }

            "--brightness" => {
                if let Some(arg) = args.get(2) {
                    let enabled = arg == "on" || arg == "1" || arg == "true";
                    rt.set_auto_brightness(enabled);
                    let _ = writeln!(out, "Auto-brightness set to: {enabled}");
                } else {
                    let _ = writeln!(
                        out,
                        "Auto-brightness is currently: {}",
                        rt.auto_brightness()
                    );
                }
                return 0;
            }

            "--help" | "-h" | "help" => {
                print_help(out);
                return 0;
            }

            // Unknown first argument: Go does not error out, it falls through to the
            // root check and (as root) to the dashboard.
            _ => {}
        }
    }

    if !rt.is_root() {
        let _ = writeln!(
            out,
            "Error: You must run this program with administrator/root privileges to change system power settings."
        );
        return 1;
    }

    rt.sync_installed_binary();

    if std::env::var("WATTWARDEN_DAEMON").as_deref() == Ok("1") {
        if let Err(e) = rt.run_daemon() {
            let _ = writeln!(out, "Error running daemon: {e}");
            return 1;
        }
        return 0;
    }

    if let Err(e) = rt.launch_dashboard() {
        let _ = writeln!(out, "Error: {e}");
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeRuntime {
        root: bool,
        active: bool,
        auto_brightness: Cell<bool>,
    }

    impl FakeRuntime {
        fn new(root: bool, active: bool, auto_brightness: bool) -> Self {
            Self {
                root,
                active,
                auto_brightness: Cell::new(auto_brightness),
            }
        }
    }

    impl CliRuntime for FakeRuntime {
        fn is_root(&self) -> bool {
            self.root
        }
        fn daemon_active(&self) -> bool {
            self.active
        }
        fn auto_brightness(&self) -> bool {
            self.auto_brightness.get()
        }
        fn set_auto_brightness(&self, enabled: bool) {
            self.auto_brightness.set(enabled);
        }
        fn sync_installed_binary(&self) {}
        fn start_background_daemon(&self) -> Result<(), String> {
            Ok(())
        }
        fn stop_background_daemon(&self) {}
        fn install_service(&self) -> Result<(), String> {
            Ok(())
        }
        fn uninstall_service(&self) -> Result<(), String> {
            Ok(())
        }
        fn run_daemon(&self) -> Result<(), String> {
            Ok(())
        }
        fn launch_dashboard(&self) -> Result<(), String> {
            Ok(())
        }
    }

    fn run_args(args: &[&str], rt: &FakeRuntime) -> (i32, String) {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut buf: Vec<u8> = Vec::new();
        let code = run(&owned, &mut buf, rt);
        (code, String::from_utf8(buf).expect("utf-8 output"))
    }

    fn expected_help() -> String {
        let mut s = String::new();
        s.push_str("⚡ WattWarden - Hardware Power & Auto-Brightness Management\n");
        s.push('\n');
        s.push_str("Usage:\n");
        s.push_str("  sudo wattwarden                   Launch interactive TUI Dashboard\n");
        s.push_str(
            "  sudo wattwarden --daemon          Run background auto-tuning daemon in foreground\n",
        );
        s.push_str("  sudo wattwarden --start           Start background daemon (persists after closing terminal)\n");
        s.push_str("  sudo wattwarden --stop            Stop background daemon\n");
        s.push_str("  sudo wattwarden --status          Check background daemon status\n");
        s.push_str("  sudo wattwarden --brightness <on|off> Configure auto-brightness setting\n");
        s.push_str(
            "  sudo wattwarden --install-service Install & enable auto-start system service\n",
        );
        s.push_str("  sudo wattwarden --uninstall-service Remove system service\n");
        s
    }

    #[test]
    fn golden_help_short_and_long_forms() {
        let rt = FakeRuntime::new(false, false, true);
        for flag in ["--help", "-h", "help"] {
            let (code, out) = run_args(&["wattwarden", flag], &rt);
            assert_eq!(code, 0, "help must exit 0");
            assert_eq!(
                out,
                expected_help(),
                "help text must match Go byte-for-byte"
            );
        }
    }

    #[test]
    fn golden_status_active_and_inactive() {
        let active = FakeRuntime::new(false, true, true);
        let (code, out) = run_args(&["wattwarden", "--status"], &active);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "WattWarden Daemon Status: [ACTIVE] (Running in background)\n"
        );

        // Bare word form must behave identically.
        let (code, out) = run_args(&["wattwarden", "status"], &active);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "WattWarden Daemon Status: [ACTIVE] (Running in background)\n"
        );

        let inactive = FakeRuntime::new(false, false, true);
        let (code, out) = run_args(&["wattwarden", "--status"], &inactive);
        assert_eq!(code, 0);
        assert_eq!(out, "WattWarden Daemon Status: [INACTIVE]\n");
    }

    #[test]
    fn golden_brightness_report_and_set() {
        let rt = FakeRuntime::new(false, false, true);

        let (code, out) = run_args(&["wattwarden", "--brightness"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Auto-brightness is currently: true\n");
        assert!(rt.auto_brightness.get(), "report must not mutate the flag");

        let (code, out) = run_args(&["wattwarden", "--brightness", "off"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Auto-brightness set to: false\n");
        assert!(!rt.auto_brightness.get());

        let (code, out) = run_args(&["wattwarden", "--brightness", "on"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Auto-brightness set to: true\n");
        assert!(rt.auto_brightness.get());

        // Go accepts "1"/"true" as on; anything else is off.
        let (_, _) = run_args(&["wattwarden", "--brightness", "1"], &rt);
        assert!(rt.auto_brightness.get());
        let (_, _) = run_args(&["wattwarden", "--brightness", "true"], &rt);
        assert!(rt.auto_brightness.get());
        let (_, out) = run_args(&["wattwarden", "--brightness", "maybe"], &rt);
        assert_eq!(out, "Auto-brightness set to: false\n");
        assert!(!rt.auto_brightness.get());
    }

    #[test]
    fn golden_root_error_without_flags_and_unknown_flag() {
        let rt = FakeRuntime::new(false, false, true);
        let expected = "Error: You must run this program with administrator/root privileges to change system power settings.\n";

        // No flags at all.
        let (code, out) = run_args(&["wattwarden"], &rt);
        assert_eq!(code, 1);
        assert_eq!(out, expected);

        // An unknown flag is NOT a clap-style parse error: Go falls back to the root check.
        let (code, out) = run_args(&["wattwarden", "--frobnicate"], &rt);
        assert_eq!(code, 1);
        assert_eq!(out, expected);

        // So does a bare unknown word.
        let (code, out) = run_args(&["wattwarden", "profile", "extreme"], &rt);
        assert_eq!(code, 1);
        assert_eq!(out, expected);
    }

    #[test]
    fn root_required_messages_per_subcommand() {
        let rt = FakeRuntime::new(false, false, true);

        let (code, out) = run_args(&["wattwarden", "--daemon"], &rt);
        assert_eq!(code, 1);
        assert_eq!(
            out,
            "Error: Administrator/root privileges are required to run the daemon.\n"
        );

        let (code, out) = run_args(&["wattwarden", "--start"], &rt);
        assert_eq!(code, 1);
        assert_eq!(out, "Error: Administrator/root privileges are required.\n");

        let (code, out) = run_args(&["wattwarden", "--install-service"], &rt);
        assert_eq!(code, 1);
        assert_eq!(
            out,
            "Error: Administrator/root privileges are required to install the service.\n"
        );

        let (code, out) = run_args(&["wattwarden", "--uninstall-service"], &rt);
        assert_eq!(code, 1);
        assert_eq!(
            out,
            "Error: Administrator/root privileges are required to uninstall the service.\n"
        );
    }

    #[test]
    fn golden_service_and_lifecycle_strings_as_root() {
        let rt = FakeRuntime::new(true, false, true);

        let (code, out) = run_args(&["wattwarden", "--start"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "⚡ WattWarden background daemon started.\n");

        let (code, out) = run_args(&["wattwarden", "--stop"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "🛑 WattWarden background daemon stopped.\n");

        let (code, out) = run_args(&["wattwarden", "--install-service"], &rt);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "✅ WattWarden background service installed and started successfully.\n"
        );

        let (code, out) = run_args(&["wattwarden", "--uninstall-service"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "✅ WattWarden background service uninstalled.\n");
    }
}
