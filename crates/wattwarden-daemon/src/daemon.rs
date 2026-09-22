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
    pub applied: bool,
    pub discrete_power: f64,
    pub target_cores: usize,
    pub target_freq: u32,
    pub target_gpu: u32,
    pub target_rapl: u32,
    pub target_turbo: bool,
    pub target_epp: &'static str,
}

/// Ceiling of online cores for a level, derived from the discovered CPU count.
///
/// `discrete_power` is quantised into 4 steps (`0`, `1/3`, `2/3`, `1`). The ceiling is
/// `ceil(ncpu * level_ceiling)` clipped to `[2, ncpu]` as soon as `ncpu >= 2`.
///
/// The floor of 2 is the whole point: the previous `(ncpu / 2).max(1)` gave **1** on a
/// 3-core machine (the Dell Vostro), i.e. a flat ramp with zero adaptation. With the
/// floor, `ncpu = 3` produces the ramp `1, 1, 2, 2` (at least two distinct steps) and
/// any larger machine keeps a ceiling proportional to its own core count.
fn core_ceiling(ncpu: usize, level_ceiling: f64) -> usize {
    if ncpu <= 1 {
        return 1;
    }
    ((ncpu as f64 * level_ceiling).ceil() as usize).clamp(2, ncpu)
}

/// `cores = clamp(round(1 + discrete_power * (ceiling - 1)), 1, ceiling)`.
fn core_ramp_cores(ncpu: usize, level_ceiling: f64, discrete_power: f64) -> usize {
    let ceiling = core_ceiling(ncpu, level_ceiling);
    let cores = (1.0 + discrete_power * (ceiling - 1) as f64).round() as usize;
    cores.clamp(1, ceiling)
}

/// Scales `discrete_power` inside the *discovered* range `[min, max]`, capped by the
/// level ceiling: `ceiling = min + (max - min) * level_ceiling`, then
/// `min + discrete_power * (ceiling - min)`. Clamped to `max`, so no value can ever
/// exceed the ceiling the hardware itself declared.
fn scale_within(min: u32, max: u32, level_ceiling: f64, discrete_power: f64) -> u32 {
    let ceiling = min as f64 + (max - min) as f64 * level_ceiling;
    let value = min as f64 + discrete_power * (ceiling - min as f64);
    (value as u32).min(max)
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

        // Juan: brightness follows the user's auto-brightness setting, never the
        // cable. No 100 % forcing when plugged in.
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
        // Juan: the ladder follows the USER flag, never the cable. `is_charging`
        // is still read (UI display + charge threshold stay), but it no longer
        // picks a branch. Disabled mode = respect the config profile: touch nothing.
        let is_charging = self.backend.battery.is_charging().unwrap_or(false);
        let ncpu = self.backend.cpu.num_cpus().max(1);

        if !config.auto_extreme_enabled {
            return LogicStepResult {
                is_charging,
                applied: false,
                discrete_power: 1.0,
                target_cores: ncpu,
                target_freq: 0,
                target_gpu: 0,
                target_rapl: 0,
                target_turbo: true,
                target_epp: "performance",
            };
        }

        // Adaptive ladder, plugged in or not — Go backend_linux.go:937-991 shape.
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

        // Frequency/GPU ranges come from discovery only. When a range cannot be
        // discovered the value stays at 0 and the corresponding setter (gated on the
        // same discovery) writes nothing — no absolute fallback ever reaches a write.
        let (min_cpu, hw_max_cpu) = self.backend.cpu.discovered_freq_bounds().unwrap_or((0, 0));
        let max_cpu = (min_cpu as f64 + (hw_max_cpu as f64 - min_cpu as f64) * ceiling) as u32;

        let (min_gpu, hw_max_gpu) = self
            .backend
            .gpu
            .as_ref()
            .and_then(|gpu| gpu.discovered_gpu_bounds())
            .unwrap_or((0, 0));
        let max_gpu = (min_gpu as f64 + (hw_max_gpu as f64 - min_gpu as f64) * ceiling) as u32;

        // Cores: the ramp is derived from the discovered CPU count, never from a
        // static `(ncpu / 2).max(1)` that collapses on small machines (`core_ramp_cores`).
        let target_cores = core_ramp_cores(ncpu, ceiling, discrete_power);

        let target_cpu =
            (min_cpu as f64 + discrete_power * (max_cpu as f64 - min_cpu as f64)) as u32;
        let target_gpu =
            (min_gpu as f64 + discrete_power * (max_gpu as f64 - min_gpu as f64)) as u32;

        // RAPL: PL1 and PL2 are separate constraints with their own *discovered*
        // ranges. Each target is scaled inside its own range and can never exceed the
        // hardware's `constraint_N_max_power_uw`; a constraint without a discovered
        // range is skipped by the controller (nothing is written).
        let mut target_rapl = 0;
        if let Some(rapl) = &self.backend.rapl {
            if let Some((min_w, max_w)) = rapl.pl1_bounds_watts() {
                let target = scale_within(min_w, max_w, ceiling, discrete_power);
                let _ = rapl.set_pl1_watts(target);
                target_rapl = target;
            }
            if let Some((min_w, max_w)) = rapl.pl2_bounds_watts() {
                let target = scale_within(min_w, max_w, ceiling, discrete_power);
                let _ = rapl.set_pl2_watts(target);
            }
        }

        // Turbo thresholds per approved table (the spec labels the middle steps
        // 0.67/0.33 after 2-decimal rounding; compare against the exact step
        // fractions so step 2/3 counts for Medium and step 1/3 for Low).
        // High stays exactly as the Go-validated `>= 0.8`.
        let target_turbo = match level {
            AutoExtremeLevel::High => discrete_power >= 0.8,
            AutoExtremeLevel::Medium => discrete_power >= 2.0 / 3.0,
            AutoExtremeLevel::Low => discrete_power >= 1.0 / 3.0,
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
        let _ = self
            .backend
            .cpu
            .set_energy_performance_preference(target_epp);
        let _ = self.backend.cpu.set_turbo_enabled(target_turbo);

        // Energy-saving peripherals stay gated on battery: they are power
        // saving, not a mode. Plugged in, the ladder still scales cores/freq/
        // RAPL/GPU, but the machine keeps its normal peripheral behavior.
        if !is_charging {
            if let Some(aspm) = &self.backend.aspm {
                let _ = aspm.set_aspm_policy("powersave");
            }
            let _ = self.backend.tweaks.set_wifi_power_save(true);
            let _ = self.backend.peripherals.set_kbd_backlight(false);
            let _ = self.backend.tweaks.set_audio_power_save(true);
            let _ = self.backend.tweaks.set_autosuspend(true);
            let _ = self.backend.tweaks.set_nmi_watchdog(false);
            let _ = self.backend.tweaks.set_vm_writeback_seconds(60); // 6000 cs
        }

        LogicStepResult {
            is_charging,
            applied: true,
            discrete_power,
            target_cores,
            target_freq: target_cpu,
            target_gpu,
            target_rapl,
            target_turbo,
            target_epp,
        }
    }

    pub fn apply_logic(&self) {
        // Juan: the mode follows the user flag (`auto_extreme_enabled`), on AC
        // or battery. The cable never enables or disables anything by itself.
        let config = self.config();
        self.apply_logic_step(&config);
    }

    /// Arranque opt-in puro (Juan): el daemon solo escribe lo que el usuario
    /// habilitó explícitamente. Sin ningún ajuste, esto no toca nada: ni
    /// perfil, ni umbral de carga, ni nada más.
    pub fn apply_boot_settings(&self, config: &Config) {
        if let Some(profile) = &config.profile {
            if let Err(e) = self.backend.apply_profile(profile) {
                warn!("Failed to apply power profile {profile}: {e}");
            } else {
                info!("Power profile applied: {profile}");
            }
        }

        if let Some(limit) = config.battery_charge_limit {
            if self.backend.threshold.supports_threshold() {
                if let Err(e) = self.backend.threshold.set_charge_threshold(limit) {
                    warn!("Failed to apply battery charge threshold: {}", e);
                } else {
                    info!("Battery charge threshold locked at {}%", limit);
                }
            }
        }
    }

    pub async fn run(self) -> Result<()> {
        self.pid_mgr.acquire()?;
        info!(
            "WattWarden daemon started successfully with PID {}",
            std::process::id()
        );

        let config = self.config();

        // Opt-in puro: solo se escribe lo que el usuario habilitó explícitamente
        // (perfil y/o umbral de carga). Sin ajustes, no se toca nada.
        self.apply_boot_settings(&config);

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

        // RAPL: 2 W .. 60 W, with the per-constraint range the kernel really exposes.
        write(&root, &format!("{RAPL}/min_power_range_uw"), "2000000\n");
        write(&root, &format!("{RAPL}/max_power_range_uw"), "60000000\n");
        write(&root, &format!("{RAPL}/constraint_0_name"), "long_term\n");
        write(&root, &format!("{RAPL}/constraint_1_name"), "short_term\n");
        write(
            &root,
            &format!("{RAPL}/constraint_0_min_power_uw"),
            "2000000\n",
        );
        write(
            &root,
            &format!("{RAPL}/constraint_0_max_power_uw"),
            "60000000\n",
        );
        write(
            &root,
            &format!("{RAPL}/constraint_1_min_power_uw"),
            "2000000\n",
        );
        write(
            &root,
            &format!("{RAPL}/constraint_1_max_power_uw"),
            "60000000\n",
        );
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

    /// (a) Escalera High a batería, byte a byte como Go: load normalizado
    /// 0.0, 0.2, 0.5, 1.0 -> escalones 0 / 0.333 / 0.667 / 1.0 con techo 40 %.
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

    /// Juan: la escalera corre enchufado o no. El viejo restore de Go a 115 W
    /// ya no existe: con la misma carga da el mismo resultado que a bateria.
    #[test]
    fn test_e_plugged_in_runs_ladder_not_go_restore() {
        // Dell Vostro shape: RAPL range 0..115 W so the Go clamp lands on 115 W
        // in both constraints, like the real machine in the A/B report.
        let dir = std::env::temp_dir().join(format!("ww_daemon_test_e_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = SysfsRoot::new(&dir);

        for i in 0..4 {
            let base = format!("{CPU}/cpu{i}");
            write(
                &root,
                &format!("{base}/cpufreq/cpuinfo_min_freq"),
                "400000\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/cpuinfo_max_freq"),
                "4200000\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_min_freq"),
                "400000\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_max_freq"),
                "4200000\n",
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
        write(&root, &format!("{CPU}/intel_pstate/no_turbo"), "1\n");

        // Each constraint declares its own ceiling through `constraint_N_max_power_uw`;
        // the 115 W here is that discovered ceiling, not a literal baked anywhere.
        write(&root, &format!("{RAPL}/min_power_range_uw"), "0\n");
        write(&root, &format!("{RAPL}/max_power_range_uw"), "115000000\n");
        write(&root, &format!("{RAPL}/constraint_0_name"), "long_term\n");
        write(&root, &format!("{RAPL}/constraint_1_name"), "short_term\n");
        write(
            &root,
            &format!("{RAPL}/constraint_0_max_power_uw"),
            "115000000\n",
        );
        write(
            &root,
            &format!("{RAPL}/constraint_1_max_power_uw"),
            "115000000\n",
        );
        write(
            &root,
            &format!("{RAPL}/constraint_0_power_limit_uw"),
            "45000000\n",
        );
        write(
            &root,
            &format!("{RAPL}/constraint_1_power_limit_uw"),
            "65000000\n",
        );

        // Plugged in: Go `IsCharging` sees `Mains` + `online == "1"`
        // (`backend_linux.go:165-178`).
        write(&root, &format!("{BAT}/type"), "Battery\n");
        write(&root, &format!("{BAT}/status"), "Charging\n");
        write(&root, "sys/class/power_supply/AC/type", "Mains\n");
        write(&root, "sys/class/power_supply/AC/online", "1\n");

        // The plugged-in step now runs the ladder: assert it writes the ladder
        // values for the simulated idle load, NOT the old Go 115 W restore.
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        assert!(backend.battery.is_charging().unwrap());
        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
        let config = Config {
            auto_extreme_enabled: true,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);
        let res = runner.apply_logic_step(&config);
        assert!(res.is_charging);
        assert!(res.applied);
        assert_eq!(res.discrete_power, 0.0);
        assert_eq!(res.target_cores, 1);
        assert_eq!(res.target_freq, 400);
        assert_eq!(res.target_rapl, 0);
        // This fake has no DRM card at all: with no discovered GPU range nothing is
        // written and the reported target is 0 (never an invented 300 MHz default).
        assert_eq!(res.target_gpu, 0);
        assert!(!res.target_turbo);
        assert_eq!(res.target_epp, "power");

        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "0"
        );
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
            "0"
        );
        for i in 0..4 {
            assert_eq!(
                read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_governor")),
                "powersave"
            );
        }

        let _ = fs::remove_dir_all(root.root());
    }

    /// (b) Sin modo habilitado no se adapta nada: se respeta el perfil de
    /// config y no se toca el hardware. `is_charging` se sigue reportando.
    #[test]
    fn test_f_disabled_mode_touches_nothing() {
        let root = fake_intel_laptop("test_f");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: false,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");
        let before_freq = read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq"));
        let before_rapl = read(&root, &format!("{RAPL}/constraint_0_power_limit_uw"));
        let res = runner.apply_logic_step(&config);
        assert!(!res.is_charging);
        assert!(!res.applied);
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            before_freq
        );
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            before_rapl
        );

        let _ = fs::remove_dir_all(root.root());
    }

    /// (g) Enchufado, la escalera da el mismo resultado que a batería para la
    /// misma carga: el cable no decide nada.
    #[test]
    fn test_g_ladder_same_plugged_and_unplugged() {
        let root = fake_intel_laptop("test_g");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        let mut on_ac = Vec::new();
        let mut on_battery = Vec::new();
        for load in ["0.00", "1.60", "4.00", "8.00"] {
            write(
                &root,
                "proc/loadavg",
                &format!("{load} {load} {load} 1/100 1234\n"),
            );
            write(&root, "sys/class/power_supply/AC/online", "1\n");
            on_ac.push(runner.apply_logic_step(&config));

            write(
                &root,
                "proc/loadavg",
                &format!("{load} {load} {load} 1/100 1234\n"),
            );
            write(&root, "sys/class/power_supply/AC/online", "0\n");
            on_battery.push(runner.apply_logic_step(&config));
        }
        assert_eq!(on_ac.len(), 4);
        for (ac, bat) in on_ac.iter().zip(on_battery.iter()) {
            assert!(ac.is_charging, "AC=1 must still report charging");
            assert!(!bat.is_charging, "AC=0 must still report discharging");
            assert_eq!(ac.discrete_power, bat.discrete_power);
            assert_eq!(ac.target_cores, bat.target_cores);
            assert_eq!(ac.target_freq, bat.target_freq);
            assert_eq!(ac.target_gpu, bat.target_gpu);
            assert_eq!(ac.target_rapl, bat.target_rapl);
            assert_eq!(ac.target_turbo, bat.target_turbo);
            assert_eq!(ac.target_epp, bat.target_epp);
        }

        let _ = fs::remove_dir_all(root.root());
    }

    /// (h) Regresión: con el cable puesto, el daemon NO escribe el máximo del
    /// hardware; escribe lo que dice la escalera (idle -> mínimos).
    #[test]
    fn test_h_plugged_in_writes_ladder_not_hw_max() {
        let root = fake_intel_laptop("test_h");
        write(&root, "sys/class/power_supply/AC/online", "1\n");
        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");

        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        assert!(backend.battery.is_charging().unwrap());
        let config = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        let res = runner.apply_logic_step(&config);
        assert!(res.applied);
        assert_eq!(res.target_freq, 400);
        assert_eq!(res.target_cores, 1);
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "400000"
        );
        assert_ne!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "3500000"
        );
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "2000000"
        );
        assert_ne!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "60000000"
        );
        assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "300");

        let _ = fs::remove_dir_all(root.root());
    }

    /// (i) Enchufado, los periféricos de ahorro NO se tocan (son ahorro
    /// energético, no un modo); a batería sí.
    #[test]
    fn test_i_saving_peripherals_only_on_battery() {
        let root = fake_intel_laptop("test_i");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
        write(&root, "sys/class/power_supply/AC/online", "1\n");
        let res_ac = runner.apply_logic_step(&config);
        assert!(res_ac.is_charging);
        assert_eq!(read(&root, "sys/module/iwlwifi/parameters/power_save"), "N");
        assert_eq!(read(&root, "proc/sys/kernel/nmi_watchdog"), "1");
        assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "500");

        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
        write(&root, "sys/class/power_supply/AC/online", "0\n");
        let res_bat = runner.apply_logic_step(&config);
        assert!(!res_bat.is_charging);
        assert_eq!(read(&root, "sys/module/iwlwifi/parameters/power_save"), "Y");
        assert_eq!(read(&root, "proc/sys/kernel/nmi_watchdog"), "0");
        assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "6000");

        let _ = fs::remove_dir_all(root.root());
    }

    /// (b) Los tres niveles con valores exactos en los 4 escalones
    /// (cores + freq + rapl + gpu + turbo + epp), a batería Y enchufado con el
    /// mismo resultado. Hardware falso: 8 CPUs 400..3500 MHz, RAPL 2..60 W,
    /// GPU 300..1100 MHz.
    /// High (techo 0.4): max_cpu 1640, max_w 25, max_gpu 620, cores 1->4.
    /// Medium (techo 0.7): max_cpu 2570, max_w 42, max_gpu 860, cores 1->8.
    /// Low (techo 1.0): max_cpu 3500, max_w 60, max_gpu 1100, cores 1->8.
    #[test]
    fn test_b_plugged_in_ac_complete_set() {
        struct Case {
            load: &'static str,
            dp_num: f64,
            dp_den: f64,
            cores: usize,
            freq: u32,
            rapl: u32,
            gpu: u32,
            turbo: bool,
            epp: &'static str,
        }
        let high_cases = [
            Case {
                load: "0.00",
                dp_num: 0.0,
                dp_den: 1.0,
                cores: 1,
                freq: 400,
                rapl: 2,
                gpu: 300,
                turbo: false,
                epp: "power",
            },
            Case {
                load: "1.60",
                dp_num: 1.0,
                dp_den: 3.0,
                cores: 2,
                freq: 813,
                rapl: 9,
                gpu: 406,
                turbo: false,
                epp: "power",
            },
            Case {
                load: "4.00",
                dp_num: 2.0,
                dp_den: 3.0,
                cores: 3,
                freq: 1226,
                rapl: 17,
                gpu: 513,
                turbo: false,
                epp: "power",
            },
            Case {
                load: "8.00",
                dp_num: 1.0,
                dp_den: 1.0,
                cores: 4,
                freq: 1640,
                rapl: 25,
                gpu: 620,
                turbo: true,
                epp: "power",
            },
        ];
        let medium_cases = [
            Case {
                load: "0.00",
                dp_num: 0.0,
                dp_den: 1.0,
                cores: 1,
                freq: 400,
                rapl: 2,
                gpu: 300,
                turbo: false,
                epp: "power",
            },
            Case {
                load: "1.60",
                dp_num: 1.0,
                dp_den: 3.0,
                cores: 3,
                freq: 1123,
                rapl: 15,
                gpu: 486,
                turbo: false,
                epp: "power",
            },
            Case {
                load: "4.00",
                dp_num: 2.0,
                dp_den: 3.0,
                // Core ceiling = ceil(8 * 0.7) = 6 -> ramp 1/3/4/6.
                cores: 4,
                freq: 1846,
                rapl: 29,
                gpu: 673,
                turbo: true,
                epp: "power",
            },
            Case {
                load: "8.00",
                dp_num: 1.0,
                dp_den: 1.0,
                cores: 6,
                freq: 2570,
                rapl: 42,
                gpu: 860,
                turbo: true,
                epp: "power",
            },
        ];
        let low_cases = [
            Case {
                load: "0.00",
                dp_num: 0.0,
                dp_den: 1.0,
                cores: 1,
                freq: 400,
                rapl: 2,
                gpu: 300,
                turbo: false,
                epp: "balance_power",
            },
            Case {
                load: "1.60",
                dp_num: 1.0,
                dp_den: 3.0,
                cores: 3,
                freq: 1433,
                rapl: 21,
                gpu: 566,
                turbo: true,
                epp: "balance_power",
            },
            Case {
                load: "4.00",
                dp_num: 2.0,
                dp_den: 3.0,
                // Core ceiling = ceil(8 * 1.0) = 8 -> ramp 1/3/6/8.
                cores: 6,
                freq: 2466,
                rapl: 40,
                gpu: 833,
                turbo: true,
                epp: "balance_performance",
            },
            Case {
                load: "8.00",
                dp_num: 1.0,
                dp_den: 1.0,
                cores: 8,
                freq: 3500,
                rapl: 60,
                gpu: 1100,
                turbo: true,
                epp: "balance_performance",
            },
        ];
        let levels = [
            (AutoExtremeLevel::High, &high_cases[..]),
            (AutoExtremeLevel::Medium, &medium_cases[..]),
            (AutoExtremeLevel::Low, &low_cases[..]),
        ];

        for (level, cases) in levels {
            for ac in ["0", "1"] {
                let root = fake_intel_laptop(&format!("test_b_{level}_{ac}"));
                write(
                    &root,
                    "sys/class/power_supply/AC/online",
                    &format!("{ac}\n"),
                );
                let backend = LinuxBackend::with_root(root.clone()).unwrap();
                let config = Config {
                    auto_extreme_enabled: true,
                    auto_extreme_level: level,
                    ..Default::default()
                };
                let runner = DaemonRunner::with_paths(backend, None, None);

                for c in cases {
                    write(
                        &root,
                        "proc/loadavg",
                        &format!("{} {} {} 1/100 1234\n", c.load, c.load, c.load),
                    );
                    let res = runner.apply_logic_step(&config);
                    let expected_dp = c.dp_num / c.dp_den;
                    assert!(
                        (res.discrete_power - expected_dp).abs() < 1e-9,
                        "{level} AC={ac} load={}: dp {} != {expected_dp}",
                        c.load,
                        res.discrete_power,
                    );
                    assert_eq!(
                        res.target_cores, c.cores,
                        "{level} AC={ac} load={} cores",
                        c.load
                    );
                    assert_eq!(
                        res.target_freq, c.freq,
                        "{level} AC={ac} load={} freq",
                        c.load
                    );
                    assert_eq!(
                        res.target_rapl, c.rapl,
                        "{level} AC={ac} load={} rapl",
                        c.load
                    );
                    assert_eq!(res.target_gpu, c.gpu, "{level} AC={ac} load={} gpu", c.load);
                    assert_eq!(
                        res.target_turbo, c.turbo,
                        "{level} AC={ac} load={} turbo",
                        c.load
                    );
                    assert_eq!(res.target_epp, c.epp, "{level} AC={ac} load={} epp", c.load);
                    assert_eq!(
                        res.is_charging,
                        ac == "1",
                        "{level} AC={ac} load={} is_charging",
                        c.load
                    );
                    assert!(
                        res.applied,
                        "{level} AC={ac} load={}: ladder must apply",
                        c.load
                    );
                    assert_eq!(
                        read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
                        (c.freq * 1000).to_string(),
                        "{level} AC={ac} load={} sysfs freq",
                        c.load
                    );
                }

                let _ = fs::remove_dir_all(root.root());
            }
        }
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

    /// Niveles medium/low con la tabla aprobada: techo 0.7/1.0, cores 1->ncpu,
    /// turbo Medium >= 2/3 y Low >= 1/3, EPP Low balance_* por umbral 0.5.
    #[test]
    fn test_medium_and_low_parameterized_levels() {
        let root = fake_intel_laptop("test_levels");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();

        // Medium level: ceiling 0.7
        // CPU: 400 + 3100*0.7 = 400 + 2170 = 2570.
        // Turbo >= 2/3 -> false en 1/3, true en 2/3 y 1.0.
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
        assert_eq!(res_med_1.target_cores, 6);
        assert_eq!(res_med_1.target_rapl, 42);
        assert!(res_med_1.target_turbo);
        assert_eq!(res_med_1.target_epp, "power");

        write(&root, "proc/loadavg", "4.00 4.00 4.00 1/100 1234\n");
        let res_med_05 = runner_med.apply_logic_step(&cfg_medium);
        assert!(res_med_05.target_turbo); // 2/3 >= 2/3

        write(&root, "proc/loadavg", "1.60 1.60 1.60 1/100 1234\n");
        let res_med_033 = runner_med.apply_logic_step(&cfg_medium);
        assert!(!res_med_033.target_turbo); // 1/3 < 2/3
        assert_eq!(res_med_033.target_cores, 3);

        // Low level: ceiling 1.0
        // CPU: 400 + 3100*1.0 = 3500. Turbo >= 1/3. EPP: balance_power si
        // dp < 0.5, balance_performance si no.
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

        write(&root, "proc/loadavg", "1.60 1.60 1.60 1/100 1234\n");
        let res_low_033 = runner_low.apply_logic_step(&cfg_low);
        assert_eq!(res_low_033.target_cores, 3);
        assert!(res_low_033.target_turbo); // 1/3 >= 1/3
        assert_eq!(res_low_033.target_epp, "balance_power");

        write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
        let res_low_0 = runner_low.apply_logic_step(&cfg_low);
        assert_eq!(res_low_0.target_cores, 1);
        assert!(!res_low_0.target_turbo);
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

    /// Juan: auto-brightness nunca fuerza 100 % por estar enchufado; adapta por
    /// ventana activa con AC/online = 1 igual que con 0.
    #[test]
    fn test_brightness_never_forces_100_when_plugged_in() {
        let root = fake_intel_laptop("test_bl_ac");
        let backend = LinuxBackend::with_root(root.clone()).unwrap();

        let tmp_cfg = root.path("config.json");
        let cfg = Config {
            auto_extreme_enabled: true,
            auto_brightness: true,
            auto_extreme_level: AutoExtremeLevel::High,
            ..Default::default()
        };
        cfg.save(Some(&tmp_cfg)).unwrap();

        let runner = DaemonRunner::with_paths(backend, Some(tmp_cfg), None);

        for ac in ["0", "1"] {
            write(
                &root,
                "sys/class/power_supply/AC/online",
                &format!("{ac}\n"),
            );
            assert_eq!(runner.apply_brightness_step(Some("kitty")), Some(12));
            assert_eq!(read(&root, &format!("{BL}/brightness")), "120");
            assert_eq!(runner.apply_brightness_step(Some("firefox")), Some(30));
            assert_eq!(read(&root, &format!("{BL}/brightness")), "300");
            assert_eq!(runner.apply_brightness_step(Some("gimp")), Some(20));
            assert_eq!(read(&root, &format!("{BL}/brightness")), "200");
            assert_ne!(read(&root, &format!("{BL}/brightness")), "1000");
        }

        let _ = fs::remove_dir_all(root.root());
    }

    fn snapshot_sysfs(root: &SysfsRoot) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut out = std::collections::BTreeMap::new();
        let base = root.root();
        for entry in walkdir_files(base) {
            if let Ok(bytes) = fs::read(&entry) {
                out.insert(entry, bytes);
            }
        }
        out
    }

    fn walkdir_files(base: &std::path::Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut dirs = vec![base.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    // El config.json del runner vive dentro del root falso: no es
                    // hardware, se excluye del snapshot.
                    dirs.push(p);
                } else if p.file_name().and_then(|n| n.to_str()) != Some("config.json") {
                    files.push(p);
                }
            }
        }
        files
    }

    fn optin_runner(root: &SysfsRoot, config: &Config) -> DaemonRunner {
        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let tmp_cfg = root.path("config.json");
        config.save(Some(&tmp_cfg)).unwrap();
        DaemonRunner::with_paths(backend, Some(tmp_cfg), None)
    }

    /// Test estrella (Juan, opt-in puro): config por defecto, recién creada,
    /// sin tocar nada -> el daemon es de SOLO LECTURA. Ningún archivo de
    /// hardware cambia y no aparece ninguno nuevo.
    #[test]
    fn test_optin_default_config_writes_nothing() {
        let root = fake_intel_laptop("optin_star");
        // Umbrales falsos presentes pero sin valor pedido: no se tocan.
        write(
            &root,
            "sys/class/power_supply/BAT0/charge_control_end_threshold",
            "80\n",
        );
        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");

        let config = Config::default();
        assert!(!config.auto_extreme_enabled);
        assert!(!config.auto_brightness);
        assert_eq!(config.profile, None);
        assert_eq!(config.battery_charge_limit, None);

        let runner = optin_runner(&root, &config);
        let before = snapshot_sysfs(&root);

        runner.apply_boot_settings(&runner.config());
        let res = runner.apply_logic_step(&runner.config());
        assert!(!res.applied);
        assert_eq!(runner.apply_brightness_step(Some("firefox")), None);

        let after = snapshot_sysfs(&root);
        assert_eq!(before, after, "default config must not write anything");

        let _ = fs::remove_dir_all(root.root());
    }

    /// `profile: Some(p)` habilita solo sus escrituras (perfil Extreme).
    #[test]
    fn test_optin_profile_only() {
        let root = fake_intel_laptop("optin_profile");
        // Carga alta: si la escalera corriera escribiría valores altos; el
        // perfil Extreme escribe mínimos. Así se distingue quién escribió.
        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");
        let config = Config {
            profile: Some(PowerProfile::Extreme),
            ..Default::default()
        };
        let runner = optin_runner(&root, &config);
        let before = snapshot_sysfs(&root);

        runner.apply_boot_settings(&runner.config());
        let res = runner.apply_logic_step(&runner.config());
        assert!(!res.applied);
        assert_eq!(runner.apply_brightness_step(Some("firefox")), None);

        let after = snapshot_sysfs(&root);
        let changed: Vec<_> = before
            .iter()
            .filter(|(p, b)| after.get(*p) != Some(*b))
            .map(|(p, _)| {
                p.strip_prefix(root.root())
                    .unwrap_or(p)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(
            !changed.is_empty(),
            "applying an explicit profile must write something"
        );
        // El perfil Extreme escribe mínimos y su propio brillo (10 %): la
        // escalera está apagada (a carga 8.0 habría escrito valores altos) y
        // el lazo de brillo también (el 10 % es del perfil, no del lazo).
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "400000"
        );
        assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "300");
        assert_eq!(read(&root, &format!("{BL}/brightness")), "100");

        let _ = fs::remove_dir_all(root.root());
    }

    /// `auto_extreme_enabled` habilita solo la escalera adaptativa.
    #[test]
    fn test_optin_auto_extreme_only() {
        let root = fake_intel_laptop("optin_ladder");
        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");
        let config = Config {
            auto_extreme_enabled: true,
            ..Default::default()
        };
        let runner = optin_runner(&root, &config);

        let res = runner.apply_logic_step(&runner.config());
        assert!(res.applied);
        assert_ne!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "3500000"
        );
        assert_eq!(runner.apply_brightness_step(Some("firefox")), None);
        assert_eq!(read(&root, &format!("{BL}/brightness")), "500");

        let _ = fs::remove_dir_all(root.root());
    }

    /// `auto_brightness` habilita solo el lazo de brillo.
    #[test]
    fn test_optin_auto_brightness_only() {
        let root = fake_intel_laptop("optin_bl");
        write(&root, "proc/loadavg", "8.00 8.00 8.00 1/100 1234\n");
        let config = Config {
            auto_brightness: true,
            ..Default::default()
        };
        let runner = optin_runner(&root, &config);

        let res = runner.apply_logic_step(&runner.config());
        assert!(!res.applied);
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "3500000"
        );
        assert_eq!(runner.apply_brightness_step(Some("firefox")), Some(30));
        assert_eq!(read(&root, &format!("{BL}/brightness")), "300");

        let _ = fs::remove_dir_all(root.root());
    }

    /// `battery_charge_limit: Some(n)` fija el umbral; `None` no lo toca.
    #[test]
    fn test_optin_charge_limit_only() {
        let root = fake_intel_laptop("optin_limit");
        write(
            &root,
            "sys/class/power_supply/BAT0/charge_control_end_threshold",
            "80\n",
        );

        let with_limit = Config {
            battery_charge_limit: Some(60),
            ..Default::default()
        };
        let runner = optin_runner(&root, &with_limit);
        runner.apply_boot_settings(&runner.config());
        assert_eq!(
            read(
                &root,
                "sys/class/power_supply/BAT0/charge_control_end_threshold"
            ),
            "60"
        );

        write(
            &root,
            "sys/class/power_supply/BAT0/charge_control_end_threshold",
            "80\n",
        );
        let without_limit = Config::default();
        assert_eq!(without_limit.battery_charge_limit, None);
        let runner = optin_runner(&root, &without_limit);
        runner.apply_boot_settings(&runner.config());
        assert_eq!(
            read(
                &root,
                "sys/class/power_supply/BAT0/charge_control_end_threshold"
            ),
            "80"
        );

        let _ = fs::remove_dir_all(root.root());
    }

    /// Compatibilidad: `"profile": "Normal"` explícito sigue aplicándose en
    /// arranque (round-trip de config + escrituras del perfil).
    #[test]
    fn test_optin_explicit_normal_profile_still_applies() {
        let root = fake_intel_laptop("optin_compat");
        let tmp_cfg = root.path("config.json");
        fs::write(&tmp_cfg, r#"{"profile":"Normal"}"#).unwrap();
        let loaded = Config::load_or_default(Some(&tmp_cfg));
        assert_eq!(loaded.profile, Some(PowerProfile::Normal));

        let runner = optin_runner(&root, &loaded);
        runner.apply_boot_settings(&runner.config());
        assert_eq!(
            read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
            "3500000"
        );

        let _ = fs::remove_dir_all(root.root());
    }

    /// Fake Dell built from the measured table: `cpus` cores, a `min..max` kHz range
    /// and a PL1 RAPL ceiling in microwatts. PL2 is deliberately left unexposed (0),
    /// exactly like both real Dells.
    fn fake_machine(
        tag: &str,
        cpus: usize,
        min_khz: u64,
        max_khz: u64,
        pl1_max_uw: u64,
    ) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_daemon_hw_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = SysfsRoot::new(&dir);

        for i in 0..cpus {
            let base = format!("{CPU}/cpu{i}");
            write(
                &root,
                &format!("{base}/cpufreq/cpuinfo_min_freq"),
                &format!("{min_khz}\n"),
            );
            write(
                &root,
                &format!("{base}/cpufreq/cpuinfo_max_freq"),
                &format!("{max_khz}\n"),
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_min_freq"),
                &format!("{min_khz}\n"),
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_max_freq"),
                &format!("{max_khz}\n"),
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

        write(&root, &format!("{RAPL}/min_power_range_uw"), "0\n");
        write(
            &root,
            &format!("{RAPL}/max_power_range_uw"),
            &format!("{pl1_max_uw}\n"),
        );
        write(&root, &format!("{RAPL}/constraint_0_name"), "long_term\n");
        write(
            &root,
            &format!("{RAPL}/constraint_0_max_power_uw"),
            &format!("{pl1_max_uw}\n"),
        );
        write(
            &root,
            &format!("{RAPL}/constraint_0_power_limit_uw"),
            &format!("{pl1_max_uw}\n"),
        );
        write(&root, &format!("{RAPL}/constraint_1_name"), "short_term\n");
        write(&root, &format!("{RAPL}/constraint_1_max_power_uw"), "0\n");
        write(
            &root,
            &format!("{RAPL}/constraint_1_power_limit_uw"),
            "1234000000\n",
        );

        write(&root, &format!("{DRM}/card1/gt_RPn_freq_mhz"), "300\n");
        write(&root, &format!("{DRM}/card1/gt_RP0_freq_mhz"), "1500\n");
        write(&root, &format!("{DRM}/card1/gt_min_freq_mhz"), "300\n");
        write(&root, &format!("{DRM}/card1/gt_max_freq_mhz"), "1500\n");

        write(&root, &format!("{BL}/max_brightness"), "1000\n");
        write(&root, &format!("{BL}/brightness"), "500\n");

        write(&root, &format!("{BAT}/type"), "Battery\n");
        write(&root, &format!("{BAT}/status"), "Discharging\n");
        write(&root, "sys/class/power_supply/AC/type", "Mains\n");
        write(&root, "sys/class/power_supply/AC/online", "0\n");

        root
    }

    /// Same `discrete_power`, different machine: every written value scales with the
    /// discovered hardware and RAPL can never exceed `constraint_0_max_power_uw`.
    #[test]
    fn test_hw_agnostic_scales_per_machine_and_never_exceeds() {
        let cases = [
            ("vostro", 3usize, 400_000u64, 1_600_000u64, 15_000_000u64),
            ("g15", 20, 400_000, 4_600_000, 45_000_000),
        ];

        for (tag, cpus, min_khz, max_khz, pl1_max_uw) in cases {
            let root = fake_machine(tag, cpus, min_khz, max_khz, pl1_max_uw);
            let backend = LinuxBackend::with_root(root.clone()).unwrap();
            let config = Config {
                auto_extreme_enabled: true,
                auto_extreme_level: AutoExtremeLevel::High,
                ..Default::default()
            };
            let runner = DaemonRunner::with_paths(backend, None, None);

            // dp = 0 -> the discovered minimums.
            write(&root, "proc/loadavg", "0.00 0.00 0.00 1/1 1234\n");
            let idle = runner.apply_logic_step(&config);
            assert_eq!(idle.discrete_power, 0.0);
            assert_eq!(idle.target_cores, 1, "{tag}: idle cores");
            assert_eq!(idle.target_freq, 400, "{tag}: idle freq");
            assert_eq!(idle.target_rapl, 0, "{tag}: idle RAPL");

            // dp = 1 -> the level ceiling of *this* machine's discovered range.
            let load = format!("{cpus}.00");
            write(
                &root,
                "proc/loadavg",
                &format!("{load} {load} {load} 1/1 1234\n"),
            );
            let busy = runner.apply_logic_step(&config);
            assert_eq!(busy.discrete_power, 1.0);

            let min_mhz = (min_khz / 1000) as u32;
            let max_mhz = (max_khz / 1000) as u32;
            let expected_freq = (min_mhz as f64 + (max_mhz - min_mhz) as f64 * 0.4) as u32;
            assert_eq!(
                busy.target_freq, expected_freq,
                "{tag}: freq scales with the hw range"
            );

            let expected_cores = ((cpus as f64 * 0.4).ceil() as usize).clamp(2, cpus);
            assert_eq!(
                busy.target_cores, expected_cores,
                "{tag}: core ramp scales with ncpu"
            );

            let pl1_max_w = (pl1_max_uw / 1_000_000) as u32;
            let expected_rapl = (pl1_max_w as f64 * 0.4) as u32;
            assert_eq!(
                busy.target_rapl, expected_rapl,
                "{tag}: RAPL scales with the ceiling"
            );
            assert!(
                busy.target_rapl <= pl1_max_w,
                "{tag}: RAPL stays within the ceiling"
            );

            // The written constraint is exactly the reported target, inside range.
            let written: u64 = read(&root, &format!("{RAPL}/constraint_0_power_limit_uw"))
                .parse()
                .unwrap();
            assert_eq!(written, busy.target_rapl as u64 * 1_000_000);
            assert!(written <= pl1_max_uw, "{tag}: never above max_power_uw");

            // PL2 has no discovered range -> left untouched.
            assert_eq!(
                read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
                "1234000000",
                "{tag}: PL2 without a range must not be written"
            );

            let _ = fs::remove_dir_all(root.root());
        }
    }

    /// Corazón del arreglo (trinquete), a nivel daemon: `scaling_max_freq` quedó
    /// inflado en 2.4 GHz mientras `cpuinfo_max_freq` declara 1.6 GHz. La escalera
    /// descubre el rango SÓLO de `cpuinfo_*`, así que con el techo al 100 % (nivel Low)
    /// y carga máxima escribe exactamente 1600000 — nunca 2400000 — y seguir iterando
    /// no lo infla.
    #[test]
    fn test_freq_ratchet_never_exceeds_discovered_max() {
        let root = fake_machine("ratchet", 3, 400_000, 1_600_000, 15_000_000);
        for i in 0..3 {
            write(
                &root,
                &format!("{CPU}/cpu{i}/cpufreq/scaling_max_freq"),
                "2400000\n",
            );
        }

        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        assert_eq!(backend.cpu.discovered_freq_bounds(), Some((400, 1600)));
        let config = Config {
            auto_extreme_enabled: true,
            auto_extreme_level: AutoExtremeLevel::Low, // techo 100 % del rango hw
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        write(&root, "proc/loadavg", "3.00 3.00 3.00 1/3 1234\n");
        let res = runner.apply_logic_step(&config);
        assert_eq!(res.discrete_power, 1.0);
        assert_eq!(res.target_freq, 1600, "el techo es 1600 MHz, no 2400");
        for i in 0..3 {
            let written = read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_max_freq"));
            assert_eq!(written, "1600000", "cpu{i}: exactamente el máximo real");
            assert!(
                written.parse::<u64>().unwrap() <= 1_600_000,
                "cpu{i}: nunca por encima de 1.6 GHz"
            );
        }

        // Iterar no infla el rango: los `cpuinfo_*` no cambian con las escrituras.
        for load in ["0.00", "1.00", "2.00", "3.00"] {
            write(
                &root,
                "proc/loadavg",
                &format!("{load} {load} {load} 1/3 1234\n"),
            );
            let res = runner.apply_logic_step(&config);
            assert!(res.target_freq <= 1600);
            for i in 0..3 {
                let written: u64 = read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_max_freq"))
                    .parse()
                    .unwrap();
                assert!(
                    written <= 1_600_000,
                    "load={load} cpu{i}: {written} > 1.6 GHz"
                );
            }
        }

        let _ = fs::remove_dir_all(root.root());
    }

    /// The core ramp always has steps, even on a 3-core machine (the old
    /// `(ncpu / 2).max(1)` collapsed to a flat 1).
    #[test]
    fn test_core_ramp_has_at_least_two_distinct_steps_on_three_cores() {
        let steps: Vec<usize> = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
            .iter()
            .map(|dp| core_ramp_cores(3, 0.4, *dp))
            .collect();
        assert_eq!(steps, vec![1, 1, 2, 2]);

        let mut distinct = steps.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(distinct.len() >= 2, "the ramp must not be flat");
        assert_eq!(distinct, vec![1, 2]);

        // A 2-core machine still moves; a single-core machine cannot.
        assert_eq!(core_ramp_cores(2, 0.4, 1.0), 2);
        assert_eq!(core_ramp_cores(1, 0.4, 1.0), 1);

        // A big machine keeps a proportional ramp with four distinct steps.
        let big: Vec<usize> = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
            .iter()
            .map(|dp| core_ramp_cores(20, 0.4, *dp))
            .collect();
        assert_eq!(big, vec![1, 3, 6, 8]);
    }

    /// A RAPL constraint without `max_power_uw` is skipped by the ladder: nothing is
    /// written and the reported target is 0.
    #[test]
    fn test_rapl_not_written_when_constraint_range_is_missing() {
        let root = fake_machine("norange", 4, 400_000, 1_600_000, 15_000_000);
        fs::remove_file(root.path(&format!("{RAPL}/constraint_0_max_power_uw"))).unwrap();

        let backend = LinuxBackend::with_root(root.clone()).unwrap();
        let config = Config {
            auto_extreme_enabled: true,
            ..Default::default()
        };
        let runner = DaemonRunner::with_paths(backend, None, None);

        write(&root, "proc/loadavg", "4.00 4.00 4.00 1/4 1234\n");
        let res = runner.apply_logic_step(&config);
        assert_eq!(res.target_rapl, 0);
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
            "15000000"
        );

        let _ = fs::remove_dir_all(root.root());
    }
}
