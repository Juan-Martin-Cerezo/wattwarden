mod privilege;

use clap::{Parser, Subcommand};
use privilege::{is_root, require_root};
use std::process::Command;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use wattwarden_core::*;
use wattwarden_daemon::{
    install_systemd_service, uninstall_systemd_service, DaemonRunner, PidManager,
};
use wattwarden_platform::PlatformBackend as LinuxBackend;
use wattwarden_tui::{run_tui, App};

#[derive(Parser)]
#[command(
    name = "wattwarden",
    author = "Juan Martín Cerezo",
    version = "2.0.0",
    about = "⚡ WattWarden - Industrial Hardware Power & Auto-Brightness Suite (Rust)",
    long_about = "WattWarden gives you absolute ownership over your hardware's power constraints via direct sysfs, RAPL, and zero-polling Linux Netlink event loops."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the interactive TUI Dashboard (Default)
    Tui,

    /// Run background daemon in the foreground (used by systemd)
    Daemon,

    /// Start the background daemon service
    Start,

    /// Stop the running background daemon service
    Stop,

    /// Inspect current daemon status, battery drain, and profile
    Status,

    /// Apply and persist a power profile (normal, performance, extreme, auto)
    Profile {
        /// Target profile name
        name: String,
    },

    /// Set the Auto Extreme adaptive level (low, medium, high)
    Level {
        /// Target level name
        name: String,
    },

    /// Set battery BMS charge threshold ceiling (e.g. 80%)
    Threshold {
        /// Charge percentage ceiling [50-100]
        percent: u8,
    },

    /// Configure display brightness percentage or auto-brightness toggle
    Brightness {
        /// Target percentage (1-100) or 'on'/'off'
        value: String,
    },

    /// System service management
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Install & enable auto-starting systemd service
    Install,
    /// Disable and remove systemd service
    Uninstall,
}

fn sync_installed_binary() {
    if !is_root() {
        return;
    }
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };
    let target = std::path::Path::new("/usr/local/bin/wattwarden");
    if current_exe == target {
        return;
    }
    if let Ok(bytes) = std::fs::read(&current_exe) {
        let _ = std::fs::create_dir_all("/usr/local/bin");
        if std::fs::write(target, bytes).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
}

pub fn normalize_args(raw_args: &[String]) -> Vec<String> {
    if raw_args.is_empty() {
        return vec![];
    }
    let mut normalized = vec![raw_args[0].clone()];
    let mut i = 1;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "--daemon" => normalized.push("daemon".into()),
            "--start" => normalized.push("start".into()),
            "--stop" => normalized.push("stop".into()),
            "--status" => normalized.push("status".into()),
            "--brightness" => normalized.push("brightness".into()),
            "--install-service" | "install-service" => {
                normalized.push("service".into());
                normalized.push("install".into());
            }
            "--uninstall-service" | "uninstall-service" => {
                normalized.push("service".into());
                normalized.push("uninstall".into());
            }
            other => normalized.push(other.into()),
        }
        i += 1;
    }
    normalized
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sync_installed_binary();

    let raw_args: Vec<String> = std::env::args().collect();
    let normalized = normalize_args(&raw_args);
    let cli = Cli::parse_from(normalized);

    // Default to TUI if no subcommand provided
    let cmd = cli.command.unwrap_or(Commands::Tui);

    match cmd {
        Commands::Tui => {
            if !is_root() {
                eprintln!("Warning: Running without root privileges. Some hardware controls will be read-only.");
            }
            let backend = LinuxBackend::new()?;
            let config = Config::load_or_default(None);
            let app = App::new(backend, config);
            run_tui(app)?;
        }

        Commands::Daemon => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new("info"))
                .with(tracing_subscriber::fmt::layer())
                .init();

            let backend = LinuxBackend::new()?;
            let config = Config::load_or_default(None);
            let runner = DaemonRunner::new(backend, config);
            runner.run().await?;
        }

        Commands::Start => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let pid_mgr = PidManager::new();
            if pid_mgr.is_running() {
                println!(
                    "⚡ WattWarden daemon is already running (PID: {}).",
                    pid_mgr.read_pid().unwrap_or(0)
                );
                return Ok(());
            }

            // Spawn background process
            let current_exe = std::env::current_exe()?;
            let child = Command::new(current_exe).arg("daemon").spawn()?;

            println!(
                "⚡ WattWarden background daemon started with PID {}.",
                child.id()
            );
        }

        Commands::Stop => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let pid_mgr = PidManager::new();
            if let Some(pid) = pid_mgr.read_pid() {
                #[cfg(unix)]
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGTERM,
                );
                #[cfg(not(unix))]
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .status();
                pid_mgr.release();
                println!("🛑 WattWarden daemon (PID {}) stopped.", pid);
            } else {
                println!("WattWarden daemon is not currently active.");
            }
        }

        Commands::Status => {
            let pid_mgr = PidManager::new();
            let is_active = pid_mgr.is_running();
            let backend = LinuxBackend::new().ok();
            let config = Config::load_or_default(None);

            println!("⚡ WattWarden System Status");
            println!("──────────────────────────────────────────");
            println!(
                "Daemon Status     : {}",
                if is_active {
                    "[ACTIVE] (Running in background)"
                } else {
                    "[INACTIVE]"
                }
            );
            if let Some(pid) = pid_mgr.read_pid() {
                println!("Daemon PID        : {}", pid);
            }
            println!("Active Profile    : {}", config.profile);
            println!("Auto Extr. Level  : {}", config.auto_extreme_level);
            println!(
                "Auto-Brightness   : {}",
                if config.auto_brightness {
                    "Enabled"
                } else {
                    "Disabled"
                }
            );

            if let Some(b) = backend {
                let caps = b.capabilities();
                let chassis = if caps.has_battery {
                    "Laptop / Portable"
                } else {
                    "Desktop / Stationary Workstation"
                };
                println!("Chassis Type      : {}", chassis);

                if caps.has_battery {
                    if let Ok(pct) = b.battery.battery_percentage() {
                        let is_ac = b.battery.is_charging().unwrap_or(false);
                        let watts = b.battery.consumption_watts().unwrap_or(0.0);
                        println!(
                            "Battery Capacity  : {}% ({})",
                            pct,
                            if is_ac { "AC Connected" } else { "On Battery" }
                        );
                        println!("Discharge Rate    : {:.2} Watts", watts);
                    }
                    if b.threshold.supports_threshold() {
                        if let Ok(limit) = b.threshold.charge_threshold() {
                            println!("BMS Charge Ceiling: {}%", limit);
                        }
                    }
                } else {
                    println!("Power Source      : AC Mains (Stationary)");
                }
            }
        }

        Commands::Profile { name } => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let profile: PowerProfile = name.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
            let backend = LinuxBackend::new()?;
            backend.apply_profile(&profile)?;

            let mut cfg = Config::load_or_default(None);
            cfg.profile = profile.clone();
            cfg.auto_extreme_enabled = profile == PowerProfile::AutoExtreme;
            cfg.save(None)?;

            println!("✅ Power profile updated and persisted to: {}", profile);
        }

        Commands::Level { name } => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let level: AutoExtremeLevel = name.parse().map_err(|e| anyhow::anyhow!("{}", e))?;

            let mut cfg = Config::load_or_default(None);
            cfg.auto_extreme_level = level;
            cfg.save(None)?;

            println!("✅ Auto Extreme level updated and persisted to: {}", level);
        }

        Commands::Threshold { percent } => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let backend = LinuxBackend::new()?;
            if !backend.threshold.supports_threshold() {
                eprintln!("Error: Your hardware does not support setting battery charge limits.");
                std::process::exit(1);
            }
            backend.threshold.set_charge_threshold(percent)?;

            let mut cfg = Config::load_or_default(None);
            cfg.battery_charge_limit = Some(percent);
            cfg.save(None)?;

            println!(
                "✅ Battery charge ceiling set to {}%. Charging will stop at this threshold.",
                percent
            );
        }

        Commands::Brightness { value } => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let mut cfg = Config::load_or_default(None);
            let backend = LinuxBackend::new()?;

            match value.to_lowercase().as_str() {
                "on" | "1" | "enable" => {
                    cfg.auto_brightness = true;
                    cfg.save(None)?;
                    println!("✅ Dynamic auto-brightness enabled.");
                }
                "off" | "0" | "disable" => {
                    cfg.auto_brightness = false;
                    cfg.save(None)?;
                    println!("🛑 Dynamic auto-brightness disabled.");
                }
                digits => {
                    let pct: u8 = digits.parse().map_err(|_| {
                        anyhow::anyhow!("Invalid brightness percentage: {}", digits)
                    })?;
                    if let Some(bl) = &backend.backlight {
                        bl.set_brightness_percent(pct)?;
                        println!("Display brightness set to {}%.", pct);
                    } else {
                        eprintln!("Error: No controllable backlight interface found.");
                    }
                }
            }
        }

        Commands::Service { action } => {
            if let Err(e) = require_root() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            match action {
                ServiceAction::Install => {
                    let current_exe = std::env::current_exe()?;
                    install_systemd_service(&current_exe)?;
                    println!("✅ WattWarden systemd service installed and enabled successfully.");
                }
                ServiceAction::Uninstall => {
                    uninstall_systemd_service()?;
                    println!("🛑 WattWarden systemd service uninstalled.");
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_args() {
        let args = vec![
            "wattwarden".into(),
            "--status".into(),
            "--daemon".into(),
            "--install-service".into(),
        ];
        let normalized = normalize_args(&args);
        assert_eq!(
            normalized,
            vec!["wattwarden", "status", "daemon", "service", "install"]
        );

        let uninst = vec!["wattwarden".into(), "--uninstall-service".into()];
        assert_eq!(
            normalize_args(&uninst),
            vec!["wattwarden", "service", "uninstall"]
        );
    }

    #[test]
    fn test_cli_parsing_subcommands() {
        // TUI default
        let cli = Cli::try_parse_from(["wattwarden"]).unwrap();
        assert!(cli.command.is_none());

        // Status
        let cli = Cli::try_parse_from(["wattwarden", "status"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Status)));

        // Profile
        let cli = Cli::try_parse_from(["wattwarden", "profile", "extreme"]).unwrap();
        if let Some(Commands::Profile { name }) = cli.command {
            assert_eq!(name, "extreme");
        } else {
            panic!("Expected Commands::Profile");
        }

        // Level
        let cli = Cli::try_parse_from(["wattwarden", "level", "low"]).unwrap();
        if let Some(Commands::Level { name }) = cli.command {
            assert_eq!(name, "low");
        } else {
            panic!("Expected Commands::Level");
        }

        // Threshold
        let cli = Cli::try_parse_from(["wattwarden", "threshold", "80"]).unwrap();
        if let Some(Commands::Threshold { percent }) = cli.command {
            assert_eq!(percent, 80);
        } else {
            panic!("Expected Commands::Threshold");
        }

        // Brightness
        let cli = Cli::try_parse_from(["wattwarden", "brightness", "off"]).unwrap();
        if let Some(Commands::Brightness { value }) = cli.command {
            assert_eq!(value, "off");
        } else {
            panic!("Expected Commands::Brightness");
        }

        // Service install
        let cli = Cli::try_parse_from(["wattwarden", "service", "install"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Service {
                action: ServiceAction::Install
            })
        ));

        // Start & Stop
        let cli = Cli::try_parse_from(["wattwarden", "start"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Start)));

        let cli = Cli::try_parse_from(["wattwarden", "stop"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Stop)));
    }
}
