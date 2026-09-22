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
use tracing::debug;
use wattwarden_core::{CStateInfo, CStateTelemetry, CpuGovernor, Result, WattWardenError};

const CPU_BASE: &str = "sys/devices/system/cpu";

/// Energy Performance Preference ordering, most performance first. Used to pick the
/// closest *available* preference when the requested one is not advertised.
const EPP_ORDER: [&str; 5] = [
    "performance",
    "balance_performance",
    "default",
    "balance_power",
    "power",
];

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

impl LinuxCpuGovernor {
    /// `(min, max)` MHz actually discovered from `cpu0/cpufreq/cpuinfo_{min,max}_freq`.
    ///
    /// `None` when the hardware does not expose a usable range (missing, zero or
    /// inverted): callers must then **not write** a frequency limit. [`CpuGovernor::freq_bounds`]
    /// keeps the legacy `400/1600` fallback for display, but every write is gated on
    /// this discovered range, so no value can ever land outside it.
    pub fn discovered_freq_bounds(&self) -> Option<(u32, u32)> {
        let min_khz = self
            .root
            .read_i64(&format!("{CPU_BASE}/cpu0/cpufreq/cpuinfo_min_freq"))?;
        let max_khz = self
            .root
            .read_i64(&format!("{CPU_BASE}/cpu0/cpufreq/cpuinfo_max_freq"))?;
        if min_khz <= 0 || max_khz <= 0 || max_khz < min_khz {
            return None;
        }
        Some(((min_khz / 1000) as u32, (max_khz / 1000) as u32))
    }

    /// The EPP values the hardware advertises (`energy_performance_available_preferences`).
    fn available_epp(&self) -> Vec<String> {
        self.root
            .read(&format!(
                "{CPU_BASE}/cpu0/cpufreq/energy_performance_available_preferences"
            ))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// Maps a requested EPP onto the closest value the hardware actually advertises.
    ///
    /// When the hardware does not expose the list there is nothing to choose from and
    /// the request is passed through unchanged (best effort). A failure to map an
    /// unknown token yields `None` so the caller writes nothing.
    fn resolve_epp(&self, pref: &str) -> Option<String> {
        let available = self.available_epp();
        if available.is_empty() || available.iter().any(|a| a == pref) {
            return Some(pref.to_string());
        }
        let rank = |s: &str| EPP_ORDER.iter().position(|x| *x == s);
        let target = rank(pref)?;
        available
            .iter()
            .filter_map(|a| rank(a).map(|r| (r.abs_diff(target), a.clone())))
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, candidate)| candidate)
    }

    /// The governors the hardware advertises (`scaling_available_governors`).
    fn available_governors(&self) -> Vec<String> {
        self.root
            .read(&format!(
                "{CPU_BASE}/cpu0/cpufreq/scaling_available_governors"
            ))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// Maps a requested governor onto one the hardware advertises, preferring a
    /// power-saving replacement. `None` means "do not write".
    fn resolve_governor(&self, gov: &str) -> Option<String> {
        let available = self.available_governors();
        if available.is_empty() || available.iter().any(|a| a == gov) {
            return Some(gov.to_string());
        }
        for fallback in ["powersave", "performance", "schedutil", "ondemand"] {
            if available.iter().any(|a| a == fallback) {
                return Some(fallback.to_string());
            }
        }
        None
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
        // The legacy 400/1600 fallback is kept for display/compat only: every write
        // goes through `discovered_freq_bounds()` and is skipped when it is `None`.
        Ok(self
            .discovered_freq_bounds()
            .unwrap_or((FALLBACK_FREQ_MIN_MHZ, FALLBACK_FREQ_MAX_MHZ)))
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
        // No discovered range -> nothing to clamp against, so nothing is written.
        let Some((min_mhz, max_mhz)) = self.discovered_freq_bounds() else {
            debug!("CPU: cpufreq range not discovered; not writing scaling_min/max_freq");
            return Ok(());
        };
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
        // Only write a preference the hardware actually advertises (or the closest one).
        let Some(pref) = self.resolve_epp(pref) else {
            debug!("CPU: EPP '{pref}' not available on this hardware; not writing");
            return Ok(());
        };
        for id in self.cpu_ids() {
            let epp = format!("{}/cpufreq/energy_performance_preference", self.cpu_rel(id));
            if self.root.exists(&epp) {
                self.root.write_best_effort(&epp, &pref);
            }
        }

        let requested_gov = if pref == "performance" {
            "performance"
        } else {
            "powersave"
        };
        let Some(gov) = self.resolve_governor(requested_gov) else {
            debug!("CPU: governor '{requested_gov}' not available on this hardware; not writing");
            return Ok(());
        };
        for id in self.cpu_ids() {
            let governor = format!("{}/cpufreq/scaling_governor", self.cpu_rel(id));
            if self.root.exists(&governor) {
                self.root.write_best_effort(&governor, &gov);
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

    fn write(root: &SysfsRoot, rel: &str, value: &str) {
        let p = root.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, value).unwrap();
    }

    /// One CPU with `cpufreq` files but no `cpuinfo_*` range -> nothing is written.
    #[test]
    fn undiscovered_freq_range_is_never_written() {
        let root = empty_root("norange");
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/scaling_max_freq"),
            "800000\n",
        );
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/scaling_min_freq"),
            "800000\n",
        );

        let cpu = LinuxCpuGovernor::with_root(root.clone());
        assert_eq!(cpu.discovered_freq_bounds(), None);
        assert_eq!(cpu.freq_bounds().unwrap(), (400, 1600)); // display fallback only

        cpu.set_freq_limit(3000).unwrap();
        assert_eq!(
            root.read(&format!("{CPU_BASE}/cpu0/cpufreq/scaling_max_freq")),
            "800000"
        );
        assert_eq!(
            root.read(&format!("{CPU_BASE}/cpu0/cpufreq/scaling_min_freq")),
            "800000"
        );
    }

    /// Discovered bounds always clamp the write, whatever the caller asks for.
    #[test]
    fn discovered_range_clamps_every_write() {
        let root = empty_root("clamp");
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/cpuinfo_min_freq"),
            "400000\n",
        );
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/cpuinfo_max_freq"),
            "1600000\n",
        );
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/scaling_max_freq"),
            "1600000\n",
        );
        let cpu = LinuxCpuGovernor::with_root(root.clone());
        assert_eq!(cpu.discovered_freq_bounds(), Some((400, 1600)));

        cpu.set_freq_limit(99_999).unwrap();
        assert_eq!(
            root.read(&format!("{CPU_BASE}/cpu0/cpufreq/scaling_max_freq")),
            "1600000"
        );
        cpu.set_freq_limit(10).unwrap();
        assert_eq!(
            root.read(&format!("{CPU_BASE}/cpu0/cpufreq/scaling_max_freq")),
            "400000"
        );
    }

    /// The hardware's own EPP/governor lists decide what is written.
    #[test]
    fn epp_and_governor_come_from_the_available_lists() {
        let root = empty_root("epplist");
        for i in 0..2 {
            let base = format!("{CPU_BASE}/cpu{i}");
            write(
                &root,
                &format!("{base}/cpufreq/energy_performance_preference"),
                "balance_power\n",
            );
            write(
                &root,
                &format!("{base}/cpufreq/scaling_governor"),
                "powersave\n",
            );
        }
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/energy_performance_available_preferences"),
            "default performance balance_performance balance_power power\n",
        );
        // Only `powersave` is advertised by this hardware.
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/scaling_available_governors"),
            "powersave\n",
        );

        let cpu = LinuxCpuGovernor::with_root(root.clone());
        // `performance` is available, so both CPUs get it and the governor is derived
        // from it -- but the hardware only offers `powersave`, so that is written.
        cpu.set_energy_performance_preference("performance")
            .unwrap();
        assert_eq!(
            root.read(&format!(
                "{CPU_BASE}/cpu0/cpufreq/energy_performance_preference"
            )),
            "performance"
        );
        assert_eq!(
            root.read(&format!("{CPU_BASE}/cpu0/cpufreq/scaling_governor")),
            "powersave"
        );

        // A token the hardware never advertises is dropped (nothing written).
        cpu.set_energy_performance_preference("bogus").unwrap();
        assert_eq!(
            root.read(&format!(
                "{CPU_BASE}/cpu0/cpufreq/energy_performance_preference"
            )),
            "performance"
        );
    }

    /// When the hardware advertises a reduced EPP list, the closest value wins.
    #[test]
    fn epp_falls_back_to_the_closest_available_value() {
        let root = empty_root("eppnearest");
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/energy_performance_preference"),
            "default\n",
        );
        write(
            &root,
            &format!("{CPU_BASE}/cpu0/cpufreq/energy_performance_available_preferences"),
            "default performance\n",
        );
        let cpu = LinuxCpuGovernor::with_root(root.clone());

        // `power` is not offered; `default` is the closest advertised neighbour.
        cpu.set_energy_performance_preference("power").unwrap();
        assert_eq!(
            root.read(&format!(
                "{CPU_BASE}/cpu0/cpufreq/energy_performance_preference"
            )),
            "default"
        );
    }

    /// Turbo is only written when one of its interfaces actually exists.
    #[test]
    fn turbo_is_only_written_when_an_interface_exists() {
        let root = empty_root("noturbo");
        let cpu = LinuxCpuGovernor::with_root(root.clone());

        cpu.set_turbo_enabled(false).unwrap();
        assert!(!root.exists(&format!("{CPU_BASE}/intel_pstate/no_turbo")));
        assert!(!root.exists(&format!("{CPU_BASE}/cpufreq/boost")));

        // AMD/generic knob present: only this one is written.
        write(&root, &format!("{CPU_BASE}/cpufreq/boost"), "1\n");
        cpu.set_turbo_enabled(false).unwrap();
        assert_eq!(root.read(&format!("{CPU_BASE}/cpufreq/boost")), "0");
        assert!(!root.exists(&format!("{CPU_BASE}/intel_pstate/no_turbo")));
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
