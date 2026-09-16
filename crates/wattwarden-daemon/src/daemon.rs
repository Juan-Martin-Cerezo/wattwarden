use crate::pid::PidManager;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::{info, warn};
use wattwarden_core::*;
use wattwarden_platform_linux::{HyprlandIpc, LinuxBackend, NetlinkUeventListener};

pub struct DaemonRunner {
    backend: Arc<LinuxBackend>,
    config: Config,
    pid_mgr: PidManager,
}

impl DaemonRunner {
    pub fn new(backend: LinuxBackend, config: Config) -> Self {
        Self {
            backend: Arc::new(backend),
            config,
            pid_mgr: PidManager::new(),
        }
    }

    pub async fn run(self) -> Result<()> {
        self.pid_mgr.acquire()?;
        info!("WattWarden daemon started successfully with PID {}", std::process::id());

        // Apply saved charge limit threshold if configured
        if let Some(limit) = self.config.battery_charge_limit {
            if self.backend.threshold.supports_threshold() {
                if let Err(e) = self.backend.threshold.set_charge_threshold(limit) {
                    warn!("Failed to apply battery charge threshold: {}", e);
                } else {
                    info!("Battery charge threshold locked at {}%", limit);
                }
            }
        }

        // Apply configured profile
        let _ = self.backend.apply_profile(&self.config.profile);

        let (event_tx, mut event_rx) = mpsc::channel::<String>(32);

        // Spawn Netlink uevent background listener thread (zero-polling AC/battery events)
        let netlink_tx = event_tx.clone();
        std::thread::spawn(move || {
            if let Ok(listener) = NetlinkUeventListener::new() {
                loop {
                    match listener.wait_for_power_event() {
                        Ok(event) => {
                            let _ = netlink_tx.blocking_send(event);
                        }
                        Err(e) => {
                            warn!("Netlink error: {}", e);
                            std::thread::sleep(Duration::from_secs(5));
                        }
                    }
                }
            }
        });

        // Spawn Hyprland socket2 async listener if available
        let backend_hypr = Arc::clone(&self.backend);
        let config_hypr = self.config.clone();
        tokio::spawn(async move {
            if let Some(socket_path) = HyprlandIpc::discover_event_socket() {
                if let Ok(stream) = UnixStream::connect(socket_path).await {
                    let mut reader = tokio::io::BufReader::new(stream).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        if line.starts_with("activewindow>>") {
                            let payload = &line["activewindow>>".len()..];
                            let class = payload.split(',').next().unwrap_or("").to_lowercase();
                            Self::handle_window_change(&backend_hypr, &config_hypr, &class);
                        }
                    }
                }
            }
        });

        // Main event loop with signal trapping
        let mut ticker = interval(Duration::from_secs(10));

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Termination signal received. Shutting down WattWarden daemon.");
                    break;
                }
                Some(uevent) = event_rx.recv() => {
                    info!("Kernel power uevent received: {}", uevent.lines().next().unwrap_or(""));
                    self.handle_power_state_change();
                }
                _ = ticker.tick() => {
                    // Periodic maintenance: reload config dynamically
                    let cfg = Config::load_or_default(None);
                    if cfg.auto_extreme_enabled {
                        self.handle_auto_extreme();
                    }
                }
            }
        }

        self.pid_mgr.release();
        Ok(())
    }

    fn handle_window_change(backend: &LinuxBackend, config: &Config, class: &str) {
        if !config.auto_brightness {
            return;
        }

        // If charging, keep display high
        if let Ok(true) = backend.battery.is_charging() {
            return;
        }

        let is_terminal = class.contains("kitty")
            || class.contains("alacritty")
            || class.contains("foot")
            || class.contains("wezterm")
            || class.contains("tmux");

        let target_pct = if is_terminal {
            config.terminal_brightness
        } else {
            config.gui_brightness
        };

        if let Some(bl) = &backend.backlight {
            let _ = bl.set_brightness_percent(target_pct);
        }
    }

    fn handle_power_state_change(&self) {
        let is_charging = self.backend.battery.is_charging().unwrap_or(false);
        let cfg = Config::load_or_default(None);

        if is_charging {
            info!("AC power connected. Switching to full performance baseline.");
            let _ = self.backend.apply_profile(&PowerProfile::Performance);
            if cfg.auto_brightness {
                if let Some(bl) = &self.backend.backlight {
                    let _ = bl.set_brightness_percent(100);
                }
            }
        } else {
            info!("Running on battery. Applying energy conservation profile.");
            if cfg.auto_extreme_enabled {
                let _ = self.backend.apply_profile(&PowerProfile::Extreme);
            } else {
                let _ = self.backend.apply_profile(&cfg.profile);
            }
        }
    }

    fn handle_auto_extreme(&self) {
        if let Ok(false) = self.backend.battery.is_charging() {
            // Read 1-minute load average
            if let Ok(load_content) = std::fs::read_to_string("/proc/loadavg") {
                let first = load_content.split_whitespace().next().unwrap_or("0.0");
                let load: f64 = first.parse().unwrap_or(0.0);
                let num_cpus = self.backend.cpu.num_cpus() as f64;
                let normalized_load = (load / num_cpus).clamp(0.0, 1.0);

                if normalized_load < 0.3 {
                    // Idle workload: aggressive throttling
                    let (min_freq, _) = self.backend.cpu.freq_bounds().unwrap_or((400, 1600));
                    let _ = self.backend.cpu.set_freq_limit(min_freq);
                    let _ = self.backend.cpu.set_online_cores(2.min(self.backend.cpu.num_cpus()));
                } else {
                    // Burst workload: scale up to maintain UI responsiveness
                    let (min_freq, max_freq) = self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let target_freq = (min_freq as f64 + (max_freq - min_freq) as f64 * normalized_load) as u32;
                    let _ = self.backend.cpu.set_freq_limit(target_freq);
                    let _ = self.backend.cpu.set_online_cores(self.backend.cpu.num_cpus());
                }
            }
        }
    }
}
