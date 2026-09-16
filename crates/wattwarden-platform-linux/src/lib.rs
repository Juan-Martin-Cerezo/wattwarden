pub mod backlight;
pub mod battery;
pub mod cpu;
pub mod hyprland;
pub mod netlink;
pub mod rapl;
pub mod sysfs;
pub mod threshold;

pub use backlight::LinuxBacklight;
pub use battery::LinuxBattery;
pub use cpu::LinuxCpuGovernor;
pub use hyprland::HyprlandIpc;
pub use netlink::NetlinkUeventListener;
pub use rapl::LinuxRapl;
pub use threshold::LinuxChargeThreshold;

use wattwarden_core::*;

pub struct LinuxBackend {
    pub battery: LinuxBattery,
    pub cpu: LinuxCpuGovernor,
    pub rapl: Option<LinuxRapl>,
    pub backlight: Option<LinuxBacklight>,
    pub threshold: LinuxChargeThreshold,
    pub hyprland: HyprlandIpc,
}

impl LinuxBackend {
    pub fn new() -> Result<Self> {
        let battery = LinuxBattery::new()?;
        let cpu = LinuxCpuGovernor::new();
        let rapl = LinuxRapl::new().ok();
        let backlight = LinuxBacklight::new().ok();
        let threshold = LinuxChargeThreshold::new();
        let hyprland = HyprlandIpc::new();

        Ok(Self {
            battery,
            cpu,
            rapl,
            backlight,
            threshold,
            hyprland,
        })
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
                if let Some(bl) = &self.backlight {
                    let _ = bl.set_brightness_percent(15);
                }
                let _ = sysfs::write_sysfs_string("/proc/sys/vm/drop_caches", "3");
                let _ = sysfs::write_sysfs_string("/proc/sys/vm/dirty_writeback_centisecs", "6000");
            }
            PowerProfile::Normal => {
                let _ = self.cpu.set_online_cores(self.cpu.num_cpus());
                let (_, max_freq) = self.cpu.freq_bounds().unwrap_or((400, 3500));
                let _ = self.cpu.set_freq_limit(max_freq);
                let _ = self.cpu.set_turbo_enabled(true);
                let _ = self.cpu.set_energy_performance_preference("balance_performance");
                if let Some(rapl) = &self.rapl {
                    let _ = rapl.set_pl1_watts(45);
                    let _ = rapl.set_pl2_watts(65);
                }
                let _ = sysfs::write_sysfs_string("/proc/sys/vm/dirty_writeback_centisecs", "500");
            }
            PowerProfile::AutoExtreme => {
                // Initial baseline: balanced
                let _ = self.cpu.set_energy_performance_preference("balance_power");
            }
        }
        Ok(())
    }
}
