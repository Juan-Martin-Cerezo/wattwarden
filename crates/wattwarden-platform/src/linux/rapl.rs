//! Intel/AMD RAPL package power limits (PL1 long-term / PL2 short-term).
//!
//! Hardware-agnostic rewrite of the Go transcription. The Go reference clamps
//! every constraint to the *package* range and falls back to a literal `5..115 W`,
//! which is what made WattWarden write **115 W on a machine whose real PL1 ceiling
//! is 45 W** (and 34 W where the ceiling is 15 W).
//!
//! The rules here:
//!
//! * Each constraint is discovered by name (`long_term` = PL1, `short_term` = PL2)
//!   exactly like Go's `getRAPLPath`.
//! * The real ceiling of a constraint is `constraint_N_max_power_uw` (and its floor
//!   `constraint_N_min_power_uw`, defaulting to 0 when the firmware does not expose
//!   it). PL1 and PL2 are handled **separately** — they are different constraints
//!   with different ranges, never the same number.
//! * A write is clamped inside that constraint's *own* discovered range and can
//!   therefore never exceed it.
//! * When `constraint_N_max_power_uw` is missing or 0 (the firmware does not expose
//!   a range — the real machines only expose `constraint_0`, `constraint_1` is 0)
//!   the constraint is **not written at all** and the reason is logged. There is no
//!   absolute fallback: `5/115 W` is never invented for a write.
//!
//! Every path goes through [`SysfsRoot`] so `WATTWARDEN_SYSFS_ROOT` relocates the
//! whole probe.

use crate::linux::sysfs::SysfsRoot;
use tracing::debug;
use wattwarden_core::{RaplController, Result, WattWardenError};

/// RAPL package 0 (relative to the sysfs root).
const RAPL_BASE: &str = "sys/class/powercap/intel-rapl:0";

/// Legacy **display-only** fallback for [`LinuxRapl::rapl_bounds`]. It is NEVER used
/// to compute a write: a constraint without a discovered `max_power_uw` is skipped.
pub const FALLBACK_MIN_WATTS: u32 = 5;
pub const FALLBACK_MAX_WATTS: u32 = 115;

/// Go `getRAPLPath` iterates `i` from 0 to 4 inclusive.
const CONSTRAINT_SLOTS: std::ops::RangeInclusive<usize> = 0..=4;

/// Constraint name for the long-term (PL1) limit.
const LONG_TERM: &str = "long_term";
/// Constraint name for the short-term (PL2) limit.
const SHORT_TERM: &str = "short_term";

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

    /// Go `getRAPLPath(name)`: index of the first `constraint_%d_name` equal to `name`.
    fn constraint_index(&self, name: &str) -> Option<usize> {
        let mut slots = CONSTRAINT_SLOTS;
        slots.find(|i| self.root.read(&format!("{RAPL_BASE}/constraint_{i}_name")) == name)
    }

    /// `constraint_N_max_power_uw` path for slot `i`.
    fn max_uw_path(i: usize) -> String {
        format!("{RAPL_BASE}/constraint_{i}_max_power_uw")
    }

    /// `constraint_N_power_limit_uw` path for slot `i`.
    fn limit_uw_path(i: usize) -> String {
        format!("{RAPL_BASE}/constraint_{i}_power_limit_uw")
    }

    /// Discovered `(min, max)` in Watts for constraint slot `i`.
    ///
    /// `None` when the firmware does not expose `constraint_N_max_power_uw` or
    /// exposes it as 0, or when the range is degenerate (`max <= min`). Callers
    /// must treat `None` as "do not write this constraint".
    pub fn constraint_bounds_watts(&self, i: usize) -> Option<(u32, u32)> {
        let max_uw = self.root.read_u64(&Self::max_uw_path(i))?;
        if max_uw == 0 {
            return None;
        }
        let min_uw = self
            .root
            .read_u64(&format!("{RAPL_BASE}/constraint_{i}_min_power_uw"))
            .unwrap_or(0);
        let min = (min_uw / 1_000_000) as u32;
        let max = (max_uw / 1_000_000) as u32;
        if max <= min {
            return None;
        }
        Some((min, max))
    }

    /// Writes `watts` to the constraint called `name`, clamped inside its **own**
    /// discovered range. Returns the value actually written, or `None` when the
    /// constraint (or its range) does not exist — then nothing is written.
    pub fn write_constraint(&self, name: &str, watts: u32) -> Option<u32> {
        let Some(i) = self.constraint_index(name) else {
            debug!("RAPL: constraint '{name}' not exposed by the hardware; not writing");
            return None;
        };
        let Some((min, max)) = self.constraint_bounds_watts(i) else {
            debug!(
                "RAPL: constraint '{name}' does not expose a usable range \
                 (constraint_{i}_max_power_uw missing or 0); not writing"
            );
            return None;
        };
        let clamped = watts.clamp(min, max);
        self.root.write_best_effort(
            &Self::limit_uw_path(i),
            &(clamped as u64 * 1_000_000).to_string(),
        );
        Some(clamped)
    }

    /// Range used for display/stepping. Discovery first: the union of the per-constraint
    /// ranges (`constraint_N_{min,max}_power_uw`), then the package `*_power_range_uw`,
    /// and only if nothing is discoverable the legacy `5..115 W` fallback. **Never**
    /// used to pick a written value — writes go through [`LinuxRapl::write_constraint`].
    fn bounds(&self) -> (u32, u32) {
        let mut discovered: Option<(u32, u32)> = None;
        for name in [LONG_TERM, SHORT_TERM] {
            if let Some((min, max)) = self
                .constraint_index(name)
                .and_then(|i| self.constraint_bounds_watts(i))
            {
                discovered = Some(match discovered {
                    Some((lo, hi)) => (lo.min(min), hi.max(max)),
                    None => (min, max),
                });
            }
        }
        if let Some(bounds) = discovered {
            return bounds;
        }

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

    fn get_limit(&self, constraint: &str) -> u32 {
        match self.constraint_index(constraint) {
            Some(i) => {
                (self.root.read_u64(&Self::limit_uw_path(i)).unwrap_or(0) / 1_000_000) as u32
            }
            // Go `getRAPLPath` returning "" makes the getter return 0.
            None => 0,
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

    /// Discovered `(min, max)` Watts of the PL1 (`long_term`) constraint, if any.
    fn pl1_bounds_watts(&self) -> Option<(u32, u32)> {
        self.constraint_index(LONG_TERM)
            .and_then(|i| self.constraint_bounds_watts(i))
    }

    /// Discovered `(min, max)` Watts of the PL2 (`short_term`) constraint, if any.
    fn pl2_bounds_watts(&self) -> Option<(u32, u32)> {
        self.constraint_index(SHORT_TERM)
            .and_then(|i| self.constraint_bounds_watts(i))
    }

    fn pl1_watts(&self) -> Result<u32> {
        Ok(self.get_limit(LONG_TERM))
    }

    fn set_pl1_watts(&self, watts: u32) -> Result<()> {
        self.write_constraint(LONG_TERM, watts);
        Ok(())
    }

    fn pl2_watts(&self) -> Result<u32> {
        Ok(self.get_limit(SHORT_TERM))
    }

    fn set_pl2_watts(&self, watts: u32) -> Result<()> {
        self.write_constraint(SHORT_TERM, watts);
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

    fn write(root: &SysfsRoot, rel: &str, value: &str) {
        let p = root.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, value).unwrap();
    }

    /// Defines the two real constraints with `max_power_uw` ceilings.
    fn real_constraints(root: &SysfsRoot, pl1_max_uw: u64, pl2_max_uw: u64) {
        write(
            root,
            &format!("{RAPL_BASE}/constraint_0_name"),
            "long_term\n",
        );
        write(
            root,
            &format!("{RAPL_BASE}/constraint_0_max_power_uw"),
            &format!("{pl1_max_uw}\n"),
        );
        write(
            root,
            &format!("{RAPL_BASE}/constraint_0_power_limit_uw"),
            &format!("{pl1_max_uw}\n"),
        );
        write(
            root,
            &format!("{RAPL_BASE}/constraint_1_name"),
            "short_term\n",
        );
        write(
            root,
            &format!("{RAPL_BASE}/constraint_1_max_power_uw"),
            &format!("{pl2_max_uw}\n"),
        );
        write(
            root,
            &format!("{RAPL_BASE}/constraint_1_power_limit_uw"),
            &format!("{pl2_max_uw}\n"),
        );
    }

    #[test]
    fn bounds_fall_back_to_five_and_hundred_fifteen_for_display_only() {
        let rapl = LinuxRapl::with_root(root("empty")).unwrap();
        assert_eq!(rapl.rapl_bounds().unwrap(), (5, 115));
        // No constraint files at all -> Go returns 0 instead of erroring.
        assert_eq!(rapl.pl1_watts().unwrap(), 0);
        assert_eq!(rapl.pl2_watts().unwrap(), 0);
        // And crucially: without a discovered range, nothing is written.
        assert_eq!(rapl.write_constraint(LONG_TERM, 115), None);
        assert_eq!(rapl.pl1_bounds_watts(), None);
    }

    #[test]
    fn constraints_are_discovered_by_name_not_by_index() {
        let root = root("byname");
        // Deliberately swapped so index-based lookups would be wrong.
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_name"),
            "short_term\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_1_name"),
            "long_term\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_max_power_uw"),
            "25000000\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_1_max_power_uw"),
            "45000000\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_power_limit_uw"),
            "20000000\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_1_power_limit_uw"),
            "45000000\n",
        );

        let rapl = LinuxRapl::with_root(root.clone()).unwrap();
        assert_eq!(rapl.pl1_watts().unwrap(), 45);
        assert_eq!(rapl.pl2_watts().unwrap(), 20);
        assert_eq!(rapl.pl1_bounds_watts(), Some((0, 45)));
        assert_eq!(rapl.pl2_bounds_watts(), Some((0, 25)));

        // PL1 ceiling is 45 W, so 60 clamps down to it (never above max_power_uw).
        assert_eq!(rapl.write_constraint(LONG_TERM, 60), Some(45));
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_1_power_limit_uw")),
            "45000000"
        );
        assert_eq!(rapl.write_constraint(SHORT_TERM, 70), Some(25));
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_0_power_limit_uw")),
            "25000000"
        );
    }

    #[test]
    fn pl1_and_pl2_use_their_own_ranges_not_the_same_number() {
        let root = root("perconstraint");
        real_constraints(&root, 45_000_000, 15_000_000);
        let rapl = LinuxRapl::with_root(root.clone()).unwrap();

        assert_eq!(rapl.pl1_bounds_watts(), Some((0, 45)));
        assert_eq!(rapl.pl2_bounds_watts(), Some((0, 15)));

        // The same requested watts land on each constraint's own ceiling.
        assert_eq!(rapl.write_constraint(LONG_TERM, 999), Some(45));
        assert_eq!(rapl.write_constraint(SHORT_TERM, 999), Some(15));
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_0_power_limit_uw")),
            "45000000"
        );
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_1_power_limit_uw")),
            "15000000"
        );

        // And the floor is per-constraint too.
        assert_eq!(rapl.write_constraint(LONG_TERM, 0), Some(0));
        assert_eq!(rapl.write_constraint(SHORT_TERM, 0), Some(0));
    }

    #[test]
    fn a_zero_or_missing_max_power_uw_means_that_constraint_is_not_written() {
        let root = root("norange");
        // PL1 exposes a range, PL2 explicitly reports 0 (the real Dell case).
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_name"),
            "long_term\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_max_power_uw"),
            "45000000\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_power_limit_uw"),
            "45000000\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_1_name"),
            "short_term\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_1_max_power_uw"),
            "0\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_1_power_limit_uw"),
            "1234000000\n",
        );

        let rapl = LinuxRapl::with_root(root.clone()).unwrap();
        assert_eq!(rapl.pl2_bounds_watts(), None);
        // No discovered range -> not written, the firmware value is left alone.
        assert_eq!(rapl.write_constraint(SHORT_TERM, 30), None);
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_1_power_limit_uw")),
            "1234000000"
        );
        // PL1 still works.
        assert_eq!(rapl.write_constraint(LONG_TERM, 30), Some(30));
    }

    #[test]
    fn min_power_uw_is_honoured_when_exposed() {
        let root = root("minbound");
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_name"),
            "long_term\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_min_power_uw"),
            "10000000\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_max_power_uw"),
            "45000000\n",
        );
        write(
            &root,
            &format!("{RAPL_BASE}/constraint_0_power_limit_uw"),
            "45000000\n",
        );

        let rapl = LinuxRapl::with_root(root.clone()).unwrap();
        assert_eq!(rapl.pl1_bounds_watts(), Some((10, 45)));
        assert_eq!(rapl.write_constraint(LONG_TERM, 2), Some(10));
        assert_eq!(
            root.read(&format!("{RAPL_BASE}/constraint_0_power_limit_uw")),
            "10000000"
        );
    }

    #[test]
    fn display_bounds_prefer_discovered_constraint_ranges() {
        let root = root("displaybounds");
        // No package `*_power_range_uw` at all: the union of the constraints is used.
        real_constraints(&root, 45_000_000, 15_000_000);
        let rapl = LinuxRapl::with_root(root).unwrap();
        assert_eq!(rapl.rapl_bounds().unwrap(), (0, 45));
    }

    #[test]
    fn missing_rapl_interface_is_reported() {
        let dir = std::env::temp_dir().join(format!("ww_rapl_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        assert!(LinuxRapl::with_root(SysfsRoot::new(dir)).is_err());
    }
}
