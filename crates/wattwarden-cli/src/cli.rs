//! Command-line interface dispatcher for WattWarden.
//!
//! An unknown flag falls through to the root privilege check followed by
//! the interactive dashboard.
//!
//! The `CliRuntime` trait isolates every effectful operation so the string/exit-code
//! contract can be unit-tested without root, hardware, systemd, or a terminal.

use std::io::Write;
use wattwarden_core::{AutoExtremeLevel, BatteryHealth, PowerProfile};

/// Battery facts the CLI needs to render `--health` and the `--status` battery line.
#[derive(Debug, Clone, Default)]
pub struct HealthReport {
    pub present: bool,
    pub health: BatteryHealth,
    pub capacity: u8,
    pub charging: bool,
    /// `Config.battery_charge_limit` (opt-in; `None` = WattWarden sets no ceiling).
    pub configured_limit: Option<u8>,
    /// Threshold actually active in sysfs (may come from the BIOS or another tool).
    pub active_threshold: Option<u8>,
    pub time_remaining: String,
}

/// Side effects the dispatcher needs. The production implementation talks to the
/// real platform/daemon/TUI; tests use an in-memory fake.
pub trait CliRuntime {
    fn is_root(&self) -> bool;
    /// Query whether the daemon is actively running (PID file or systemd service active).
    fn daemon_active(&self) -> bool;
    fn auto_brightness(&self) -> bool;
    /// Toggles the in-memory auto-brightness flag and persists configuration.
    fn set_auto_brightness(&self, enabled: bool);
    /// Synchronizes the active binary to the system install location.
    fn sync_installed_binary(&self);
    /// Starts the background daemon process.
    fn start_background_daemon(&self) -> Result<(), String>;
    /// Stops the background daemon process.
    fn stop_background_daemon(&self);
    fn install_service(&self) -> Result<(), String>;
    fn uninstall_service(&self) -> Result<(), String>;
    /// Runs the daemon loop in the foreground.
    fn run_daemon(&self) -> Result<(), String>;
    /// Launches the interactive dashboard.
    fn launch_dashboard(&self) -> Result<(), String>;
    /// Battery facts for `--health` / `--status` (read-only).
    fn health_report(&self) -> Result<HealthReport, String>;
    /// Persist `battery_charge_limit`; `Some(n)` also applies the hardware ceiling,
    /// `None` only clears WattWarden's setting (never touches sysfs).
    fn set_charge_limit(&self, limit: Option<u8>) -> Result<(), String>;
    /// Guided gauge recalibration (see `calibrate.rs`).
    fn calibrate_start(&self) -> Result<String, String>;
    fn calibrate_status(&self) -> Result<String, String>;
    fn calibrate_finish(&self) -> Result<String, String>;
    /// Apply a power profile and persist it (`None` = restore defaults).
    fn set_profile(&self, profile: Option<PowerProfile>) -> Result<(), String>;
    /// Persist the Auto Extreme adaptive level.
    fn set_auto_extreme_level(&self, level: AutoExtremeLevel) -> Result<(), String>;
    /// Set LCD brightness (1-100); disables auto-brightness like the dashboard does.
    fn set_brightness_percent(&self, percent: u8) -> Result<(), String>;
}

/// Standard command usage and help output.
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
    let _ = writeln!(
        out,
        "  sudo wattwarden --health          Show battery health, charge limit and suggestions"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --charge-limit <55-100|off> Set or clear the battery charge ceiling"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden --calibrate <start|status|done|abort> Recalibrate the battery gauge"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden profile <name>    Apply profile (normal, performance, extreme, auto)"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden level <low|medium|high> Set Auto Extreme adaptive level"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden brightness <val>  Set display brightness (1-100) or toggle auto (on/off)"
    );
    let _ = writeln!(
        out,
        "  sudo wattwarden service <install|uninstall> Manage the background system service"
    );
}

/// Renders the `--health` report. Pure: no hardware access, fully golden-testable.
pub fn render_health(r: &HealthReport) -> String {
    if !r.present {
        return "No battery detected (stationary mains).".to_string();
    }
    let h = &r.health;
    let device = match (&h.manufacturer, &h.model) {
        (Some(mfr), Some(model)) => format!("{mfr} {model}"),
        (None, Some(model)) => model.clone(),
        (Some(mfr), None) => mfr.clone(),
        (None, None) => "Unknown".to_string(),
    };
    let health_line = match (h.health_pct(), h.unit) {
        (Some(pct), Some(unit)) => match (h.full, h.design) {
            (Some(full), Some(design)) => format!(
                "{pct}% ({}/{}) {}",
                unit.scale_to_display(full),
                unit.scale_to_display(design),
                unit.display_label()
            ),
            _ => format!("{pct}%"),
        },
        _ => "not reported".to_string(),
    };
    let cycles = h
        .cycle_count
        .map(|c| c.to_string())
        .unwrap_or_else(|| "not reported".to_string());
    let temp = h
        .temp_c
        .map(|t| format!("{t:.1}°C"))
        .unwrap_or_else(|| "not reported".to_string());
    let state = if r.charging {
        "Charging"
    } else {
        "Discharging"
    };
    let limit_line = match (r.configured_limit, r.active_threshold) {
        (Some(c), Some(a)) => format!("configured {c}% (active threshold {a}%)"),
        (Some(c), None) => format!("configured {c}% (threshold not exposed)"),
        (None, Some(a)) => format!("not set (active threshold {a}%)"),
        (None, None) => "not set".to_string(),
    };

    let mut lines = vec![
        "Battery Health".to_string(),
        format!("  {:<14}: {device}", "Device"),
        format!("  {:<14}: {health_line}", "Health"),
        format!("  {:<14}: {cycles}", "Cycles"),
        format!("  {:<14}: {temp}", "Temperature"),
        format!(
            "  {:<14}: {capacity}% ({state})",
            "Charge",
            capacity = r.capacity
        ),
        format!("  {:<14}: {limit_line}", "Charge limit"),
        format!("  {:<14}: {}", "Est. time", r.time_remaining),
    ];
    // Suggestion only: WattWarden obeys the user, never the cable — it never
    // applies a ceiling by itself.
    if r.configured_limit.is_none() {
        lines.push(String::new());
        lines.push(
            "  Suggestion: sudo wattwarden charge-limit 80 — WattWarden never applies this by itself."
                .to_string(),
        );
    }
    lines.join("\n")
}

/// One-line battery summary for `--status`; `None` when no battery is present.
pub fn render_status_battery(r: &HealthReport) -> Option<String> {
    if !r.present {
        return None;
    }
    let health = r
        .health
        .health_pct()
        .map(|p| format!("{p}%"))
        .unwrap_or_else(|| "not reported".to_string());
    let limit = r
        .configured_limit
        .map(|l| format!("{l}%"))
        .unwrap_or_else(|| "not set".to_string());
    Some(format!("Battery Health: {health} | Charge limit: {limit}"))
}

/// Runs the CLI and returns the process exit code. Primary subcommand is evaluated,
/// and unhandled arguments fall through to the root check followed by the TUI.
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

            "service" => match args.get(2).map(String::as_str) {
                Some("install") => {
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
                Some("uninstall") => {
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
                _ => {
                    print_help(out);
                    return 1;
                }
            },

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
                if let Ok(report) = rt.health_report() {
                    if let Some(line) = render_status_battery(&report) {
                        let _ = writeln!(out, "{line}");
                    }
                }
                return 0;
            }

            "--health" | "health" => {
                match rt.health_report() {
                    Ok(report) => {
                        let _ = writeln!(out, "{}", render_health(&report));
                    }
                    Err(e) => {
                        let _ = writeln!(out, "Error: {e}");
                        return 1;
                    }
                }
                return 0;
            }

            "--charge-limit" | "charge-limit" => {
                if !rt.is_root() {
                    let _ = writeln!(
                        out,
                        "Error: Administrator/root privileges are required to change the charge limit."
                    );
                    return 1;
                }
                match args.get(2).map(String::as_str) {
                    Some("off") => {
                        if let Err(e) = rt.set_charge_limit(None) {
                            let _ = writeln!(out, "Error clearing charge limit: {e}");
                            return 1;
                        }
                        let _ = writeln!(
                            out,
                            "Charge limit cleared. WattWarden will not touch the charge threshold."
                        );
                        if let Ok(report) = rt.health_report() {
                            if let Some(a) = report.active_threshold {
                                let _ = writeln!(
                                    out,
                                    "Active threshold remains: {a}% (set outside WattWarden)."
                                );
                            }
                        }
                        return 0;
                    }
                    Some(arg) => match arg.parse::<u8>() {
                        Ok(n) if (55..=100).contains(&n) => {
                            if let Err(e) = rt.set_charge_limit(Some(n)) {
                                let _ = writeln!(out, "Error setting charge limit: {e}");
                                return 1;
                            }
                            let _ = writeln!(out, "Charge limit set to: {n}%");
                            return 0;
                        }
                        _ => {
                            let _ = writeln!(
                                out,
                                "Error: charge limit must be a value between 55 and 100, or 'off'."
                            );
                            return 1;
                        }
                    },
                    None => {
                        return match rt.health_report() {
                            Ok(report) => {
                                let _ = writeln!(
                                    out,
                                    "Charge limit: {}",
                                    match (report.configured_limit, report.active_threshold) {
                                        (Some(c), Some(a)) => {
                                            format!("configured {c}% (active threshold {a}%)")
                                        }
                                        (Some(c), None) => format!("configured {c}%"),
                                        (None, Some(a)) => {
                                            format!("not set (active threshold {a}%)")
                                        }
                                        (None, None) => "not set".to_string(),
                                    }
                                );
                                0
                            }
                            Err(e) => {
                                let _ = writeln!(out, "Error: {e}");
                                1
                            }
                        };
                    }
                }
            }

            "--calibrate" | "calibrate" => {
                if !rt.is_root() {
                    let _ = writeln!(
                        out,
                        "Error: Administrator/root privileges are required to run calibration."
                    );
                    return 1;
                }
                let result = match args.get(2).map(String::as_str) {
                    Some("start") | None => rt.calibrate_start(),
                    Some("status") => rt.calibrate_status(),
                    Some("done") | Some("abort") => rt.calibrate_finish(),
                    Some(other) => {
                        let _ = writeln!(
                            out,
                            "Error: unknown calibrate phase '{other}' (use start|status|done|abort)."
                        );
                        return 1;
                    }
                };
                return match result {
                    Ok(msg) => {
                        let _ = writeln!(out, "{msg}");
                        0
                    }
                    Err(e) => {
                        let _ = writeln!(out, "Error: {e}");
                        1
                    }
                };
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

            "brightness" => match args.get(2).map(String::as_str) {
                Some(arg @ ("on" | "off")) => {
                    let enabled = arg == "on";
                    rt.set_auto_brightness(enabled);
                    let _ = writeln!(out, "Auto-brightness set to: {enabled}");
                    return 0;
                }
                Some(arg) => match arg.parse::<u8>() {
                    Ok(n) if (1..=100).contains(&n) => {
                        if !rt.is_root() {
                            let _ = writeln!(
                                    out,
                                    "Error: Administrator/root privileges are required to set brightness."
                                );
                            return 1;
                        }
                        return match rt.set_brightness_percent(n) {
                            Ok(()) => {
                                let _ = writeln!(out, "Brightness set to: {n}%");
                                0
                            }
                            Err(e) => {
                                let _ = writeln!(out, "Error setting brightness: {e}");
                                1
                            }
                        };
                    }
                    _ => {
                        let _ = writeln!(out, "Error: brightness must be 1-100, 'on' or 'off'.");
                        return 1;
                    }
                },
                None => {
                    let _ = writeln!(
                        out,
                        "Auto-brightness is currently: {}",
                        rt.auto_brightness()
                    );
                    return 0;
                }
            },

            "profile" | "--profile" => {
                if !rt.is_root() {
                    let _ = writeln!(
                        out,
                        "Error: Administrator/root privileges are required to change system power settings."
                    );
                    return 1;
                }
                let name = args
                    .get(2)
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                let result = match name.as_str() {
                    "performance" | "perf" => rt.set_profile(Some(PowerProfile::Performance)),
                    "extreme" => rt.set_profile(Some(PowerProfile::Extreme)),
                    "normal" | "restore" | "default" => rt.set_profile(None),
                    "auto" => rt.start_background_daemon(),
                    _ => {
                        let _ = writeln!(
                            out,
                            "Error: unknown profile '{}' (use normal|performance|extreme|auto).",
                            args.get(2).map(String::as_str).unwrap_or("")
                        );
                        return 1;
                    }
                };
                return match result {
                    Ok(()) => {
                        let label = match name.as_str() {
                            "performance" | "perf" => "Performance",
                            "extreme" => "Extreme",
                            "auto" => "Auto Extreme",
                            _ => "Normal (restored)",
                        };
                        let _ = writeln!(out, "Power profile set to: {label}");
                        0
                    }
                    Err(e) => {
                        let _ = writeln!(out, "Error setting profile: {e}");
                        1
                    }
                };
            }

            "level" | "--level" => {
                if !rt.is_root() {
                    let _ = writeln!(
                        out,
                        "Error: Administrator/root privileges are required to change the Auto Extreme level."
                    );
                    return 1;
                }
                return match args.get(2).map(String::as_str) {
                    Some(l @ ("low" | "medium" | "high")) => {
                        let level = match l {
                            "low" => AutoExtremeLevel::Low,
                            "medium" => AutoExtremeLevel::Medium,
                            _ => AutoExtremeLevel::High,
                        };
                        match rt.set_auto_extreme_level(level) {
                            Ok(()) => {
                                let _ = writeln!(out, "Auto Extreme level set to: {l}");
                                0
                            }
                            Err(e) => {
                                let _ = writeln!(out, "Error setting level: {e}");
                                1
                            }
                        }
                    }
                    _ => {
                        let _ = writeln!(out, "Error: level must be low, medium or high.");
                        1
                    }
                };
            }

            "--version" | "-v" | "version" => {
                let _ = writeln!(out, "WattWarden v{}", env!("CARGO_PKG_VERSION"));
                return 0;
            }

            "--help" | "-h" | "help" => {
                print_help(out);
                return 0;
            }

            // Unknown first argument: falls through to the root check
            // and (as root) to the dashboard.
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
    use std::cell::{Cell, RefCell};

    struct FakeRuntime {
        root: bool,
        active: bool,
        auto_brightness: Cell<bool>,
        report: HealthReport,
        charge_limit: Cell<Option<u8>>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeRuntime {
        fn new(root: bool, active: bool, auto_brightness: bool) -> Self {
            Self {
                root,
                active,
                auto_brightness: Cell::new(auto_brightness),
                report: HealthReport::default(),
                charge_limit: Cell::new(None),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn with_report(mut self, report: HealthReport) -> Self {
            self.report = report;
            self
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
        fn health_report(&self) -> Result<HealthReport, String> {
            Ok(self.report.clone())
        }
        fn set_charge_limit(&self, limit: Option<u8>) -> Result<(), String> {
            self.charge_limit.set(limit);
            Ok(())
        }
        fn calibrate_start(&self) -> Result<String, String> {
            Ok("Calibration started (fake).".to_string())
        }
        fn calibrate_status(&self) -> Result<String, String> {
            Ok("Calibration in progress (fake).".to_string())
        }
        fn calibrate_finish(&self) -> Result<String, String> {
            Ok("Calibration finished (fake).".to_string())
        }
        fn set_profile(&self, profile: Option<PowerProfile>) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("profile:{profile:?}"));
            Ok(())
        }
        fn set_auto_extreme_level(&self, level: AutoExtremeLevel) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("level:{level:?}"));
            Ok(())
        }
        fn set_brightness_percent(&self, percent: u8) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("brightness:{percent}"));
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
        s.push_str("  sudo wattwarden --health          Show battery health, charge limit and suggestions\n");
        s.push_str("  sudo wattwarden --charge-limit <55-100|off> Set or clear the battery charge ceiling\n");
        s.push_str("  sudo wattwarden --calibrate <start|status|done|abort> Recalibrate the battery gauge\n");
        s.push_str("  sudo wattwarden profile <name>    Apply profile (normal, performance, extreme, auto)\n");
        s.push_str("  sudo wattwarden level <low|medium|high> Set Auto Extreme adaptive level\n");
        s.push_str("  sudo wattwarden brightness <val>  Set display brightness (1-100) or toggle auto (on/off)\n");
        s.push_str(
            "  sudo wattwarden service <install|uninstall> Manage the background system service\n",
        );
        s
    }

    #[test]
    fn golden_help_short_and_long_forms() {
        let rt = FakeRuntime::new(false, false, true);
        for flag in ["--help", "-h", "help"] {
            let (code, out) = run_args(&["wattwarden", flag], &rt);
            assert_eq!(code, 0, "help must exit 0");
            assert_eq!(out, expected_help(), "help text must match standard format");
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
    fn golden_version_flag() {
        let rt = FakeRuntime::new(false, false, true);
        let (code, out) = run_args(&["wattwarden", "--version"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, format!("WattWarden v{}\n", env!("CARGO_PKG_VERSION")));

        let (code, out) = run_args(&["wattwarden", "-v"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, format!("WattWarden v{}\n", env!("CARGO_PKG_VERSION")));
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

        // Accepts "1"/"true" as on; anything else is off.
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

        // An unknown flag falls back to the root check.
        let (code, out) = run_args(&["wattwarden", "--frobnicate"], &rt);
        assert_eq!(code, 1);
        assert_eq!(out, expected);

        // So does a bare unknown word.
        let (code, out) = run_args(&["wattwarden", "bogus-command"], &rt);
        assert_eq!(code, 1);
        assert_eq!(out, expected);
    }

    /// Unknown flags fall through to `sync_installed_binary` + the dashboard. The fake runtime
    /// records the dashboard launch so the fall-through is observable.
    #[test]
    fn unknown_flag_falls_through_to_dashboard_as_root() {
        use std::cell::Cell;
        struct SpyRuntime {
            dashboard_launched: Cell<bool>,
        }
        impl CliRuntime for SpyRuntime {
            fn is_root(&self) -> bool {
                true
            }
            fn daemon_active(&self) -> bool {
                false
            }
            fn auto_brightness(&self) -> bool {
                true
            }
            fn set_auto_brightness(&self, _enabled: bool) {}
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
                self.dashboard_launched.set(true);
                Ok(())
            }
            fn health_report(&self) -> Result<HealthReport, String> {
                Ok(HealthReport::default())
            }
            fn set_charge_limit(&self, _limit: Option<u8>) -> Result<(), String> {
                Ok(())
            }
            fn calibrate_start(&self) -> Result<String, String> {
                Ok(String::new())
            }
            fn calibrate_status(&self) -> Result<String, String> {
                Ok(String::new())
            }
            fn calibrate_finish(&self) -> Result<String, String> {
                Ok(String::new())
            }
            fn set_profile(&self, _profile: Option<PowerProfile>) -> Result<(), String> {
                Ok(())
            }
            fn set_auto_extreme_level(&self, _level: AutoExtremeLevel) -> Result<(), String> {
                Ok(())
            }
            fn set_brightness_percent(&self, _percent: u8) -> Result<(), String> {
                Ok(())
            }
        }
        for flag in ["--frobnicate", "bogus-flag-xyz"] {
            let rt = SpyRuntime {
                dashboard_launched: Cell::new(false),
            };
            let owned: Vec<String> = ["wattwarden", flag].iter().map(|s| s.to_string()).collect();
            let mut buf: Vec<u8> = Vec::new();
            let code = run(&owned, &mut buf, &rt);
            assert_eq!(code, 0, "unknown flag {flag} must not error");
            assert_eq!(String::from_utf8(buf).unwrap(), "");
            assert!(
                rt.dashboard_launched.get(),
                "unknown flag {flag} must fall through to the dashboard"
            );
        }
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

    fn vostro_report() -> HealthReport {
        HealthReport {
            present: true,
            health: BatteryHealth {
                present: true,
                full: Some(2_068_000),
                design: Some(3_684_000),
                unit: Some(wattwarden_core::HealthUnit::ChargeUah),
                cycle_count: Some(0),
                temp_c: Some(38.5),
                model: Some("DELL FC92NFB".to_string()),
                manufacturer: Some("Samsung SDI".to_string()),
                serial: None,
            },
            capacity: 75,
            charging: false,
            configured_limit: None,
            active_threshold: Some(80),
            time_remaining: "2h 30m".to_string(),
        }
    }

    #[test]
    fn golden_health_without_battery() {
        let rt = FakeRuntime::new(true, false, false);
        let (code, out) = run_args(&["wattwarden", "--health"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "No battery detected (stationary mains).\n");
    }

    /// The Vostro 5471 pack, golden-rendered: 2068/3684 mAh = 56.1% with the
    /// suggestion nudge (charge limit unset).
    #[test]
    fn golden_health_vostro_report_with_nudge() {
        let rt = FakeRuntime::new(true, false, false).with_report(vostro_report());
        let (code, out) = run_args(&["wattwarden", "health"], &rt);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "Battery Health\n\
             \x20 Device        : Samsung SDI DELL FC92NFB\n\
             \x20 Health        : 56.1% (2068/3684) mAh\n\
             \x20 Cycles        : 0\n\
             \x20 Temperature   : 38.5°C\n\
             \x20 Charge        : 75% (Discharging)\n\
             \x20 Charge limit  : not set (active threshold 80%)\n\
             \x20 Est. time     : 2h 30m\n\
             \n\
             \x20 Suggestion: sudo wattwarden charge-limit 80 — WattWarden never applies this by itself.\n"
        );
    }

    #[test]
    fn health_with_limit_hides_nudge() {
        let mut report = vostro_report();
        report.configured_limit = Some(80);
        report.active_threshold = Some(80);
        let rt = FakeRuntime::new(true, false, false).with_report(report);
        let (code, out) = run_args(&["wattwarden", "--health"], &rt);
        assert_eq!(code, 0);
        assert!(out.contains("Charge limit  : configured 80% (active threshold 80%)"));
        assert!(!out.contains("Suggestion"));
    }

    #[test]
    fn status_shows_battery_line_only_with_battery() {
        let rt = FakeRuntime::new(true, true, false).with_report(vostro_report());
        let (_, out) = run_args(&["wattwarden", "--status"], &rt);
        assert_eq!(
            out,
            "WattWarden Daemon Status: [ACTIVE] (Running in background)\nBattery Health: 56.1% | Charge limit: not set\n"
        );

        // Without a battery the status line is unchanged.
        let rt = FakeRuntime::new(true, true, false);
        let (_, out) = run_args(&["wattwarden", "--status"], &rt);
        assert_eq!(
            out,
            "WattWarden Daemon Status: [ACTIVE] (Running in background)\n"
        );
    }

    #[test]
    fn charge_limit_set_clear_and_validate() {
        let rt = FakeRuntime::new(true, false, false);

        let (code, out) = run_args(&["wattwarden", "--charge-limit", "80"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Charge limit set to: 80%\n");
        assert_eq!(rt.charge_limit.get(), Some(80));

        let (code, out) = run_args(&["wattwarden", "--charge-limit", "off"], &rt);
        assert_eq!(code, 0);
        assert!(out.contains("Charge limit cleared"));
        assert_eq!(rt.charge_limit.get(), None);

        for bad in ["30", "101", "banana"] {
            let (code, out) = run_args(&["wattwarden", "--charge-limit", bad], &rt);
            assert_eq!(code, 1, "{bad} must be rejected");
            assert!(out.contains("must be a value between 55 and 100"));
        }
    }

    /// `charge-limit off` must never write sysfs: the active threshold may come
    /// from the BIOS or another tool.
    #[test]
    fn charge_limit_off_reports_external_threshold() {
        let rt = FakeRuntime::new(true, false, false).with_report(vostro_report());
        let (code, out) = run_args(&["wattwarden", "charge-limit", "off"], &rt);
        assert_eq!(code, 0);
        assert!(out.contains("Active threshold remains: 80% (set outside WattWarden)."));
        assert_eq!(rt.charge_limit.get(), None);
    }

    #[test]
    fn charge_limit_and_calibrate_require_root() {
        let rt = FakeRuntime::new(false, false, false);
        let (code, out) = run_args(&["wattwarden", "--charge-limit", "80"], &rt);
        assert_eq!(code, 1);
        assert!(out.contains("root privileges"));
        let (code, out) = run_args(&["wattwarden", "--calibrate", "start"], &rt);
        assert_eq!(code, 1);
        assert!(out.contains("root privileges"));
    }

    #[test]
    fn calibrate_flow_dispatches_phases() {
        let rt = FakeRuntime::new(true, false, false);
        let (code, out) = run_args(&["wattwarden", "--calibrate", "start"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Calibration started (fake).\n");

        let (code, out) = run_args(&["wattwarden", "--calibrate", "status"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Calibration in progress (fake).\n");

        for verb in ["done", "abort"] {
            let (code, out) = run_args(&["wattwarden", "--calibrate", verb], &rt);
            assert_eq!(code, 0);
            assert_eq!(out, "Calibration finished (fake).\n");
        }

        let (code, out) = run_args(&["wattwarden", "--calibrate", "bogus"], &rt);
        assert_eq!(code, 1);
        assert!(out.contains("unknown calibrate phase 'bogus'"));
    }

    #[test]
    fn service_subcommand_manages_service() {
        let rt = FakeRuntime::new(true, false, false);
        let (code, out) = run_args(&["wattwarden", "service", "install"], &rt);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "✅ WattWarden background service installed and started successfully.\n"
        );
        let (code, out) = run_args(&["wattwarden", "service", "uninstall"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "✅ WattWarden background service uninstalled.\n");
        let (code, out) = run_args(&["wattwarden", "service", "bogus"], &rt);
        assert_eq!(code, 1);
        assert!(
            out.contains("Usage:"),
            "unknown service subcommand prints help"
        );

        let rt = FakeRuntime::new(false, false, false);
        let (code, _) = run_args(&["wattwarden", "service", "install"], &rt);
        assert_eq!(code, 1);
    }

    #[test]
    fn profile_subcommand_applies_and_validates() {
        let rt = FakeRuntime::new(true, false, false);
        let (code, out) = run_args(&["wattwarden", "profile", "performance"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Power profile set to: Performance\n");
        let (code, out) = run_args(&["wattwarden", "profile", "normal"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Power profile set to: Normal (restored)\n");
        let (code, out) = run_args(&["wattwarden", "profile", "auto"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Power profile set to: Auto Extreme\n");
        let (code, out) = run_args(&["wattwarden", "profile", "bogus"], &rt);
        assert_eq!(code, 1);
        assert!(out.contains("unknown profile 'bogus'"));

        let calls = rt.calls.borrow().clone();
        assert!(calls.contains(&"profile:Some(Performance)".to_string()));
        assert!(calls.contains(&"profile:None".to_string()));

        let rt = FakeRuntime::new(false, false, false);
        let (code, _) = run_args(&["wattwarden", "profile", "extreme"], &rt);
        assert_eq!(code, 1);
    }

    #[test]
    fn level_subcommand_sets_level_and_requires_root() {
        let rt = FakeRuntime::new(true, false, false);
        for (arg, label) in [("low", "Low"), ("medium", "Medium"), ("high", "High")] {
            let (code, out) = run_args(&["wattwarden", "level", arg], &rt);
            assert_eq!(code, 0);
            assert_eq!(out, format!("Auto Extreme level set to: {arg}\n"));
            assert!(rt.calls.borrow().contains(&format!("level:{label}")));
        }
        let (code, out) = run_args(&["wattwarden", "level", "banana"], &rt);
        assert_eq!(code, 1);
        assert!(out.contains("level must be low, medium or high"));

        let rt = FakeRuntime::new(false, false, false);
        let (code, _) = run_args(&["wattwarden", "level", "low"], &rt);
        assert_eq!(code, 1);
    }

    #[test]
    fn brightness_subcommand_sets_percent_or_auto() {
        let rt = FakeRuntime::new(true, false, false);
        let (code, out) = run_args(&["wattwarden", "brightness", "50"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Brightness set to: 50%\n");
        assert!(rt.calls.borrow().contains(&"brightness:50".to_string()));

        let (code, out) = run_args(&["wattwarden", "brightness", "on"], &rt);
        assert_eq!(code, 0);
        assert_eq!(out, "Auto-brightness set to: true\n");

        for bad in ["0", "101", "banana"] {
            let (code, out) = run_args(&["wattwarden", "brightness", bad], &rt);
            assert_eq!(code, 1, "{bad} must be rejected");
            assert!(out.contains("brightness must be 1-100"));
        }

        let rt = FakeRuntime::new(false, false, false);
        let (code, _) = run_args(&["wattwarden", "brightness", "50"], &rt);
        assert_eq!(code, 1);
    }
}
