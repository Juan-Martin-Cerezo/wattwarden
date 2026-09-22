use std::process::Command;
use wattwarden_core::*;

pub struct WindowsBattery;

impl PowerSource for WindowsBattery {
    fn battery_percentage(&self) -> Result<u8> {
        // Query battery status via PowerShell CIM instance
        if let Ok(output) = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance -ClassName Win32_Battery).EstimatedChargeRemaining",
            ])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Ok(pct) = s.trim().parse::<u8>() {
                return Ok(pct.min(100));
            }
        }
        // Fallback for desktops without battery
        Ok(100)
    }

    fn is_charging(&self) -> Result<bool> {
        if let Ok(output) = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance -ClassName Win32_Battery).BatteryStatus",
            ])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Ok(st) = s.trim().parse::<u32>() {
                return Ok(st == 2 || st == 6 || st == 7 || st == 8 || st == 9);
            }
        }
        Ok(true)
    }

    fn consumption_watts(&self) -> Result<f64> {
        Ok(0.0)
    }

    fn time_remaining(&self) -> Result<String> {
        if self.is_charging()? {
            return Ok("Charging (AC)".into());
        }
        Ok("AC Mains (Stationary)".into())
    }

    fn is_stationary(&self) -> bool {
        if let Ok(output) = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance -ClassName Win32_Battery).EstimatedChargeRemaining",
            ])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            s.trim().is_empty()
        } else {
            true
        }
    }
}

pub struct WindowsCpu;

impl CpuGovernor for WindowsCpu {
    fn num_cpus(&self) -> usize {
        std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
    }

    fn online_cores(&self) -> Result<usize> {
        Ok(self.num_cpus())
    }

    fn set_online_cores(&self, _count: usize) -> Result<()> {
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

    fn set_turbo_enabled(&self, enabled: bool) -> Result<()> {
        let val = if enabled { "2" } else { "0" };
        let _ = Command::new("powercfg")
            .args([
                "-setacvalueindex",
                "SCHEME_CURRENT",
                "SUB_PROCESSOR",
                "PERFBOOSTMODE",
                val,
            ])
            .status();
        let _ = Command::new("powercfg")
            .args(["-setactive", "SCHEME_CURRENT"])
            .status();
        Ok(())
    }

    fn energy_performance_preference(&self) -> Result<String> {
        Ok("balanced".into())
    }

    fn set_energy_performance_preference(&self, _pref: &str) -> Result<()> {
        Ok(())
    }
}

impl WindowsCpu {
    pub fn cstates(&self) -> Result<Vec<CStateInfo>> {
        Ok(Vec::new())
    }
}

impl CStateTelemetry for WindowsCpu {
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        self.cstates()
    }
}

pub struct WindowsDisplay;

impl DisplayManager for WindowsDisplay {
    fn brightness_percent(&self) -> Result<u8> {
        if let Ok(output) = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness).CurrentBrightness",
            ])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Ok(pct) = s.trim().parse::<u8>() {
                return Ok(pct.min(100));
            }
        }
        Ok(100)
    }

    fn set_brightness_percent(&self, percent: u8) -> Result<()> {
        let p = percent.clamp(1, 100);
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1, {})", p),
            ])
            .status();
        Ok(())
    }
}

pub struct WindowsThreshold;

impl ChargeThreshold for WindowsThreshold {
    fn supports_threshold(&self) -> bool {
        false
    }

    fn charge_threshold(&self) -> Result<u8> {
        Err(WattWardenError::Unsupported(
            "Windows battery thresholds managed via vendor software (Lenovo Vantage, MyASUS, Dell Command)".into(),
        ))
    }

    fn set_charge_threshold(&self, _threshold: u8) -> Result<()> {
        Err(WattWardenError::Unsupported(
            "Windows battery thresholds managed via vendor software (Lenovo Vantage, MyASUS, Dell Command)".into(),
        ))
    }
}

pub struct WindowsPeripherals;

impl PeripheralsController for WindowsPeripherals {
    fn kbd_backlight(&self) -> Result<bool> {
        Ok(false)
    }

    fn set_kbd_backlight(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn bluetooth_enabled(&self) -> Result<bool> {
        Ok(true)
    }

    fn set_bluetooth_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    fn wifi_enabled(&self) -> Result<bool> {
        Ok(true)
    }

    fn set_wifi_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }
}

pub struct WindowsTweaks;

impl SystemTweaksController for WindowsTweaks {
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
        Ok(())
    }
}

pub struct WindowsCompositor;

impl CompositorFocus for WindowsCompositor {
    fn active_window_class(&self) -> Option<String> {
        None
    }
}

pub struct WindowsBackend {
    pub battery: WindowsBattery,
    pub cpu: WindowsCpu,
    pub rapl: Option<Box<dyn RaplController>>,
    pub gpu: Option<Box<dyn GpuController>>,
    pub aspm: Option<Box<dyn AspmController>>,
    pub backlight: Option<WindowsDisplay>,
    pub threshold: WindowsThreshold,
    pub peripherals: WindowsPeripherals,
    pub tweaks: WindowsTweaks,
    pub hyprland: WindowsCompositor,
}

impl WindowsBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            battery: WindowsBattery,
            cpu: WindowsCpu,
            rapl: None,
            gpu: None,
            aspm: None,
            backlight: Some(WindowsDisplay),
            threshold: WindowsThreshold,
            peripherals: WindowsPeripherals,
            tweaks: WindowsTweaks,
            hyprland: WindowsCompositor,
        })
    }

    /// Approximate CPU load, expressed in the units Linux `/proc/loadavg` reports so
    /// the shared adaptive ladder can normalize it by the CPU count.
    ///
    /// Windows has no load average, so Go `getWinLoad()` measures
    /// `\Processor Information(_Total)\% Processor Time` with `typeperf` and returns
    /// the 0.0..1.0 fraction its caller uses directly; multiplying that fraction by
    /// the CPU count makes the ladder's `load / num_cpus` reproduce the exact same
    /// power level. `0.0` — the idle step — when `typeperf` is unavailable or its
    /// output is unparsable, exactly like Go.
    pub fn load_average(&self) -> f64 {
        if let Ok(output) = Command::new("typeperf")
            .args([
                r"\Processor Information(_Total)\% Processor Time",
                "-sc",
                "1",
            ])
            .output()
        {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Some(value) = s.lines().nth(2).and_then(|line| line.split(',').nth(1)) {
                if let Ok(percent) = value.trim().trim_matches('"').parse::<f64>() {
                    return percent / 100.0 * self.cpu.num_cpus() as f64;
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
                // SCHEME_MAX: Power Saver GUID (a1841308-3541-4fab-bc81-f71556f20b4a)
                let _ = Command::new("powercfg")
                    .args(["-setactive", "a1841308-3541-4fab-bc81-f71556f20b4a"])
                    .status();
            }
            PowerProfile::Performance => {
                // SCHEME_MIN: High Performance GUID (8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c)
                let _ = Command::new("powercfg")
                    .args(["-setactive", "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"])
                    .status();
            }
            PowerProfile::Normal | PowerProfile::AutoExtreme => {
                // SCHEME_BALANCED: Balanced GUID (381b4222-f694-41f0-9685-ff5bb260df2e)
                let _ = Command::new("powercfg")
                    .args(["-setactive", "381b4222-f694-41f0-9685-ff5bb260df2e"])
                    .status();
            }
        }
        Ok(())
    }
}
