use crate::pid::PidManager;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI16, Ordering};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{info, warn};
use wattwarden_core::*;
use wattwarden_platform::linux::LinuxBackend;

#[derive(Debug, Clone, PartialEq)]
pub struct LogicStepResult {
    pub is_charging: bool,
    pub discrete_power: f64,
    pub target_cores: usize,
    pub target_freq: u32,
    pub target_gpu: u32,
    pub target_rapl: u32,
    pub target_turbo: bool,
    pub target_epp: &'static str,
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT");
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM");
        let mut sighup = signal(SignalKind::hangup()).expect("install SIGHUP");

        tokio::select! {
            _ = sigint.recv() => {
                info!("SIGINT received. Shutting down WattWarden daemon.");
            }
            _ = sigterm.recv() => {
                info!("SIGTERM received. Shutting down WattWarden daemon.");
            }
            _ = sighup.recv() => {
                info!("SIGHUP received. Shutting down WattWarden daemon.");
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        info!("Termination signal received. Shutting down WattWarden daemon.");
    }
}

pub struct DaemonRunner {
    backend: Arc<LinuxBackend>,
    config_path: Option<PathBuf>,
    pid_mgr: PidManager,
    last_applied_brightness: Arc<AtomicI16>,
}

impl DaemonRunner {
    pub fn new(backend: LinuxBackend, config: Config) -> Self {
        let _ = config;
        Self {
            backend: Arc::new(backend),
            config_path: None,
            pid_mgr: PidManager::new(),
            last_applied_brightness: Arc::new(AtomicI16::new(-1)),
        }
    }

    pub fn with_paths(
        backend: LinuxBackend,
        config_path: Option<PathBuf>,
        pid_path: Option<PathBuf>,
    ) -> Self {
        let pid_mgr = match pid_path {
            Some(p) => PidManager::with_path(p),
            None => PidManager::new(),
        };
        Self {
            backend: Arc::new(backend),
            config_path,
            pid_mgr,
            last_applied_brightness: Arc::new(AtomicI16::new(-1)),
        }
    }

    pub fn backend(&self) -> &LinuxBackend {
        &self.backend
    }

    pub fn config(&self) -> Config {
        Config::load_or_default(self.config_path.as_deref())
    }

    pub fn active_window_class(&self) -> String {
        #[cfg(target_os = "linux")]
        {
            if let Some(c) = self.backend.hyprland.active_window_class() {
                let trimmed = c.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_lowercase();
                }
            }
            if let Ok(out) = std::process::Command::new("xdotool")
                .args(["getactivewindow", "getwindowclassname"])
                .output()
            {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                if !s.is_empty() {
                    return s;
                }
            }
        }
        String::new()
    }

    pub fn apply_brightness_step(&self, active_class_override: Option<&str>) -> Option<u8> {
        let config = self.config();
        if !config.auto_brightness {
            return None;
        }

        let is_charging = self.backend.battery.is_charging().unwrap_or(false);
        if is_charging {
            if let Some(bl) = &self.backend.backlight {
                let current = bl.brightness_percent().unwrap_or(100);
                if current < 80 {
                    let _ = bl.set_brightness_percent(100);
                    self.last_applied_brightness.store(100, Ordering::Relaxed);
                    return Some(100);
                }
            }
            return None;
        }

        let active_class = match active_class_override {
            Some(c) => c.to_lowercase(),
            None => self.active_window_class(),
        };

        // Go backend_linux.go:884-909:
        // isTerminal: class == "" || contains(kitty|foot|alacritty|wezterm|ghostty|xterm) -> 12%
        // isHeavyUI: contains(firefox|chrome|chromium|brave|zen|code|cursor|idea|studio) -> 30%
        // other: 20%
        let is_terminal = active_class.is_empty()
            || active_class.contains("kitty")
            || active_class.contains("foot")
            || active_class.contains("alacritty")
            || active_class.contains("wezterm")
            || active_class.contains("ghostty")
            || active_class.contains("xterm");

        let is_heavy_ui = active_class.contains("firefox")
            || active_class.contains("chrome")
            || active_class.contains("chromium")
            || active_class.contains("brave")
            || active_class.contains("zen")
            || active_class.contains("code")
            || active_class.contains("cursor")
            || active_class.contains("idea")
            || active_class.contains("studio");

        let base_brightness = if is_terminal {
            12
        } else if is_heavy_ui {
            30
        } else {
            20
        };

        let delta = match config.auto_extreme_level {
            AutoExtremeLevel::High => 0,
            AutoExtremeLevel::Medium => 10,
            AutoExtremeLevel::Low => 20,
        };

        let target_brightness = ((base_brightness as i16 + delta).clamp(1, 100)) as u8;

        if let Some(bl) = &self.backend.backlight {
            let current = bl.brightness_percent().unwrap_or(0);
            let last = self.last_applied_brightness.load(Ordering::Relaxed);
            if last != target_brightness as i16 || current != target_brightness {
                let _ = bl.set_brightness_percent(target_brightness);
                self.last_applied_brightness
                    .store(target_brightness as i16, Ordering::Relaxed);
            }
        }
        Some(target_brightness)
    }

    pub fn apply_brightness(&self) {
        self.apply_brightness_step(None);
    }

    pub fn apply_logic_step(&self, config: &Config) -> LogicStepResult {
        let is_charging = self.backend.battery.is_charging().unwrap_or(false);
        let ncpu = self.backend.cpu.num_cpus().max(1);

        if is_charging {
            // Plugged in (AC): baseline of performance — Go backend_linux.go:919-935
            let _ = self.backend.cpu.set_online_cores(ncpu);
            let _ = self.backend.cpu.set_freq_limit(99_999);
            if let Some(rapl) = &self.backend.rapl {
                let _ = rapl.set_pl1_watts(115);
                let _ = rapl.set_pl2_watts(115);
            }
            let _ = self.backend.cpu.set_turbo_enabled(true);
            let _ = self
                .backend
                .cpu
                .set_energy_performance_preference("performance");
            if let Some(gpu) = &self.backend.gpu {
                let _ = gpu.set_gpu_freq(99_999);
            }
            if let Some(aspm) = &self.backend.aspm {
                let _ = aspm.set_aspm_policy("performance");
            }
            let _ = self.backend.tweaks.set_wifi_power_save(false);
            let _ = self.backend.tweaks.set_audio_power_save(false);
            let _ = self.backend.tweaks.set_autosuspend(false);
            let _ = self.backend.tweaks.set_nmi_watchdog(true);
            let _ = self.backend.tweaks.set_vm_writeback_seconds(5); // 500 cs
            if config.auto_brightness {
                if let Some(bl) = &self.backend.backlight {
                    let _ = bl.set_brightness_percent(100);
                }
            }

            LogicStepResult {
                is_charging: true,
                discrete_power: 1.0,
                target_cores: ncpu,
                target_freq: 99_999,
                target_gpu: 99_999,
                target_rapl: 115,
                target_turbo: true,
                target_epp: "performance",
            }
        } else {
            // Battery: adaptive loop — Go backend_linux.go:937-991
            let load_str = self.backend.root().read("proc/loadavg");
            let load: f64 = load_str
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let power_level = (load / ncpu as f64).min(1.0);
            // Discrete quantization into 4 steps: 0, 0.333, 0.667, 1.0
            let discrete_power = (power_level * 3.0).round() / 3.0;

            let level = config.auto_extreme_level;
            let ceiling = match level {
                AutoExtremeLevel::High => 0.4,
                AutoExtremeLevel::Medium => 0.7,
                AutoExtremeLevel::Low => 1.0,
            };

            let (min_cpu, hw_max_cpu) = self.backend.cpu.freq_bounds().unwrap_or((400, 1600));
            let max_cpu = (min_cpu as f64 + (hw_max_cpu as f64 - min_cpu as f64) * ceiling) as u32;

            let (min_gpu, hw_max_gpu) = if let Some(gpu) = &self.backend.gpu {
                gpu.gpu_bounds().unwrap_or((300, 1100))
            } else {
                (300, 1100)
            };
            let max_gpu = (min_gpu as f64 + (hw_max_gpu as f64 - min_gpu as f64) * ceiling) as u32;

            let (min_w, hw_max_w) = if let Some(rapl) = &self.backend.rapl {
                rapl.rapl_bounds().unwrap_or((5, 115))
            } else {
                (5, 115)
            };
            let max_w = (min_w as f64 + (hw_max_w as f64 - min_w as f64) * ceiling) as u32;

            let target_cores = match level {
                AutoExtremeLevel::High => {
                    let max_cores = (ncpu / 2).max(1);
                    let cores = (1.0 + discrete_power * (max_cores - 1) as f64) as usize;
                    cores.max(1)
                }
                AutoExtremeLevel::Medium => {
                    let idle_cores = (ncpu / 2).max(2).min(ncpu);
                    let cores =
                        (idle_cores as f64 + discrete_power * (ncpu - idle_cores) as f64) as usize;
                    cores.max(1).min(ncpu)
                }
                AutoExtremeLevel::Low => ncpu,
            };

            let target_cpu =
                (min_cpu as f64 + discrete_power * (max_cpu as f64 - min_cpu as f64)) as u32;
            let target_gpu =
                (min_gpu as f64 + discrete_power * (max_gpu as f64 - min_gpu as f64)) as u32;
            let target_rapl =
                (min_w as f64 + discrete_power * (max_w as f64 - min_w as f64)) as u32;

            let target_turbo = match level {
                AutoExtremeLevel::High => discrete_power >= 0.8,
                AutoExtremeLevel::Medium => discrete_power >= 0.5,
                AutoExtremeLevel::Low => discrete_power >= 0.5,
            };

            let target_epp = match level {
                AutoExtremeLevel::High => "power",
                AutoExtremeLevel::Medium => "power",
                AutoExtremeLevel::Low => {
                    if discrete_power < 0.5 {
                        "balance_power"
                    } else {
                        "balance_performance"
                    }
                }
            };

            let _ = self.backend.cpu.set_online_cores(target_cores);
            let _ = self.backend.cpu.set_freq_limit(target_cpu);
            if let Some(gpu) = &self.backend.gpu {
                let _ = gpu.set_gpu_freq(target_gpu);
            }
            if let Some(rapl) = &self.backend.rapl {
                let _ = rapl.set_pl1_watts(target_rapl);
                let _ = rapl.set_pl2_watts(target_rapl);
            }
            let _ = self
                .backend
                .cpu
                .set_energy_performance_preference(target_epp);
            let _ = self.backend.cpu.set_turbo_enabled(target_turbo);

            // Peripherals on battery branch — Go backend_linux.go:984-990
            if let Some(aspm) = &self.backend.aspm {
                let _ = aspm.set_aspm_policy("powersave");
            }
            let _ = self.backend.tweaks.set_wifi_power_save(true);
            let _ = self.backend.peripherals.set_kbd_backlight(false);
            let _ = self.backend.tweaks.set_audio_power_save(true);
            let _ = self.backend.tweaks.set_autosuspend(true);
            let _ = self.backend.tweaks.set_nmi_watchdog(false);
            let _ = self.backend.tweaks.set_vm_writeback_seconds(60); // 6000 cs

            LogicStepResult {
                is_charging: false,
                discrete_power,
                target_cores,
                target_freq: target_cpu,
                target_gpu,
                target_rapl,
                target_turbo,
                target_epp,
            }
        }
    }

    pub fn apply_logic(&self) {
        let config = self.config();
        if config.auto_extreme_enabled {
            self.apply_logic_step(&config);
        }
    }

    pub async fn run(self) -> Result<()> {
        self.pid_mgr.acquire()?;
        info!(
            "WattWarden daemon started successfully with PID {}",
            std::process::id()
        );

        let config = self.config();

        // Apply saved charge limit threshold if configured
        if let Some(limit) = config.battery_charge_limit {
            if self.backend.threshold.supports_threshold() {
                if let Err(e) = self.backend.threshold.set_charge_threshold(limit) {
                    warn!("Failed to apply battery charge threshold: {}", e);
                } else {
                    info!("Battery charge threshold locked at {}%", limit);
                }
            }
        }

        // Run immediately once before the loop, exactly as Go does
        self.apply_logic();
        self.apply_brightness();

        let mut logic_ticker = interval(Duration::from_secs(5));
        let mut brightness_ticker = interval(Duration::from_millis(300));

        // Consume the first immediate tick since we ran both immediately above
        logic_ticker.tick().await;
        brightness_ticker.tick().await;

        let shutdown = wait_for_shutdown();
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    break;
                }
                _ = logic_ticker.tick() => {
                    self.apply_logic();
                }
                _ = brightness_ticker.tick() => {
                    self.apply_brightness();
                }
            }
        }

        self.pid_mgr.release();
        info!("WattWarden daemon cleanly stopped.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wattwarden_platform::linux::SysfsRoot;

    const CPU: &str = "sys/devices/system/cpu";
    const RAPL: &str = "sys/class/powercap/intel-rapl:0";
    const DRM: &str = "sys/class/drm";
    const BL: &str = "sys/class/backlight/intel_backlight";
    const BAT: &str = "sys/class/power_supply/BAT0";

    fn write(root: &SysfsRoot, rel: &str, value: &str) {
        let p = root.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, value).unwrap();
    }

    fn read(root: &SysfsRoot, rel: &str) -> String {
        fs::read_to_string(root.path(rel))
            .unwrap()
            .trim()
            .to_string()
    }

    fn fake_intel_laptop(tag: &str) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_daemon_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = SysfsRoot::new(&dir);

        // 8 CPUs (min 400 MHz, max 3500 MHz)
        for i in 0..8 {
            let base = format!("{CPU}/cpu{i}");
            write(
                &root,
                &format!("{base}/cpufreq/cpuinfo_min_freq"),
                "400000\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/cpuinfo_max_freq"),
                "3500000\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_min_freq"),
                "400000\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_max_freq"),
                "3500000\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/energy_performance_preference"),
                "balance_performance\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_governor"),
                "powersave\n",
            );
            if i > 0 {
                write(&root, &format!("{base}/online"), "1\n");
            }
        }
        write(&root, &format!("{CPU}/intel_pstate/no_turbo"), "0\n");

        // RAPL: 2 W .. 60 W
        write(&root, &format!("{RAPL}/min_power_range_uw"), "2000000\n");
        write(&root, &format!("{RAPL}/max_power_range_uw"), "60000000\n");
        write(&root, &format!("{RAPL}/constraint_0_name"), "long_term\n");
        write(&root, &format!("{RAPL}/constraint_1_name"), "short_term\n");
        write(
            &root,
            &format!("{RAPL}/constraint_0_power_limit_uw"),
            "45000000\n",
        );
        write(
            &root,
            &format!("{RAPL}/constraint_1_power_limit_uw"),
            "60000000\n",
        );

        // GPU: card0 300..1100 MHz
        write(&root, &format!("{DRM}/card0/gt_max_freq_mhz"), "1100\n");
        write(&root, &format!("{DRM}/card0/gt_RPn_freq_mhz"), "300\n");
        write(&root, &format!("{DRM}/card0/gt_RP0_freq_mhz"), "1100\n");
        write(&root, &format!("{DRM}/card0/gt_min_freq_mhz"), "300\n");

        // Backlight
        write(&root, &format!("{BL}/max_brightness"), "1000\n");
        write(&root, &format!("{BL}/brightness"), "500\n");

        // Battery
        write(&root, &format!("{BAT}/type"), "Battery\n");
        write(&root, &format!("{BAT}/capacity"), "72\n");
        write(&root, &format!("{BAT}/status"), "Discharging\n");
        write(&root, "sys/class/power_supply/AC/type", "Mains\n");
        write(&root, "sys/class/power_supply/AC/online", "0\n");

        // Peripherals & tweaks
        write(
            &root,
            "sys/class/leds/tpacpi::kbd_backlight/brightness",
            "0\n",
        );
        write(
            &root,
            "sys/class/leds/tpacpi::kbd_backlight/max_brightness",
            "2\n",
        );
        write(
            &root,
            "sys/module/snd_hda_intel/parameters/power_save",
            "0\n",
        );
        write(
            &root,
            "sys/module/snd_hda_intel/parameters/power_save_controller",
            "N\n",
        );
        write(&root, "sys/module/iwlwifi/parameters/power_save", "N\n");
        write(&root, "sys/bus/usb/devices/usb1/power/control", "on\n");
        write(
            &root,
            "sys/bus/pci/devices/0000:00:14.0/power/control",
            "on\n",
        );
        write(&root, "proc/sys/kernel/nmi_watchdog", "1\n");
        write(&root, "proc/sys/vm/dirty_writeback_centisecs", "500\n");
        write(
            &root,
            "sys/module/pcie_aspm/parameters/policy",
            "default [powersave] performance\n",
        );
        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");

        root
    }

    /// (a) a batería con load normalizado 0.0, 0.2, 0.5, 1.0 -> escalones 0 / 0.333 / 0.667 / 1.0
    /// y target_freq, target_cores, target_rapl calculados con el techo 40 %.
    #[test]
    fn test_a_battery_adaptive_quantized_steps_and_40_percent_ceiling() {
        let root = fake_intel_laptop("test_a");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        // Hardware parameters:
        // CPU: 400..3500 -> range 3100. Techo 40% = 400 + 3100*0.4 = 1640. MaxCPU - MinCPU = 1240.
        // Cores: 8 CPUs -> maxCores = 4. Target cores = 1 + dp * 3.
        // RAPL: 2..60 -> range 58. Techo 40% = 2 + int(58*0.4) = 25. MaxW - MinW = 23.
        // GPU: 300..1100 -> range 800. Techo 40% = 300 + 800*0.4 = 620. MaxGPU - MinGPU = 320.

        // Case 1: load = 0.0 (normalized 0.0) -> discrete_power = 0.0
        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
        let res0 = runner.apply_logic_step(&config);
        assert_eq!(res0.discrete_power, 0.0);
        assert_eq!(res0.target_cores, 1);
        assert_eq!(res0.target_freq, 400);
        assert_eq!(res0.target_rapl, 2);
        assert_eq!(res0.target_gpu, 300);
        assert!(!res0.target_turbo);
        assert_eq!(res0.target_epp, "power");

        // Sysfs assertions for step 0.0
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "400000"
        );
        for i in 1..8 {
            assert_eq!(read(&root, &format!("{CPU}/cpu{i}/online")), "0");
        }
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "2000000"
        );
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
            "2000000"
        );
        assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "300");

        // Case 2: load = 1.6 (normalized 1.6/8 = 0.2) -> discrete_power = 1/3 (0.333...)
        write(&root, "proc/loadavg", "1.60 1.60 1.60 1/100 1234\n");
        let res02 = runner.apply_logic_step(&config);
        assert!((res02.discrete_power - (1.0 / 3.0)).abs() < 1e-9);
        // target_cores = 1 + int(1/3 * 3) = 2
        assert_eq!(res02.target_cores, 2);
        // target_freq = 400 + int(1/3 * 1240) = 400 + 413 = 813
        assert_eq!(res02.target_freq, 813);
        // target_rapl = 2 + int(1/3 * 23) = 2 + 7 = 9
        assert_eq!(res02.target_rapl, 9);
        // target_gpu = 300 + int(1/3 * 320) = 300 + 106 = 406
        assert_eq!(res02.target_gpu, 406);
        assert!(!res02.target_turbo);
        assert_eq!(res02.target_epp, "power");

        // Sysfs assertions for step 0.333
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "813000"
        );
        assert_eq!(read(&root, &format!("{CPU}/cpu1/online")), "1");
        for i in 2..8 {
            assert_eq!(read(&root, &format!("{CPU}/cpu{i}/online")), "0");
        }
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "9000000"
        );
        assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "406");

        // Case 3: load = 4.0 (normalized 4.0/8 = 0.5) -> discrete_power = 2/3 (0.667...)
        write(&root, "proc/loadavg", "4.00 4.00 4.00 1/100 1234\n");
        let res05 = runner.apply_logic_step(&config);
        assert!((res05.discrete_power - (2.0 / 3.0)).abs() < 1e-9);
        // target_cores = 1 + int(2/3 * 3) = 3
        assert_eq!(res05.target_cores, 3);
        // target_freq = 400 + int(2/3 * 1240) = 400 + 826 = 1226
        assert_eq!(res05.target_freq, 1226);
        // target_rapl = 2 + int(2/3 * 23) = 2 + 15 = 17
        assert_eq!(res05.target_rapl, 17);
        // target_gpu = 300 + int(2/3 * 320) = 300 + 213 = 513
        assert_eq!(res05.target_gpu, 513);
        assert!(!res05.target_turbo);
        assert_eq!(res05.target_epp, "power");

        // Sysfs assertions for step 0.667
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "1226000"
        );
        assert_eq!(read(&root, &format!("{CPU}/cpu1/online")), "1");
        assert_eq!(read(&root, &format!("{CPU}/cpu2/online")), "1");
        for i in 3..8 {
            assert_eq!(read(&root, &format!("{CPU}/cpu{i}/online")), "0");
        }
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "17000000"
        );
        assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "513");

        // Case 4: load = 8.0 (normalized 8.0/8 = 1.0) -> discrete_power = 1.0
        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");
        let res1 = runner.apply_logic_step(&config);
        assert_eq!(res1.discrete_power, 1.0);
        // target_cores = 1 + 1.0 * 3 = 4
        assert_eq!(res1.target_cores, 4);
        // target_freq = 400 + 1240 = 1640
        assert_eq!(res1.target_freq, 1640);
        // target_rapl = 2 + 23 = 25
        assert_eq!(res1.target_rapl, 25);
        // target_gpu = 300 + 320 = 620
        assert_eq!(res1.target_gpu, 620);
        assert!(res1.target_turbo);
        assert_eq!(res1.target_epp, "power");

        // Sysfs assertions for step 1.0
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "1640000"
        );
        for i in 1..4 {
            assert_eq!(read(&root, &format!("{CPU}/cpu{i}/online")), "1");
        }
        for i in 4..8 {
            assert_eq!(read(&root, &format!("{CPU}/cpu{i}/online")), "0");
        }
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "25000000"
        );
        assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "620");
        assert_eq!(read(&root, &format!("{CPU}/intel_pstate/no_turbo")), "0");

        let _ = fs::remove_dir_all(root.root());
    }

    /// (b) enchufado -> el set completo de la rama AC
    #[test]
    fn test_b_plugged_in_ac_complete_set() {
        let root = fake_intel_laptop("test_b");
        // Plug in AC
        write(&root, "sys/class/power_supply/AC/online", "1\n");

        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: true,
            auto_brightness: true,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        let res = runner.apply_logic_step(&config);
        assert!(res.is_charging);
        assert_eq!(res.target_cores, 8);
        assert_eq!(res.target_freq, 99_999);
        assert_eq!(res.target_rapl, 115);
        assert_eq!(res.target_gpu, 99_999);
        assert!(res.target_turbo);
        assert_eq!(res.target_epp, "performance");

        // Verificación completa de los 13 writes en la rama AC:
        // 1. Cores: todos los 8 cores online
        for i in 1..8 {
            assert_eq!(read(&root, &format!("{CPU}/cpu{i}/online")), "1");
        }
        // 2. FreqLimit: clampeado a hwMax (3500 MHz = 3500000 kHz)
        for i in 0..8 {
            assert_eq!(
                read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_max_freq")),
                "3500000"
            );
        }
        // 3. RAPL PL1 & PL2: 115W clampeado a hwMax (60 W = 60000000 uW)
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "60000000"
        );
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
            "60000000"
        );
        // 4. Turbo: enabled (no_turbo = "0")
        assert_eq!(read(&root, &format!("{CPU}/intel_pstate/no_turbo")), "0");
        // 5. EPP: "performance" y governor "performance" en todos los CPUs
        for i in 0..8 {
            assert_eq!(
                read(
                    &root,
                    &format!("{CPU}/cpu{i}/cpufreq/energy_performance_preference")
                ),
                "performance"
            );
            assert_eq!(
                read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_governor")),
                "performance"
            );
        }
        // 6. GPU: 99999 clampeado a hwMax (1100 MHz)
        assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "1100");
        // 7. ASPM: "performance"
        assert_eq!(
            read(&root, "sys/module/pcie_aspm/parameters/policy"),
            "performance"
        );
        // 8. WiFi power save: false ("N")
        assert_eq!(read(&root, "sys/module/iwlwifi/parameters/power_save"), "N");
        // 9. Audio power save: false ("0" y "N")
        assert_eq!(
            read(&root, "sys/module/snd_hda_intel/parameters/power_save"),
            "0"
        );
        assert_eq!(
            read(
                &root,
                "sys/module/snd_hda_intel/parameters/power_save_controller"
            ),
            "N"
        );
        // 10. Autosuspend: false ("on")
        assert_eq!(read(&root, "sys/bus/usb/devices/usb1/power/control"), "on");
        assert_eq!(
            read(&root, "sys/bus/pci/devices/0000:00:14.0/power/control"),
            "on"
        );
        // 11. Watchdog: true ("1")
        assert_eq!(read(&root, "proc/sys/kernel/nmi_watchdog"), "1");
        // 12. VM writeback: 500 centiseconds
        assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "500");
        // 13. Auto-brightness: 100% (1000)
        assert_eq!(read(&root, &format!("{BL}/brightness")), "1000");

        let _ = fs::remove_dir_all(root.root());
    }

    /// (c) `turbo` sólo en el escalón 1.0 y EPP `power` en todos los escalones
    #[test]
    fn test_c_turbo_only_on_step_1_and_epp_power_all_steps() {
        let root = fake_intel_laptop("test_c");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        let loads = [
            ("0.00", 0.0, false),
            ("1.60", 1.0 / 3.0, false),
            ("4.00", 2.0 / 3.0, false),
            ("8.00", 1.0, true),
        ];

        for (load_str, expected_dp, expected_turbo) in loads {
            write(
                &root,
                "proc/loadavg",
                &format!("{load_str} {load_str} {load_str} 1/100 1234\n"),
            );
            let res = runner.apply_logic_step(&config);
            assert!((res.discrete_power - expected_dp).abs() < 1e-9);
            assert_eq!(
                res.target_turbo, expected_turbo,
                "Turbo mismatch at dp={}",
                expected_dp
            );
            assert_eq!(
                res.target_epp, "power",
                "EPP must be power at dp={}",
                expected_dp
            );

            // Verify in sysfs
            let expected_no_turbo = if expected_turbo { "0" } else { "1" };
            assert_eq!(
                read(&root, &format!("{CPU}/intel_pstate/no_turbo")),
                expected_no_turbo
            );
            assert_eq!(
                read(
                    &root,
                    &format!("{CPU}/cpu0/cpufreq/energy_performance_preference")
                ),
                "power"
            );
            assert_eq!(
                read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_governor")),
                "powersave"
            );
        }

        let _ = fs::remove_dir_all(root.root());
    }

    /// (d) `high` no reescribe turbo/EPP distintos de Go (test de preservación de comportamiento)
    #[test]
    fn test_d_high_level_preserves_go_behavior() {
        let root = fake_intel_laptop("test_d");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        // High must strictly write:
        // - EPP = "power" (never balance_power or balance_performance)
        // - Governor = "powersave"
        // - Turbo = false for loads below 0.8 (steps 0, 1/3, 2/3), true only for step 1.0
        // - ASPM = "powersave", wifi = true, kbd = false, audio = true, autosuspend = true, watchdog = false, vm = 6000
        for load in [0.0, 0.1, 0.3, 0.4, 0.6, 0.7, 0.9, 1.0] {
            let load_total = load * 8.0;
            write(
                &root,
                "proc/loadavg",
                &format!(
                    "{:.2} {:.2} {:.2} 1/100 1234\n",
                    load_total, load_total, load_total
                ),
            );
            let res = runner.apply_logic_step(&config);
            assert_eq!(res.target_epp, "power");
            assert_eq!(
                read(
                    &root,
                    &format!("{CPU}/cpu0/cpufreq/energy_performance_preference")
                ),
                "power"
            );
            assert_eq!(
                read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_governor")),
                "powersave"
            );

            let expected_turbo = res.discrete_power >= 0.8;
            assert_eq!(res.target_turbo, expected_turbo);
            let expected_no_turbo = if expected_turbo { "0" } else { "1" };
            assert_eq!(
                read(&root, &format!("{CPU}/intel_pstate/no_turbo")),
                expected_no_turbo
            );

            // Peripherals must always match Go battery state
            assert_eq!(
                read(&root, "sys/module/pcie_aspm/parameters/policy"),
                "powersave"
            );
            assert_eq!(read(&root, "sys/module/iwlwifi/parameters/power_save"), "Y");
            assert_eq!(
                read(&root, "sys/class/leds/tpacpi::kbd_backlight/brightness"),
                "0"
            );
            assert_eq!(
                read(&root, "sys/module/snd_hda_intel/parameters/power_save"),
                "1"
            );
            assert_eq!(
                read(
                    &root,
                    "sys/module/snd_hda_intel/parameters/power_save_controller"
                ),
                "Y"
            );
            assert_eq!(
                read(&root, "sys/bus/usb/devices/usb1/power/control"),
                "auto"
            );
            assert_eq!(read(&root, "proc/sys/kernel/nmi_watchdog"), "0");
            assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "6000");
        }

        let _ = fs::remove_dir_all(root.root());
    }

    /// Parameterized levels test: medium (ceiling 0.7, turbo from 0.5) and low (ceiling 1.0, turbo from 0.5)
    #[test]
    fn test_medium_and_low_parameterized_levels() {
        let root = fake_intel_laptop("test_levels");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();

        // Medium level: ceiling 0.7
        // CPU: 400 + 3100*0.7 = 400 + 2170 = 2570.
        // At load 1.0: target_freq = 2570. Turbo from 0.5 -> true at load 0.5 and 1.0.
        let cfg_medium = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::Medium,
            ..Default::default()
        };
        let backend_med = LinuxBackend::with_root(root.clone()).unwrap();
        let runner_med = DaemonRunner::with_paths(backend_med, None, None);

        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");
        let res_med_1 = runner_med.apply_logic_step(&cfg_medium);
        assert_eq!(res_med_1.target_freq, 2570);
        assert!(res_med_1.target_turbo);
        assert_eq!(res_med_1.target_epp, "power");

        write(&root, "proc/loadavg", "4.00 4.00 4.00 1/100 1234\n");
        let res_med_05 = runner_med.apply_logic_step(&cfg_medium);
        assert!(res_med_05.target_turbo); // 2/3 >= 0.5

        // Low level: ceiling 1.0
        // CPU: 400 + 3100*1.0 = 3500.
        // All cores online (8). EPP: balance_power at idle, balance_performance at load.
        let cfg_low = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::Low,
            ..Default::default()
        };
        let runner_low = DaemonRunner::with_paths(backend, None, None);

        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");
        let res_low_1 = runner_low.apply_logic_step(&cfg_low);
        assert_eq!(res_low_1.target_freq, 3500);
        assert_eq!(res_low_1.target_cores, 8);
        assert!(res_low_1.target_turbo);
        assert_eq!(res_low_1.target_epp, "balance_performance");

        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
        let res_low_0 = runner_low.apply_logic_step(&cfg_low);
        assert_eq!(res_low_0.target_cores, 8);
        assert_eq!(res_low_0.target_epp, "balance_power");

        let _ = fs::remove_dir_all(root.root());
    }

    /// Brightness test: terminal (12), empty (12), heavy UI (30), other (20) + deltas
    #[test]
    fn test_brightness_logic_matches_go_and_level_deltas() {
        let root = fake_intel_laptop("test_bl");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();

        let tmp_cfg = root.path("config.json");
        let mut cfg = Config {
            auto_extreme_enabled: true,
            auto_brightness: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        cfg.save(Some(&tmp_cfg)).unwrap();

        let runner = DaemonRunner::with_paths(backend, Some(tmp_cfg.clone()), None);

        // On High:
        // Empty class -> terminal -> 12%
        assert_eq!(runner.apply_brightness_step(Some("")), Some(12));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "120"); // 12% of 1000

        // Terminal (kitty) -> 12%
        assert_eq!(runner.apply_brightness_step(Some("kitty")), Some(12));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "120");

        // Heavy UI (firefox) -> 30%
        assert_eq!(runner.apply_brightness_step(Some("firefox")), Some(30));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "300");

        // Normal UI (gimp) -> 20%
        assert_eq!(runner.apply_brightness_step(Some("gimp")), Some(20));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "200");

        // Medium level: +10 delta -> terminal 22, heavy 40, normal 30
        cfg.auto_extreme_level = AutoExtremeLevel::Medium;
        cfg.save(Some(&tmp_cfg)).unwrap();
        assert_eq!(runner.apply_brightness_step(Some("kitty")), Some(22));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "220");
        assert_eq!(runner.apply_brightness_step(Some("firefox")), Some(40));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "400");
        assert_eq!(runner.apply_brightness_step(Some("vlc")), Some(30));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "300");

        // Low level: +20 delta -> terminal 32, heavy 50, normal 40
        cfg.auto_extreme_level = AutoExtremeLevel::Low;
        cfg.save(Some(&tmp_cfg)).unwrap();
        assert_eq!(runner.apply_brightness_step(Some("kitty")), Some(32));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "320");
        assert_eq!(runner.apply_brightness_step(Some("code")), Some(50));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "500");

        let _ = fs::remove_dir_all(root.root());
    }
}
