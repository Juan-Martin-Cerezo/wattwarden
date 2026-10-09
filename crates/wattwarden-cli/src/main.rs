mod cli;
mod privilege;
mod service;

use cli::{run, CliRuntime};
use std::io::Write;
#[cfg(target_os = "linux")]
use std::path::Path;
// Only Linux (systemd) shells out from the CLI; macOS drives
// everything through its backend.
#[cfg(target_os = "linux")]
use std::process::Command;
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt, util::SubscriberInitExt};
use wattwarden_core::Config;
use wattwarden_daemon::{spawn, DaemonRunner, PidManager};
use wattwarden_platform::PlatformBackend as LinuxBackend;
use wattwarden_tui::{run_tui, App};

/// `tracing_subscriber` writer routing daemon logs to the system log file
/// (`/var/log/wattwarden.log`), never to stdout. Falls back to a sink when the
/// file cannot be opened (e.g. non-root), so the TUI alternate screen is never
/// touched either way.
#[derive(Debug, Clone, Copy)]
struct DaemonLogWriter;

impl<'a> MakeWriter<'a> for DaemonLogWriter {
    type Writer = Box<dyn Write + 'a>;

    fn make_writer(&'a self) -> Self::Writer {
        match spawn::open_daemon_log(&spawn::daemon_log_path()) {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::sink()),
        }
    }
}

/// Checks whether the background daemon or system service is active.
fn daemon_active() -> bool {
    if PidManager::new().is_running() {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = Command::new("systemctl")
            .args(["is-active", "wattwarden.service"])
            .output()
        {
            if String::from_utf8_lossy(&out.stdout).trim() == "active" {
                return true;
            }
        }
    }
    false
}

/// Starts the background daemon service or detached daemon process.
fn start_background_daemon() -> Result<(), String> {
    service::sync_installed_binary();

    let mut cfg = Config::load_or_default(None);
    cfg.auto_extreme_enabled = true;
    let _ = cfg.save(None);

    #[cfg(target_os = "linux")]
    {
        if Path::new("/etc/systemd/system/wattwarden.service").exists() {
            let _ = Command::new("systemctl")
                .args(["restart", "wattwarden.service"])
                .status();
            if daemon_active() {
                return Ok(());
            }
        }
    }

    if daemon_active() {
        return Ok(());
    }

    // Spawn detached daemon process with stdio redirected to /var/log/wattwarden.log
    // so the child process never writes on the caller's terminal.
    let _ = spawn::spawn_detached_daemon();
    Ok(())
}

/// Stops the background daemon process or system service.
fn stop_background_daemon() {
    let mut cfg = Config::load_or_default(None);
    cfg.auto_extreme_enabled = false;
    let _ = cfg.save(None);

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("systemctl")
            .args(["stop", "wattwarden.service"])
            .status();
    }

    let pid = PidManager::new();
    if let Some(p) = pid.read_pid() {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(p),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    pid.release();
}

/// Builds the backend, degrading gracefully instead of aborting the process
/// (`AGENTS.md`: the app must boot on a desktop/server/Pi/container). If the primary
/// probe chain cannot initialize, we relocate it to an empty root so every control
/// reports its `N/A`/fallback value rather than killing the app.
fn build_backend() -> Result<LinuxBackend, String> {
    match LinuxBackend::new() {
        Ok(b) => Ok(b),
        Err(e) => {
            eprintln!("Warning: hardware backend initialization failed: {e}");
            eprintln!(
                "Warning: continuing with a degraded fallback backend (controls report N/A)."
            );
            #[cfg(target_os = "linux")]
            {
                LinuxBackend::with_root(wattwarden_platform::SysfsRoot::new(
                    "/nonexistent/wattwarden-fallback",
                ))
                .map_err(|_| "No backend implementation available for this OS.".to_string())
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err("No backend implementation available for this OS.".to_string())
            }
        }
    }
}

/// Runs the daemon loop in the foreground with tracing output routed to
/// `/var/log/wattwarden.log`, keeping stdio clean for terminal supervisors.
fn run_daemon() -> Result<(), String> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(DaemonLogWriter)
                .with_ansi(false),
        )
        .try_init()
        .ok();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    runtime.block_on(async {
        let backend = build_backend()?;
        let config = Config::load_or_default(None);
        DaemonRunner::new(backend, config)
            .run()
            .await
            .map_err(|e| e.to_string())
    })
}

/// Launches the interactive terminal dashboard.
fn launch_dashboard() -> Result<(), String> {
    let backend = build_backend()?;
    let config = Config::load_or_default(None);
    let app = App::new(backend, config);
    run_tui(app).map_err(|e| e.to_string())
}

struct RealRuntime;

impl CliRuntime for RealRuntime {
    fn is_root(&self) -> bool {
        privilege::is_root()
    }
    fn daemon_active(&self) -> bool {
        daemon_active()
    }
    fn auto_brightness(&self) -> bool {
        Config::load_or_default(None).auto_brightness
    }
    fn set_auto_brightness(&self, enabled: bool) {
        let mut cfg = Config::load_or_default(None);
        cfg.auto_brightness = enabled;
        let _ = cfg.save(None);
    }
    fn sync_installed_binary(&self) {
        service::sync_installed_binary();
    }
    fn start_background_daemon(&self) -> Result<(), String> {
        start_background_daemon()
    }
    fn stop_background_daemon(&self) {
        stop_background_daemon();
    }
    fn install_service(&self) -> Result<(), String> {
        service::install_service()
    }
    fn uninstall_service(&self) -> Result<(), String> {
        service::uninstall_service()
    }
    fn run_daemon(&self) -> Result<(), String> {
        run_daemon()
    }
    fn launch_dashboard(&self) -> Result<(), String> {
        launch_dashboard()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    let code = run(&args, &mut out, &RealRuntime);
    let _ = out.flush();
    std::process::exit(code);
}
