use std::sync::atomic::{AtomicUsize, Ordering};
use wattwarden_core::*;

/// Universal Fallback Power Source for stationary computers or unrecognised operating systems
#[derive(Debug, Clone, Default)]
pub struct FallbackBattery;

impl PowerSource for FallbackBattery {
    fn battery_percentage(&self) -> Result<u8> {
        Ok(100)
    }

    fn is_charging(&self) -> Result<bool> {
        Ok(true)
    }

    fn consumption_watts(&self) -> Result<f64> {
        Ok(0.0)
    }

    fn time_remaining(&self) -> Result<String> {
        Ok("AC Mains (Stationary)".into())
    }

    fn is_stationary(&self) -> bool {
        true
    }
}

/// Universal CPU Governor relying strictly on standard Rust runtime parallelism
#[derive(Debug)]
pub struct FallbackCpu {
    online: AtomicUsize,
}

impl FallbackCpu {
    pub fn new() -> Self {
        let count = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1);
        Self {
            online: AtomicUsize::new(count),
        }
    }
}

impl Default for FallbackCpu {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuGovernor for FallbackCpu {
    fn num_cpus(&self) -> usize {
        std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
    }

    fn online_cores(&self) -> Result<usize> {
        Ok(self.online.load(Ordering::Relaxed))
    }

    fn set_online_cores(&self, count: usize) -> Result<()> {
        let max = self.num_cpus();
        self.online.store(count.clamp(1, max), Ordering::Relaxed);
        Ok(())
    }

    fn freq_bounds(&self) -> Result<(u32, u32)> {
        Ok((800, 4000))
    }

    fn freq_limit(&self) -> Result<u32> {
        Ok(4000)
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
        Ok("default".into())
    }

    fn set_energy_performance_preference(&self, _pref: &str) -> Result<()> {
        Ok(())
    }
}

impl FallbackCpu {
    pub fn cstates(&self) -> Result<Vec<CStateInfo>> {
        Ok(Vec::new())
    }
}

impl CStateTelemetry for FallbackCpu {
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        self.cstates()
    }
}

/// Universal Fallback Charge Threshold (Safe unsupported stub)
#[derive(Debug, Clone, Default)]
pub struct FallbackThreshold;

impl ChargeThreshold for FallbackThreshold {
    fn supports_threshold(&self) -> bool {
        false
    }

    fn charge_threshold(&self) -> Result<u8> {
        Err(WattWardenError::Unsupported(
            "Hardware charge threshold not supported on this platform".into(),
        ))
    }

    fn set_charge_threshold(&self, _threshold: u8) -> Result<()> {
        Err(WattWardenError::Unsupported(
            "Hardware charge threshold not supported on this platform".into(),
        ))
    }
}

/// Universal Fallback Peripherals Controller
#[derive(Debug, Clone, Default)]
pub struct FallbackPeripherals;

impl PeripheralsController for FallbackPeripherals {
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

/// Universal Fallback System Tweaks Controller
#[derive(Debug, Clone, Default)]
pub struct FallbackTweaks;

impl SystemTweaksController for FallbackTweaks {
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
        Ok(false)
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

/// Universal Fallback Compositor Tracker
#[derive(Debug, Clone, Default)]
pub struct FallbackCompositor;

impl CompositorFocus for FallbackCompositor {
    fn active_window_class(&self) -> Option<String> {
        None
    }
}

/// The Universal Fallback Backend: guaranteed to compile and operate without panic on any platform
pub struct FallbackBackend {
    pub battery: FallbackBattery,
    pub cpu: FallbackCpu,
    pub rapl: Option<Box<dyn RaplController>>,
    pub gpu: Option<Box<dyn GpuController>>,
    pub aspm: Option<Box<dyn AspmController>>,
    pub backlight: Option<Box<dyn DisplayManager>>,
    pub threshold: FallbackThreshold,
    pub peripherals: FallbackPeripherals,
    pub tweaks: FallbackTweaks,
    pub hyprland: FallbackCompositor,
}

impl FallbackBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            battery: FallbackBattery,
            cpu: FallbackCpu::new(),
            rapl: None,
            gpu: None,
            aspm: None,
            backlight: None,
            threshold: FallbackThreshold,
            peripherals: FallbackPeripherals,
            tweaks: FallbackTweaks,
            hyprland: FallbackCompositor,
        })
    }

    /// No Go backend exists for an unrecognised OS; the dashboard treats this like
    /// Go's `else` branch of `buildMenuItems` (profiles only).
    pub fn os_name(&self) -> &'static str {
        "Unknown"
    }

    /// No portable load average exists for an unrecognised platform, so the shared
    /// ladder stays on its idle step instead of guessing at one.
    pub fn load_average(&self) -> f64 {
        0.0
    }

    pub fn capabilities(&self) -> HardwareCapabilities {
        HardwareCapabilities {
            has_battery: false,
            is_stationary_mains: true,
            has_cpu_frequency_control: false,
            has_cpu_core_control: self.cpu.num_cpus() > 1,
            has_rapl: false,
            has_gpu_control: false,
            has_backlight_control: false,
            has_charge_threshold: false,
            has_peripherals_control: false,
            has_system_tweaks: false,
            has_compositor_focus: false,
        }
    }

    pub fn apply_profile(&self, profile: &PowerProfile) -> Result<()> {
        match profile {
            PowerProfile::Performance => {
                let _ = self.cpu.set_online_cores(self.cpu.num_cpus());
            }
            PowerProfile::Extreme => {
                let _ = self.cpu.set_online_cores(2.min(self.cpu.num_cpus()));
            }
            PowerProfile::Normal | PowerProfile::AutoExtreme => {
                let _ = self.cpu.set_online_cores(self.cpu.num_cpus());
            }
        }
        Ok(())
    }
}

impl Default for FallbackBackend {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_backend_zero_panic_guarantee() {
        let backend = FallbackBackend::new().expect("fallback backend must never fail");
        assert_eq!(backend.os_name(), "Unknown");
        assert!(backend.battery.is_stationary());
        assert_eq!(backend.battery.battery_percentage().unwrap(), 100);
        assert!(backend.battery.is_charging().unwrap());
        assert_eq!(backend.battery.consumption_watts().unwrap(), 0.0);
        assert_eq!(
            backend.battery.time_remaining().unwrap(),
            "AC Mains (Stationary)"
        );

        let caps = backend.capabilities();
        assert!(!caps.has_battery);
        assert!(caps.is_stationary_mains);
        assert!(!caps.has_rapl);

        assert!(backend.apply_profile(&PowerProfile::Extreme).is_ok());
        assert!(backend.apply_profile(&PowerProfile::Performance).is_ok());
        assert!(backend.apply_profile(&PowerProfile::Normal).is_ok());
    }
}
