//! Intel/AMD RAPL package power limits (PL1 long-term / PL2 short-term).
//!
//! Faithful transcription of `hal/backend_linux.go`:
//!
//! * `GetRAPLBounds` -> `/sys/class/powercap/intel-rapl:0/{min,max}_power_range_uw`
//!   divided by 1e6 (integer), fallback `5`/`115` W when the file is missing or
//!   unparsable. Note Go keeps the fallback *per* file, it does not require both.
//! * `getRAPLPath(name)` -> scans `constraint_%d_name` for `i` in `0..=4` and
//!   returns `constraint_%d_power_limit_uw` for the first literal match
//!   (`long_term` / `short_term`); `""` when nothing matches.
//! * `GetRAPLPL1/PL2` -> matched value / 1e6, **`0` when there is no constraint**
//!   (Go returns 0 rather than an error).
//! * `SetRAPLPL1/PL2` -> clamp to bounds, write `W * 1e6` microwatts (best effort).
//!
//! Every path goes through [`SysfsRoot`] so `WATTWARDEN_SYSFS_ROOT` relocates the
//! whole probe.

use crate::linux::sysfs::SysfsRoot;
use wattwarden_core::{RaplController, Result, WattWardenError};

/// RAPL package 0 (relative to the sysfs root).
const RAPL_BASE: &str = "sys/class/powercap/intel-rapl:0";

/// Go fallbacks for `GetRAPLBounds`.
pub const FALLBACK_MIN_WATTS: u32 = 5;
pub const FALLBACK_MAX_WATTS: u32 = 115;

/// Go `getRAPLPath` iterates `i` from 0 to 4 inclusive.
const CONSTRAINT_SLOTS: std::ops::RangeInclusive<usize> = 0..=4;

pub struct LinuxRapl {
    root: SysfsRoot,
}

impl LinuxRapl {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Result<Self> {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Result<Self> {
        if !root.exists(RAPL_BASE) {
            return Err(WattWardenError::InterfaceNotFound(format!(
                "Intel/AMD RAPL interface not present at /{RAPL_BASE}"
            )));
        }
        Ok(Self { root })
    }

    /// Go `getRAPLPath(name)`: first `constraint_%d_name` equal to `name`.
    fn constraint_path(&self, name: &str) -> Option<String> {
        for i in CONSTRAINT_SLOTS {
            let name_path = format!("{RAPL_BASE}/constraint_{i}_name");
            if self.root.read(&name_path) == name {
                return Some(format!("{RAPL_BASE}/constraint_{i}_power_limit_uw"));
            }
        }
        None
    }

    /// Go `GetRAPLBounds`: per-file fallback, integer division by 1e6.
    fn bounds(&self) -> (u32, u32) {
        let min = self
            .root
            .read_u64(&format!("{RAPL_BASE}/min_power_range_uw"))
            .map(|v| (v / 1_000_000) as u32)
            .unwrap_or(FALLBACK_MIN_WATTS);
        let max = self
            .root
            .read_u64(&format!("{RAPL_BASE}/max_power_range_uw"))
            .map(|v| (v / 1_000_000) as u32)
            .unwrap_or(FALLBACK_MAX_WATTS);
        (min, max)
    }

    /// Go `w < minW`, `w > maxW` clamping.
    fn clamp(&self, watts: u32) -> u32 {
        let (min, max) = self.bounds();
        watts.clamp(min, max)
    }

    fn get_limit(&self, constraint: &str) -> u32 {
        match self.constraint_path(constraint) {
            Some(path) => (self.root.read_u64(&path).unwrap_or(0) / 1_000_000) as u32,
            // Go `getRAPLPath` returning "" makes the getter return 0.
            None => 0,
        }
    }

    fn set_limit(&self, constraint: &str, watts: u32) {
        let clamped = self.clamp(watts);
        if let Some(path) = self.constraint_path(constraint) {
            self.root
                .write_best_effort(&path, &(clamped as u64 * 1_000_000).to_string());
        }
    }
}

impl Default for LinuxRapl {
    fn default() -> Self {
        Self::new().expect("RAPL interface unavailable")
    }
}

impl RaplController for LinuxRapl {
    fn rapl_bounds(&self) -> Result<(u32, u32)> {
        Ok(self.bounds())
    }

    fn pl1_watts(&self) -> Result<u32> {
        Ok(self.get_limit("long_term"))
    }

    fn set_pl1_watts(&self, watts: u32) -> Result<()> {
        self.set_limit("long_term", watts);
        Ok(())
    }

    fn pl2_watts(&self) -> Result<u32> {
        Ok(self.get_limit("short_term"))
    }

    fn set_pl2_watts(&self, watts: u32) -> Result<()> {
        self.set_limit("short_term", watts);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(tag: &str) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_rapl_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(RAPL_BASE)).unwrap();
        SysfsRoot::new(dir)
    }

    #[test]
    fn bounds_fall_back_to_five_and_hundred_fifteen() {
        let rapl = LinuxRapl::with_root(root("empty")).unwrap();
        assert_eq!(rapl.rapl_bounds().unwrap(), (5, 115));
        // No constraint files at all -> Go returns 0 instead of erroring.
        assert_eq!(rapl.pl1_watts().unwrap(), 0);
        assert_eq!(rapl.pl2_watts().unwrap(), 0);
    }

    #[test]
    fn constraints_are_discovered_by_name_not_by_index() {
        let root = root("byname");
        // Deliberately swapped so index-based lookups would be wrong.
        fs::write(
            root.path(&format!("{RAPL_BASE}/constraint_0_name")),
            "short_term\n",
        )
        .unwrap();
        fs::write(
            root.path(&format!("{RAPL_BASE}/constraint_1_name")),
            "long_term\n",
        )
        .unwrap();
        fs::write(
            root.path(&format!("{RAPL_BASE}/constraint_0_power_limit_uw")),
            "20000000\n",
        )
        .unwrap();
        fs::write(
            root.path(&format!("{RAPL_BASE}/constraint_1_power_limit_uw")),
            "45000000\n",
        )
        .unwrap();

        let rapl = LinuxRapl::with_root(root.clone()).unwrap();
        assert_eq!(rapl.pl1_watts().unwrap(), 45);
        assert_eq!(rapl.pl2_watts().unwrap(), 20);

        rapl.set_pl1_watts(60).unwrap();
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_1_power_limit_uw")),
            "60000000"
        );
        rapl.set_pl2_watts(70).unwrap();
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_0_power_limit_uw")),
            "70000000"
        );
    }

    #[test]
    fn missing_rapl_interface_is_reported() {
        let dir = std::env::temp_dir().join(format!("ww_rapl_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        assert!(LinuxRapl::with_root(SysfsRoot::new(dir)).is_err());
    }
}
