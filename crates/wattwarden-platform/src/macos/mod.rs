use std::process::Command;
use wattwarden_core::*;

pub struct MacOsBattery;

impl PowerSource for MacOsBattery {
    fn battery_percentage(&self) -> Result<u8> {
        if let Ok(output) = Command::new("pmset").args(["-g", "batt"]).output() {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Some(idx) = s.find('%') {
                let start = idx.saturating_sub(3);
                let slice = &s[start..idx];
                let num_str: String = slice.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(pct) = num_str.parse::<u8>() {
                    return Ok(pct.min(100));
                }
            }
        }
        // If no battery is found (e.g. Mac Mini, Mac Studio, Mac Pro, iMac)
        Ok(100)
    }

    fn is_charging(&self) -> Result<bool> {
        if let Ok(output) = Command::new("pmset").args(["-g", "batt"]).output() {
            let s = String::from_utf8_lossy(&output.stdout);
            if s.contains("AC Power") && !s.contains("discharging") {
                return Ok(true);
            }
            if s.contains("discharging") {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn consumption_watts(&self) -> Result<f64> {
        if let Ok(output) = Command::new("ioreg")
            .args(["-rn", "AppleSmartBattery"])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            let mut current = 0.0;
            let mut voltage = 0.0;
            for line in s.lines() {
                if line.contains("\"Current\" =") {
                    if let Some(val) = line.split('=').nth(1) {
                        current = val.trim().parse::<f64>().unwrap_or(0.0).abs();
                    }
                } else if line.contains("\"Voltage\" =") {
                    if let Some(val) = line.split('=').nth(1) {
                        voltage = val.trim().parse::<f64>().unwrap_or(0.0).abs();
                    }
                }
            }
            if current > 0.0 && voltage > 0.0 {
                return Ok(current * voltage / 1_000_000.0);
            }
        }
        Ok(0.0)
    }

    fn time_remaining(&self) -> Result<String> {
        if self.is_charging()? {
            return Ok("Charging (AC)".into());
        }
        if let Ok(output) = Command::new("pmset").args(["-g", "batt"]).output() {
            let s = String::from_utf8_lossy(&output.stdout);
            for word in s.split_whitespace() {
                if word.contains(':') && word.len() >= 4 {
                    let parts: Vec<&str> = word.split(':').collect();
                    if parts.len() == 2 {
                        if let (Ok(h), Ok(m)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                            return Ok(format!("{}h {:02}m", h, m));
                        }
                    }
                }
            }
        }
        Ok("AC Mains (Stationary)".into())
    }

    fn is_stationary(&self) -> bool {
        if let Ok(output) = Command::new("pmset").args(["-g", "batt"]).output() {
            let s = String::from_utf8_lossy(&output.stdout);
            !s.contains("InternalBattery")
        } else {
            true
        }
    }
}

pub struct MacOsCpu;

impl CpuGovernor for MacOsCpu {
    fn num_cpus(&self) -> usize {
        std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
    }

    fn online_cores(&self) -> Result<usize> {
        Ok(self.num_cpus())
    }

    fn set_online_cores(&self, _count: usize) -> Result<()> {
        // macOS does not allow unprivileged core offlining
        Ok(())
    }

    fn freq_bounds(&self) -> Result<(u32, u32)> {
        Ok((1000, 4500))
    }

    fn freq_limit(&self) -> Result<u32> {
        Ok(3500)
    }

    fn set_freq_limit(&self, _mhz: u32) -> Result<()> {
        Ok(())
    }

    fn turbo_enabled(&self) -> Result<bool> {
        Ok(true)
    }

    fn set_turbo_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn energy_performance_preference(&self) -> Result<String> {
        Ok("balance".into())
    }

    fn set_energy_performance_preference(&self, _pref: &str) -> Result<()> {
        Ok(())
    }
}

impl MacOsCpu {
    pub fn cstates(&self) -> Result<Vec<CStateInfo>> {
        Ok(Vec::new())
    }
}

impl CStateTelemetry for MacOsCpu {
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        self.cstates()
    }
}

pub struct MacOsDisplay;

impl DisplayManager for MacOsDisplay {
    fn brightness_percent(&self) -> Result<u8> {
        if let Ok(output) = Command::new("brightness").arg("-l").output() {
            let s = String::from_utf8_lossy(&output.stdout);
            for line in s.lines() {
                if line.contains("brightness") {
                    if let Some(val_str) = line.split_whitespace().last() {
                        if let Ok(f) = val_str.parse::<f64>() {
                            return Ok((f * 100.0).round().clamp(1.0, 100.0) as u8);
                        }
                    }
                }
            }
        }
        Ok(100)
    }

    fn set_brightness_percent(&self, percent: u8) -> Result<()> {
        let p = (percent.clamp(1, 100) as f64) / 100.0;
        let _ = Command::new("brightness").arg(format!("{:.2}", p)).status();
        Ok(())
    }
}

pub struct MacOsThreshold;

impl ChargeThreshold for MacOsThreshold {
    fn supports_threshold(&self) -> bool {
        false
    }

    fn charge_threshold(&self) -> Result<u8> {
        Err(WattWardenError::Unsupported(
            "macOS battery thresholds managed natively via Optimized Battery Charging".into(),
        ))
    }

    fn set_charge_threshold(&self, _threshold: u8) -> Result<()> {
        Err(WattWardenError::Unsupported(
            "macOS battery thresholds managed natively via Optimized Battery Charging".into(),
        ))
    }
}

pub struct MacOsPeripherals;

impl PeripheralsController for MacOsPeripherals {
    fn kbd_backlight(&self) -> Result<bool> {
        Ok(false)
    }

    fn set_kbd_backlight(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn bluetooth_enabled(&self) -> Result<bool> {
        if let Ok(output) = Command::new("defaults")
            .args([
                "read",
                "/Library/Preferences/com.apple.Bluetooth",
                "ControllerPowerState",
            ])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            return Ok(s.trim() != "0");
        }
        Ok(true)
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()> {
        let val = if enabled { "1" } else { "0" };
        let _ = Command::new("defaults")
            .args([
                "write",
                "/Library/Preferences/com.apple.Bluetooth",
                "ControllerPowerState",
                val,
            ])
            .status();
        Ok(())
    }

    fn wifi_enabled(&self) -> Result<bool> {
        Ok(true)
    }

    fn set_wifi_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
}

pub struct MacOsTweaks;

impl SystemTweaksController for MacOsTweaks {
    fn wifi_power_save(&self) -> Result<bool> {
        Ok(false)
    }

    fn set_wifi_power_save(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn audio_power_save(&self) -> Result<bool> {
        Ok(false)
    }

    fn set_audio_power_save(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn autosuspend(&self) -> Result<bool> {
        Ok(true)
    }

    fn set_autosuspend(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn nmi_watchdog(&self) -> Result<bool> {
        Ok(false)
    }

    fn set_nmi_watchdog(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn vm_writeback_seconds(&self) -> Result<u32> {
        Ok(5)
    }

    fn set_vm_writeback_seconds(&self, _seconds: u32) -> Result<()> {
        Ok(())
    }

    fn process_purge(&self) -> Result<()> {
        let _ = Command::new("purge").status();
        Ok(())
    }
}

pub struct MacOsCompositor;

impl CompositorFocus for MacOsCompositor {
    fn active_window_class(&self) -> Option<String> {
        None
    }
}

pub struct MacOsBackend {
    pub battery: MacOsBattery,
    pub cpu: MacOsCpu,
    pub rapl: Option<Box<dyn RaplController>>,
    pub gpu: Option<Box<dyn GpuController>>,
    pub aspm: Option<Box<dyn AspmController>>,
    pub backlight: Option<MacOsDisplay>,
    pub threshold: MacOsThreshold,
    pub peripherals: MacOsPeripherals,
    pub tweaks: MacOsTweaks,
    pub hyprland: MacOsCompositor,
}

impl MacOsBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            battery: MacOsBattery,
            cpu: MacOsCpu,
            rapl: None,
            gpu: None,
            aspm: None,
            backlight: Some(MacOsDisplay),
            threshold: MacOsThreshold,
            peripherals: MacOsPeripherals,
            tweaks: MacOsTweaks,
            hyprland: MacOsCompositor,
        })
    }

    /// 1-minute load average from `sysctl -n vm.loadavg`, in the units Linux
    /// `/proc/loadavg` reports so the shared adaptive ladder can normalize it by the
    /// CPU count (Go `getMacLoad()` divides by `NumCPU` and its caller uses that
    /// directly, which is the same power level). `0.0` — the idle step — when the
    /// query fails, exactly like Go.
    pub fn load_average(&self) -> f64 {
        if let Ok(output) = Command::new("sysctl").args(["-n", "vm.loadavg"]).output() {
            let s = String::from_utf8_lossy(&output.stdout);
            let cleaned = s.trim().trim_matches(['{', '}']).trim();
            if let Some(first) = cleaned.split_whitespace().next() {
                if let Ok(value) = first.parse::<f64>() {
                    return value;
                }
            }
        }
        0.0
    }

    pub fn capabilities(&self) -> HardwareCapabilities {
        HardwareCapabilities {
            has_battery: !self.battery.is_stationary(),
            is_stationary_mains: self.battery.is_stationary(),
            has_cpu_frequency_control: false,
            has_cpu_core_control: false,
            has_rapl: false,
            has_gpu_control: false,
            has_backlight_control: true,
            has_charge_threshold: false,
            has_peripherals_control: true,
            has_system_tweaks: true,
            has_compositor_focus: false,
        }
    }

    pub fn apply_profile(&self, profile: &PowerProfile) -> Result<()> {
        match profile {
            PowerProfile::Extreme => {
                let _ = Command::new("pmset")
                    .args(["-a", "lowpowermode", "1"])
                    .status();
                let _ = Command::new("pmset")
                    .args(["-a", "displaysleep", "5"])
                    .status();
                let _ = self.tweaks.process_purge();
            }
            PowerProfile::Performance | PowerProfile::Normal | PowerProfile::AutoExtreme => {
                let _ = Command::new("pmset")
                    .args(["-a", "lowpowermode", "0"])
                    .status();
                let _ = Command::new("pmset")
                    .args(["-a", "displaysleep", "15"])
                    .status();
            }
        }
        Ok(())
    }
}
