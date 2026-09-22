//! CPU core / frequency / governor control.
//!
//! Faithful transcription of `hal/backend_linux.go` (Go reference implementation):
//!
//! * `GetNumCPUs`  -> glob `/sys/devices/system/cpu/cpu[0-9]*`, fallback `runtime.NumCPU()`
//! * `GetCores`    -> `1 + count(cpuN/online == "1")` (cpu0 assumed online)
//! * `SetCores`    -> clamp 1..NumCPUs, write `online`, then **re-apply** `SetFreqLimit(GetFreqLimit())`
//! * `GetCPUFreqBounds` -> `cpu0/cpufreq/cpuinfo_{min,max}_freq` /1000, fallback 400/1600
//! * `GetFreqLimit` -> `cpu0/cpufreq/scaling_max_freq` /1000
//! * `SetFreqLimit` -> clamp, write `scaling_min_freq` = min and `scaling_max_freq` = mhz
//!   on **every** `cpu*/cpufreq`
//! * `GetTurbo`/`SetTurbo` -> `intel_pstate/no_turbo` ("0" = on), else `cpufreq/boost`
//!   ("1" = on), else default `true`
//! * `GetEPP`/`SetEPP` -> `cpu0/cpufreq/energy_performance_preference`; writes **all**
//!   `cpu*/cpufreq/energy_performance_preference` plus the governor
//!   (`performance` when pref == "performance", otherwise `powersave`)
//!   on **all** `cpu*/cpufreq/scaling_governor`

use crate::linux::sysfs::SysfsRoot;
use wattwarden_core::{CStateInfo, CStateTelemetry, CpuGovernor, Result, WattWardenError};

const CPU_BASE: &str = "sys/devices/system/cpu";

/// CPU frequency fallback bounds (MHz) — `PARITY.md` §2 / Go `GetCPUFreqBounds`.
pub const FALLBACK_FREQ_MIN_MHZ: u32 = 400;
pub const FALLBACK_FREQ_MAX_MHZ: u32 = 1600;

pub struct LinuxCpuGovernor {
    root: SysfsRoot,
    /// Numbers discovered under `/sys/devices/system/cpu` in ascending order.
    discovered: Vec<u32>,
}

impl LinuxCpuGovernor {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Self {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Self {
        let mut discovered: Vec<u32> = root
            .names(CPU_BASE)
            .into_iter()
            .filter_map(|name| name.strip_prefix("cpu").map(str::to_string))
            .filter_map(|digits| digits.parse::<u32>().ok())
            .collect();
        discovered.sort_unstable();
        discovered.dedup();
        Self { root, discovered }
    }

    /// Go `filepath.Glob("/sys/devices/system/cpu/cpu[0-9]*")`: when the glob finds
    /// nothing Go keeps going with `runtime.NumCPU()` and builds `cpuN` paths from
    /// that count, so mirror it here.
    fn cpu_ids(&self) -> Vec<u32> {
        if self.discovered.is_empty() {
            (0..self.num_cpus() as u32).collect()
        } else {
            self.discovered.clone()
        }
    }

    fn cpu_rel(&self, id: u32) -> String {
        format!("{CPU_BASE}/cpu{id}")
    }
}

impl Default for LinuxCpuGovernor {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuGovernor for LinuxCpuGovernor {
    fn num_cpus(&self) -> usize {
        if !self.discovered.is_empty() {
            self.discovered.len()
        } else {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(1)
        }
    }

    fn online_cores(&self) -> Result<usize> {
        let mut cores = 1; // cpu0 cannot be turned off
        for id in self.cpu_ids().into_iter().skip(1) {
            if self.root.read(&format!("{}/online", self.cpu_rel(id))) == "1" {
                cores += 1;
            }
        }
        Ok(cores)
    }

    fn set_online_cores(&self, count: usize) -> Result<()> {
        let n = count.clamp(1, self.num_cpus());

        for (idx, id) in self.cpu_ids().into_iter().enumerate().skip(1) {
            let val = if idx < n { "1" } else { "0" };
            self.root
                .write_best_effort(&format!("{}/online", self.cpu_rel(id)), val);
        }

        // Go re-applies the frequency limit so newly woken cores inherit it.
        let mhz = self.freq_limit()?;
        if mhz > 0 {
            self.set_freq_limit(mhz)?;
        }
        Ok(())
    }

    fn freq_bounds(&self) -> Result<(u32, u32)> {
        let min_khz = self
            .root
            .read_i64(&format!("{CPU_BASE}/cpu0/cpufreq/cpuinfo_min_freq"));
        let max_khz = self
            .root
            .read_i64(&format!("{CPU_BASE}/cpu0/cpufreq/cpuinfo_max_freq"));

        // Go: `strconv.Atoi` error -> keep the 400/1600 fallback, and integer-divide by 1000.
        let min_mhz = min_khz
            .map(|v| v / 1000)
            .unwrap_or(FALLBACK_FREQ_MIN_MHZ as i64);
        let max_mhz = max_khz
            .map(|v| v / 1000)
            .unwrap_or(FALLBACK_FREQ_MAX_MHZ as i64);

        Ok((min_mhz.max(0) as u32, max_mhz.max(0) as u32))
    }

    fn freq_limit(&self) -> Result<u32> {
        // Go: failed parse yields 0.
        let v = self
            .root
            .read_i64(&format!("{CPU_BASE}/cpu0/cpufreq/scaling_max_freq"))
            .unwrap_or(0);
        if v == 0 {
            return Ok(0);
        }
        Ok((v / 1000).max(0) as u32)
    }

    fn set_freq_limit(&self, mhz: u32) -> Result<()> {
        let (min_mhz, max_mhz) = self.freq_bounds()?;
        let mhz = mhz.clamp(min_mhz, max_mhz);

        let khz_max = (mhz as u64) * 1000;
        let khz_min = (min_mhz as u64) * 1000;

        for id in self.cpu_ids() {
            let cpufreq = format!("{}/cpufreq", self.cpu_rel(id));
            if !self.root.exists(&cpufreq) {
                continue; // Go globs existing `cpu*/cpufreq` dirs only
            }
            self.root
                .write_best_effort(&format!("{cpufreq}/scaling_min_freq"), &khz_min.to_string());
            self.root
                .write_best_effort(&format!("{cpufreq}/scaling_max_freq"), &khz_max.to_string());
        }
        Ok(())
    }

    fn turbo_enabled(&self) -> Result<bool> {
        let intel = format!("{CPU_BASE}/intel_pstate/no_turbo");
        if self.root.exists(&intel) {
            return Ok(self.root.read(&intel) == "0");
        }

        let boost = format!("{CPU_BASE}/cpufreq/boost");
        if self.root.exists(&boost) {
            return Ok(self.root.read(&boost) == "1");
        }

        // Go assumes turbo is on when neither interface exists.
        Ok(true)
    }

    fn set_turbo_enabled(&self, enabled: bool) -> Result<()> {
        let intel = format!("{CPU_BASE}/intel_pstate/no_turbo");
        if self.root.exists(&intel) {
            self.root
                .write_best_effort(&intel, if enabled { "0" } else { "1" });
            return Ok(());
        }

        let boost = format!("{CPU_BASE}/cpufreq/boost");
        if self.root.exists(&boost) {
            self.root
                .write_best_effort(&boost, if enabled { "1" } else { "0" });
        }
        Ok(())
    }

    fn energy_performance_preference(&self) -> Result<String> {
        let path = format!("{CPU_BASE}/cpu0/cpufreq/energy_performance_preference");
        if !self.root.exists(&path) {
            return Err(WattWardenError::Unsupported(
                "EPP interface not found".into(),
            ));
        }
        Ok(self.root.read(&path))
    }

    fn set_energy_performance_preference(&self, pref: &str) -> Result<()> {
        for id in self.cpu_ids() {
            let epp = format!("{}/cpufreq/energy_performance_preference", self.cpu_rel(id));
            if self.root.exists(&epp) {
                self.root.write_best_effort(&epp, pref);
            }
        }

        let gov = if pref == "performance" {
            "performance"
        } else {
            "powersave"
        };
        for id in self.cpu_ids() {
            let governor = format!("{}/cpufreq/scaling_governor", self.cpu_rel(id));
            if self.root.exists(&governor) {
                self.root.write_best_effort(&governor, gov);
            }
        }
        Ok(())
    }
}

impl CStateTelemetry for LinuxCpuGovernor {
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        let cpuidle_dir = format!("{CPU_BASE}/cpu0/cpuidle");
        let mut states = Vec::new();

        for name in self.root.names(&cpuidle_dir) {
            if !name.starts_with("state") {
                continue;
            }
            let rel = format!("{cpuidle_dir}/{name}");
            let state_name = {
                let n = self.root.read(&format!("{rel}/name"));
                if n.is_empty() {
                    "Unknown".to_string()
                } else {
                    n
                }
            };
            let time_us = self.root.read_u64(&format!("{rel}/time")).unwrap_or(0);
            let usage = self.root.read_u64(&format!("{rel}/usage")).unwrap_or(0);

            states.push(CStateInfo {
                name: state_name,
                time_microseconds: time_us,
                usage_count: usage,
            });
        }

        states.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(states)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn empty_root(tag: &str) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_cpu_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        SysfsRoot::new(dir)
    }

    #[test]
    fn missing_sysfs_uses_go_fallbacks() {
        let root = empty_root("empty");
        let cpu = LinuxCpuGovernor::with_root(root);

        assert_eq!(cpu.freq_bounds().unwrap(), (400, 1600));
        assert_eq!(cpu.freq_limit().unwrap(), 0);
        assert_eq!(cpu.online_cores().unwrap(), 1);
        assert!(cpu.turbo_enabled().unwrap());
        assert!(cpu.energy_performance_preference().is_err());

        // Writes with no hardware must not panic nor error out.
        cpu.set_freq_limit(3000).unwrap();
        cpu.set_online_cores(2).unwrap();
        cpu.set_turbo_enabled(false).unwrap();
        cpu.set_energy_performance_preference("performance")
            .unwrap();
        assert!(cpu.cstates().unwrap().is_empty());
    }

    #[test]
    fn amd_boost_interface_is_used_when_intel_pstate_is_absent() {
        let root = empty_root("amdboost");
        let cpu = LinuxCpuGovernor::with_root(root.clone());
        let _ = fs::create_dir_all(root.path(&format!("{CPU_BASE}/cpufreq")));
        fs::write(root.path(&format!("{CPU_BASE}/cpufreq/boost")), "1\n").unwrap();

        assert!(cpu.turbo_enabled().unwrap());
        cpu.set_turbo_enabled(false).unwrap();
        assert_eq!(root.read(&format!("{CPU_BASE}/cpufreq/boost")), "0");
        assert!(!cpu.turbo_enabled().unwrap());
    }

    #[test]
    fn intel_no_turbo_wins_over_generic_boost() {
        let root = empty_root("intelturbo");
        let _ = fs::create_dir_all(root.path(&format!("{CPU_BASE}/intel_pstate")));
        let _ = fs::create_dir_all(root.path(&format!("{CPU_BASE}/cpufreq")));
        fs::write(
            root.path(&format!("{CPU_BASE}/intel_pstate/no_turbo")),
            "0\n",
        )
        .unwrap();
        fs::write(root.path(&format!("{CPU_BASE}/cpufreq/boost")), "0\n").unwrap();

        let cpu = LinuxCpuGovernor::with_root(root.clone());
        assert!(cpu.turbo_enabled().unwrap()); // "0" in no_turbo means turbo ON

        cpu.set_turbo_enabled(false).unwrap();
        assert_eq!(root.read(&format!("{CPU_BASE}/intel_pstate/no_turbo")), "1");
        // The generic boost knob must stay untouched.
        assert_eq!(root.read(&format!("{CPU_BASE}/cpufreq/boost")), "0");
    }
}
