pub mod aspm;
pub mod backlight;
pub mod battery;
pub mod cmd;
pub mod cpu;
pub mod gpu;
pub mod hyprland;
pub mod netlink;
pub mod peripherals;
pub mod rapl;
pub mod sysfs;
pub mod threshold;
pub mod tweaks;

pub use aspm::LinuxAspm;
pub use backlight::LinuxBacklight;
pub use battery::LinuxBattery;
pub use cpu::LinuxCpuGovernor;
pub use gpu::LinuxGpu;
pub use hyprland::HyprlandIpc;
pub use netlink::NetlinkUeventListener;
pub use peripherals::LinuxPeripherals;
pub use rapl::LinuxRapl;
pub use sysfs::SysfsRoot;
pub use threshold::LinuxChargeThreshold;
pub use tweaks::LinuxSystemTweaks;

use wattwarden_core::*;

pub struct LinuxBackend {
    pub battery: LinuxBattery,
    pub cpu: LinuxCpuGovernor,
    pub rapl: Option<LinuxRapl>,
    pub gpu: Option<LinuxGpu>,
    pub aspm: Option<LinuxAspm>,
    pub backlight: Option<LinuxBacklight>,
    pub threshold: LinuxChargeThreshold,
    pub peripherals: LinuxPeripherals,
    pub tweaks: LinuxSystemTweaks,
    pub hyprland: HyprlandIpc,
}

impl LinuxBackend {
    pub fn new() -> Result<Self> {
        Self::with_root(SysfsRoot::from_env())
    }

    /// Builds the backend against a relocated `/sys` + `/proc` root.
    /// This is what makes the whole Linux backend testable without root or real hardware
    /// (`WATTWARDEN_SYSFS_ROOT=/tmp/fake-laptop`).
    pub fn with_root(root: SysfsRoot) -> Result<Self> {
        let battery = LinuxBattery::with_root(root.clone());
        let cpu = LinuxCpuGovernor::with_root(root.clone());
        let rapl = LinuxRapl::with_root(root.clone()).ok();
        let gpu = LinuxGpu::with_root(root.clone()).ok();
        let aspm = LinuxAspm::with_root(root.clone()).ok();
        let backlight = LinuxBacklight::with_root(root.clone()).ok();
        let threshold = LinuxChargeThreshold::with_root(root.clone());
        let peripherals = LinuxPeripherals::with_root(root.clone());
        let tweaks = LinuxSystemTweaks::with_root(root.clone());
        let hyprland = HyprlandIpc::with_root(root);

        Ok(Self {
            battery,
            cpu,
            rapl,
            gpu,
            aspm,
            backlight,
            threshold,
            peripherals,
            tweaks,
            hyprland,
        })
    }

    pub fn root(&self) -> &SysfsRoot {
        self.hyprland.root()
    }

    pub fn capabilities(&self) -> HardwareCapabilities {
        HardwareCapabilities {
            has_battery: self.battery.has_battery(),
            is_stationary_mains: self.battery.is_stationary(),
            has_cpu_frequency_control: self.cpu.freq_bounds().is_ok(),
            has_cpu_core_control: self.cpu.num_cpus() > 1,
            has_rapl: self.rapl.is_some(),
            has_gpu_control: self.gpu.is_some(),
            has_backlight_control: self.backlight.is_some(),
            has_charge_threshold: self.threshold.supports_threshold(),
            has_peripherals_control: true,
            has_system_tweaks: true,
            has_compositor_focus: self.hyprland.active_window_class().is_some(),
        }
    }

    pub fn apply_profile(&self, profile: &PowerProfile) -> Result<()> {
        match profile {
            PowerProfile::Performance => {
                let _ = self.cpu.set_online_cores(self.cpu.num_cpus());
                let (_, max_freq) = self.cpu.freq_bounds().unwrap_or((400, 4500));
                let _ = self.cpu.set_freq_limit(max_freq);
                let _ = self.cpu.set_turbo_enabled(true);
                let _ = self.cpu.set_energy_performance_preference("performance");

                if let Some(rapl) = &self.rapl {
                    let (_, max_w) = rapl.rapl_bounds().unwrap_or((5, 115));
                    let _ = rapl.set_pl1_watts(max_w);
                    let _ = rapl.set_pl2_watts(max_w);
                }
                if let Some(gpu) = &self.gpu {
                    let (_, max_g) = gpu.gpu_bounds().unwrap_or((300, 1100));
                    let _ = gpu.set_gpu_freq(max_g);
                }
                if let Some(aspm) = &self.aspm {
                    let _ = aspm.set_aspm_policy("performance");
                }
                if let Some(bl) = &self.backlight {
                    let _ = bl.set_brightness_percent(100);
                }

                let _ = self.tweaks.set_wifi_power_save(false);
                let _ = self.tweaks.set_audio_power_save(false);
                let _ = self.tweaks.set_autosuspend(false);
                let _ = self.tweaks.set_nmi_watchdog(true);
                let _ = self.tweaks.set_vm_writeback_seconds(5);
            }
            PowerProfile::Extreme => {
                let _ = self.cpu.set_online_cores(2.min(self.cpu.num_cpus()));
                let (min_freq, _) = self.cpu.freq_bounds().unwrap_or((400, 1600));
                let _ = self.cpu.set_freq_limit(min_freq);
                let _ = self.cpu.set_turbo_enabled(false);
                let _ = self.cpu.set_energy_performance_preference("power");

                if let Some(rapl) = &self.rapl {
                    let (min_w, _) = rapl.rapl_bounds().unwrap_or((5, 15));
                    let _ = rapl.set_pl1_watts(min_w);
                    let _ = rapl.set_pl2_watts(min_w);
                }
                if let Some(gpu) = &self.gpu {
                    let (min_g, _) = gpu.gpu_bounds().unwrap_or((300, 1100));
                    let _ = gpu.set_gpu_freq(min_g);
                }
                if let Some(aspm) = &self.aspm {
                    let _ = aspm.set_aspm_policy("powersave");
                }
                if let Some(bl) = &self.backlight {
                    let _ = bl.set_brightness_percent(10);
                }

                let _ = self.peripherals.set_kbd_backlight(false);
                let _ = self.tweaks.set_wifi_power_save(true);
                let _ = self.tweaks.set_audio_power_save(true);
                let _ = self.tweaks.set_autosuspend(true);
                let _ = self.tweaks.set_nmi_watchdog(false);
                let _ = self.tweaks.set_vm_writeback_seconds(60);
            }
            PowerProfile::Normal => {
                let _ = self.cpu.set_online_cores(self.cpu.num_cpus());
                let (_, max_freq) = self.cpu.freq_bounds().unwrap_or((400, 3500));
                let _ = self.cpu.set_freq_limit(max_freq);
                let _ = self.cpu.set_turbo_enabled(true);
                let _ = self.cpu.set_energy_performance_preference("default");

                if let Some(rapl) = &self.rapl {
                    let (_, max_w) = rapl.rapl_bounds().unwrap_or((5, 115));
                    let _ = rapl.set_pl1_watts(max_w);
                    let _ = rapl.set_pl2_watts(max_w);
                }
                if let Some(gpu) = &self.gpu {
                    let (_, max_g) = gpu.gpu_bounds().unwrap_or((300, 1100));
                    let _ = gpu.set_gpu_freq(max_g);
                }
                if let Some(aspm) = &self.aspm {
                    let _ = aspm.set_aspm_policy("default");
                }
                if let Some(bl) = &self.backlight {
                    let _ = bl.set_brightness_percent(100);
                }

                let _ = self.tweaks.set_wifi_power_save(false);
                let _ = self.tweaks.set_audio_power_save(false);
                let _ = self.tweaks.set_autosuspend(false);
                let _ = self.tweaks.set_nmi_watchdog(true);
                let _ = self.tweaks.set_vm_writeback_seconds(5);
            }
            PowerProfile::AutoExtreme => {
                let _ = self.cpu.set_energy_performance_preference("balance_power");
            }
        }
        Ok(())
    }
}
