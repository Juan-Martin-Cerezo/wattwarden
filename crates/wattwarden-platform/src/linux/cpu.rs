use crate::sysfs::{
    glob_dirs, read_sysfs_string, read_sysfs_u64, write_sysfs_string, write_sysfs_u64,
};
use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{CStateInfo, CStateTelemetry, CpuGovernor, Result, WattWardenError};

pub struct LinuxCpuGovernor {
    cpu_dirs: Vec<PathBuf>,
}

impl LinuxCpuGovernor {
    pub fn new() -> Self {
        let mut cpu_dirs = glob_dirs("/sys/devices/system/cpu", "cpu");
        // Keep only numbered cpu directories (e.g. cpu0, cpu1)
        cpu_dirs.retain(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|name| {
                    name.strip_prefix("cpu")
                        .map(|num| num.parse::<u32>().is_ok())
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        });
        cpu_dirs.sort_by_key(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|name| name.strip_prefix("cpu"))
                .and_then(|num| num.parse::<u32>().ok())
                .unwrap_or(0)
        });

        Self { cpu_dirs }
    }
}

impl Default for LinuxCpuGovernor {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuGovernor for LinuxCpuGovernor {
    fn num_cpus(&self) -> usize {
        if !self.cpu_dirs.is_empty() {
            self.cpu_dirs.len()
        } else {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(1)
        }
    }

    fn online_cores(&self) -> Result<usize> {
        let mut count = 1; // CPU0 cannot be turned off
        for dir in self.cpu_dirs.iter().skip(1) {
            let online_path = dir.join("online");
            if let Ok(val) = read_sysfs_u64(&online_path) {
                if val == 1 {
                    count += 1;
                }
            } else {
                // If 'online' does not exist, core is permanently online
                count += 1;
            }
        }
        Ok(count)
    }

    fn set_online_cores(&self, count: usize) -> Result<()> {
        let max_cpus = self.num_cpus();
        let target = count.clamp(1, max_cpus);

        for (idx, dir) in self.cpu_dirs.iter().enumerate().skip(1) {
            let online_path = dir.join("online");
            if online_path.exists() {
                let status = if idx < target { 1 } else { 0 };
                write_sysfs_u64(&online_path, status)?;
            }
        }
        Ok(())
    }

    fn freq_bounds(&self) -> Result<(u32, u32)> {
        let base = Path::new("/sys/devices/system/cpu/cpu0/cpufreq");
        let min_khz = read_sysfs_u64(base.join("cpuinfo_min_freq")).unwrap_or(400_000);
        let max_khz = read_sysfs_u64(base.join("cpuinfo_max_freq")).unwrap_or(3_500_000);

        Ok(((min_khz / 1000) as u32, (max_khz / 1000) as u32))
    }

    fn freq_limit(&self) -> Result<u32> {
        let cur = read_sysfs_u64("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")?;
        Ok((cur / 1000) as u32)
    }

    fn set_freq_limit(&self, mhz: u32) -> Result<()> {
        let (min_bound, max_bound) = self.freq_bounds()?;
        let clamped_mhz = mhz.clamp(min_bound, max_bound);
        let khz = (clamped_mhz as u64) * 1000;

        for dir in &self.cpu_dirs {
            let max_freq_path = dir.join("cpufreq/scaling_max_freq");
            if max_freq_path.exists() {
                write_sysfs_u64(&max_freq_path, khz)?;
            }
        }
        Ok(())
    }

    fn turbo_enabled(&self) -> Result<bool> {
        // Intel pstate
        let intel_path = Path::new("/sys/devices/system/cpu/intel_pstate/no_turbo");
        if intel_path.exists() {
            let no_turbo = read_sysfs_u64(intel_path)?;
            return Ok(no_turbo == 0);
        }

        // Generic / AMD boost
        let boost_path = Path::new("/sys/devices/system/cpu/cpufreq/boost");
        if boost_path.exists() {
            let boost = read_sysfs_u64(boost_path)?;
            return Ok(boost == 1);
        }

        Err(WattWardenError::Unsupported(
            "CPU Turbo/Boost interface not found".into(),
        ))
    }

    fn set_turbo_enabled(&self, enabled: bool) -> Result<()> {
        let intel_path = Path::new("/sys/devices/system/cpu/intel_pstate/no_turbo");
        if intel_path.exists() {
            let no_turbo = if enabled { 0 } else { 1 };
            return write_sysfs_u64(intel_path, no_turbo);
        }

        let boost_path = Path::new("/sys/devices/system/cpu/cpufreq/boost");
        if boost_path.exists() {
            let boost = if enabled { 1 } else { 0 };
            return write_sysfs_u64(boost_path, boost);
        }

        Err(WattWardenError::Unsupported(
            "Cannot set turbo: interface not found".into(),
        ))
    }

    fn energy_performance_preference(&self) -> Result<String> {
        let path = Path::new("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference");
        if path.exists() {
            return read_sysfs_string(path);
        }
        Err(WattWardenError::Unsupported(
            "EPP interface not found".into(),
        ))
    }

    fn set_energy_performance_preference(&self, pref: &str) -> Result<()> {
        for dir in &self.cpu_dirs {
            let epp_path = dir.join("cpufreq/energy_performance_preference");
            if epp_path.exists() {
                write_sysfs_string(&epp_path, pref)?;
            }
        }
        Ok(())
    }
}

impl CStateTelemetry for LinuxCpuGovernor {
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        let cpuidle_dir = Path::new("/sys/devices/system/cpu/cpu0/cpuidle");
        let mut states = Vec::new();

        if let Ok(entries) = fs::read_dir(cpuidle_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("state"))
                    .unwrap_or(false)
                {
                    let name =
                        read_sysfs_string(p.join("name")).unwrap_or_else(|_| "Unknown".into());
                    let time_us = read_sysfs_u64(p.join("time")).unwrap_or(0);
                    let usage = read_sysfs_u64(p.join("usage")).unwrap_or(0);

                    states.push(CStateInfo {
                        name,
                        time_microseconds: time_us,
                        usage_count: usage,
                    });
                }
            }
        }

        states.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(states)
    }
}
