use crate::pid::PidManager;
use std::sync::Arc;
#[cfg(target_os = "linux")]
use tokio::io::AsyncBufReadExt;
#[cfg(target_os = "linux")]
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::{info, warn};
use wattwarden_core::*;
use wattwarden_platform::PlatformBackend as LinuxBackend;

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
        info!(
            "WattWarden daemon started successfully with PID {}",
            std::process::id()
        );

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

        let (_event_tx, mut event_rx) = mpsc::channel::<String>(32);

        #[cfg(target_os = "linux")]
        {
            use wattwarden_platform::linux::{HyprlandIpc, NetlinkUeventListener};
            // Spawn Netlink uevent background listener thread (zero-polling AC/battery events)
            let netlink_tx = _event_tx.clone();
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
            tokio::spawn(async move {
                if let Some(socket_path) = HyprlandIpc::discover_event_socket() {
                    if let Ok(stream) = UnixStream::connect(socket_path).await {
                        let mut reader = tokio::io::BufReader::new(stream).lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            if let Some(payload) = line.strip_prefix("activewindow>>") {
                                let class = payload.split(',').next().unwrap_or("").to_lowercase();
                                Self::handle_window_change(&backend_hypr, &class);
                            }
                        }
                    }
                }
            });
        }

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

    #[cfg(target_os = "linux")]
    fn handle_window_change(backend: &LinuxBackend, class: &str) {
        let config = Config::load_or_default(None);
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

        let params = config.auto_extreme_level.params();
        let base_pct = if is_terminal {
            config.terminal_brightness
        } else {
            config.gui_brightness
        };
        let target_pct = (base_pct as i16 + params.brightness_delta).clamp(0, 100) as u8;

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
                let ncpu = self.backend.cpu.num_cpus();
                let normalized_load = (load / ncpu as f64).clamp(0.0, 1.0);

                // Re-read the adaptive level every tick so changes apply without a restart
                let params = Config::load_or_default(None).auto_extreme_level.params();

                if normalized_load < params.idle_threshold {
                    // Idle workload: aggressive floor
                    let (min_freq, _) = self.backend.cpu.freq_bounds().unwrap_or((400, 1600));
                    let _ = self.backend.cpu.set_freq_limit(min_freq);
                    let idle_cores = if params.idle_cores == 0 {
                        ncpu
                    } else {
                        params.idle_cores.min(ncpu)
                    };
                    let _ = self.backend.cpu.set_online_cores(idle_cores);
                    if params.manage_power_hints {
                        let turbo_on = normalized_load >= params.turbo_from;
                        let _ = self.backend.cpu.set_turbo_enabled(turbo_on);
                        let _ = self
                            .backend
                            .cpu
                            .set_energy_performance_preference(params.epp_idle);
                    }
                    if let Some(gpu) = &self.backend.gpu {
                        let (min_g, _) = gpu.gpu_bounds().unwrap_or((300, 1100));
                        let _ = gpu.set_gpu_freq(min_g);
                    }
                    if let Some(rapl) = &self.backend.rapl {
                        let (min_w, _) = rapl.rapl_bounds().unwrap_or((5, 15));
                        let _ = rapl.set_pl1_watts(min_w);
                        let _ = rapl.set_pl2_watts(min_w);
                    }
                } else {
                    // Burst workload: scale up across the full hardware range
                    let (min_freq, max_freq) =
                        self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let target_freq =
                        (min_freq as f64 + (max_freq - min_freq) as f64 * normalized_load) as u32;
                    let _ = self.backend.cpu.set_freq_limit(target_freq);
                    let _ = self.backend.cpu.set_online_cores(ncpu);
                    if params.manage_power_hints {
                        let turbo_on = normalized_load >= params.turbo_from;
                        let _ = self.backend.cpu.set_turbo_enabled(turbo_on);
                        let _ = self
                            .backend
                            .cpu
                            .set_energy_performance_preference(params.epp_load);
                    }
                    if let Some(gpu) = &self.backend.gpu {
                        let (min_g, max_g) = gpu.gpu_bounds().unwrap_or((300, 1100));
                        let target_g =
                            (min_g as f64 + (max_g - min_g) as f64 * normalized_load) as u32;
                        let _ = gpu.set_gpu_freq(target_g);
                    }
                    if let Some(rapl) = &self.backend.rapl {
                        let (min_w, max_w) = rapl.rapl_bounds().unwrap_or((5, 45));
                        let target_w =
                            (min_w as f64 + (max_w - min_w) as f64 * normalized_load) as u32;
                        let _ = rapl.set_pl1_watts(target_w);
                        let _ = rapl.set_pl2_watts(target_w);
                    }
                }
            }
        }
    }
}
