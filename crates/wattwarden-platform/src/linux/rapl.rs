//! Powercap / RAPL package power limits (PL1 long-term / PL2 short-term).
//!
//! Hardware-agnostic rewrite of the Go transcription. Two rules drive it:
//!
//! * **Discovery instead of a hardcoded path.** The Go reference (and the first Rust
//!   port) assumed `/sys/class/powercap/intel-rapl:0`. That node exists on Intel
//!   notebooks only: AMD, ARM (Raspberry Pi), servers and containers either name the
//!   zone differently (`intel-rapl-mmio:0`, `amd-rapl:0`, …) or expose no powercap
//!   class at all. [`LinuxRapl::discover_domains`] globs
//!   `/sys/class/powercap/*` and accepts **every** zone that exposes a powercap
//!   constraint (`constraint_0_name`), whatever the vendor calls it, and supports
//!   several zones at once (multi-package). No zone at all is a normal condition:
//!   [`LinuxRapl::with_root`] returns `InterfaceNotFound` and the backend simply runs
//!   without RAPL.
//! * **A write is clamped inside the constraint's own discovered range.** Each
//!   constraint's ceiling is `constraint_N_max_power_uw` and its floor
//!   `constraint_N_min_power_uw` (defaulting to 0). PL1 and PL2 are different
//!   constraints with different ranges, never the same number. When
//!   `constraint_N_max_power_uw` is missing or 0 the constraint is **not written** and
//!   the reason is logged once per attempt at `debug!` — there is no absolute
//!   fallback: `5/115 W` is never invented for a write (that is what made WattWarden
//!   write 115 W on a machine whose real ceiling is 45 W).
//!
//! Every path goes through [`SysfsRoot`] so `WATTWARDEN_SYSFS_ROOT` relocates the
//! whole probe.

use crate::linux::sysfs::SysfsRoot;
use tracing::{debug, info};
use wattwarden_core::{RaplController, Result, WattWardenError};

/// Base of the powercap class (relative to the sysfs root).
const POWERCAP_BASE: &str = "sys/class/powercap";

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
    /// Discovered powercap zones (relative paths, sorted). Never empty: the
    /// constructor refuses to build a controller without a single zone.
    domains: Vec<String>,
}

impl LinuxRapl {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Result<Self> {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Result<Self> {
        let domains = Self::discover_domains(&root);
        if domains.is_empty() {
            debug!(
                "RAPL: no powercap zone exposing a constraint under /{POWERCAP_BASE}; \
                 RAPL controls disabled"
            );
            return Err(WattWardenError::InterfaceNotFound(format!(
                "No powercap/RAPL zone discovered under /{POWERCAP_BASE}"
            )));
        }
        info!(
            "RAPL: {} powercap zone(s) discovered: {}",
            domains.len(),
            domains.join(", ")
        );
        Ok(Self { root, domains })
    }

    /// Glob `/sys/class/powercap/*`: every entry that exposes a powercap constraint
    /// (`constraint_0_name`) is a control zone, whatever the vendor names it. Naming
    /// is deliberately not matched on: an AMD, ARM or future kernel scheme is found
    /// exactly like `intel-rapl:0`, and unrelated powercap entries are ignored.
    fn discover_domains(root: &SysfsRoot) -> Vec<String> {
        root.names(POWERCAP_BASE)
            .into_iter()
            .map(|name| format!("{POWERCAP_BASE}/{name}"))
            .filter(|rel| root.exists(&format!("{rel}/constraint_0_name")))
            .collect()
    }

    /// Relative paths of the zones this controller writes to (diagnostics/tests).
    pub fn domains(&self) -> &[String] {
        &self.domains
    }

    /// Go `getRAPLPath(name)` inside one zone: index of the first `constraint_%d_name`
    /// equal to `name`.
    fn constraint_index_in(&self, domain: &str, name: &str) -> Option<usize> {
        let mut slots = CONSTRAINT_SLOTS;
        slots.find(|i| self.root.read(&format!("{domain}/constraint_{i}_name")) == name)
    }

    /// `constraint_N_max_power_uw` path for slot `i` of `domain`.
    fn max_uw_path(domain: &str, i: usize) -> String {
        format!("{domain}/constraint_{i}_max_power_uw")
    }

    /// `constraint_N_power_limit_uw` path for slot `i` of `domain`.
    fn limit_uw_path(domain: &str, i: usize) -> String {
        format!("{domain}/constraint_{i}_power_limit_uw")
    }

    /// Discovered `(min, max)` in Watts for constraint slot `i` of `domain`.
    ///
    /// `None` when the firmware does not expose `constraint_N_max_power_uw` or
    /// exposes it as 0, or when the range is degenerate (`max <= min`). Callers
    /// must treat `None` as "do not write this constraint".
    fn constraint_bounds_watts_in(&self, domain: &str, i: usize) -> Option<(u32, u32)> {
        let max_uw = self.root.read_u64(&Self::max_uw_path(domain, i))?;
        if max_uw == 0 {
            return None;
        }
        let min_uw = self
            .root
            .read_u64(&format!("{domain}/constraint_{i}_min_power_uw"))
            .unwrap_or(0);
        let min = (min_uw / 1_000_000) as u32;
        let max = (max_uw / 1_000_000) as u32;
        if max <= min {
            return None;
        }
        Some((min, max))
    }

    /// Union of the discovered `(min, max)` of every zone exposing constraint `name`.
    ///
    /// The union (floor of the floors, ceiling of the ceilings) is what a *display*
    /// range can honestly show on a multi-package machine; each individual write still
    /// clamps inside its own zone's range.
    fn bounds_of(&self, name: &str) -> Option<(u32, u32)> {
        self.domains
            .iter()
            .filter_map(|domain| {
                self.constraint_index_in(domain, name)
                    .and_then(|i| self.constraint_bounds_watts_in(domain, i))
            })
            .reduce(|(lo, hi), (min, max)| (lo.min(min), hi.max(max)))
    }

    /// Writes `watts` to the constraint called `name` in **every** discovered zone that
    /// exposes it, clamped inside that zone's own discovered range.
    ///
    /// Returns the value written to the first zone that accepted the write, or `None`
    /// when no zone exposed both the constraint and a usable range — then nothing was
    /// written at all.
    pub fn write_constraint(&self, name: &str, watts: u32) -> Option<u32> {
        let mut written: Option<u32> = None;
        let mut exposed = false;
        for domain in &self.domains {
            let Some(i) = self.constraint_index_in(domain, name) else {
                continue;
            };
            exposed = true;
            let Some((min, max)) = self.constraint_bounds_watts_in(domain, i) else {
                debug!(
                    "RAPL: constraint '{name}' in {domain} does not expose a usable range \
                     (constraint_{i}_max_power_uw missing or 0); not writing it"
                );
                continue;
            };
            let limit_path = Self::limit_uw_path(domain, i);
            if !self.root.exists(&limit_path) {
                debug!("RAPL: {limit_path} is not exposed by the kernel; not writing it");
                continue;
            }
            let clamped = watts.clamp(min, max);
            self.root
                .write_best_effort(&limit_path, &(clamped as u64 * 1_000_000).to_string());
            written.get_or_insert(clamped);
        }
        if !exposed {
            debug!("RAPL: constraint '{name}' not exposed by any discovered zone; not writing");
        }
        written
    }

    /// Range used for display/stepping. Discovery first: the union of the per-constraint
    /// ranges (`constraint_N_{min,max}_power_uw`) across every zone, then the package
    /// `*_power_range_uw`, and only if nothing is discoverable the legacy `5..115 W`
    /// fallback. **Never** used to pick a written value — writes go through
    /// [`LinuxRapl::write_constraint`].
    fn bounds(&self) -> (u32, u32) {
        let mut discovered: Option<(u32, u32)> = None;
        for name in [LONG_TERM, SHORT_TERM] {
            if let Some((min, max)) = self.bounds_of(name) {
                discovered = Some(match discovered {
                    Some((lo, hi)) => (lo.min(min), hi.max(max)),
                    None => (min, max),
                });
            }
        }
        if let Some(bounds) = discovered {
            return bounds;
        }

        let mut package: Option<(u32, u32)> = None;
        for domain in &self.domains {
            if let (Some(min), Some(max)) = (
                self.root
                    .read_u64(&format!("{domain}/min_power_range_uw"))
                    .map(|v| (v / 1_000_000) as u32),
                self.root
                    .read_u64(&format!("{domain}/max_power_range_uw"))
                    .map(|v| (v / 1_000_000) as u32),
            ) {
                package = Some(match package {
                    Some((lo, hi)) => (lo.min(min), hi.max(max)),
                    None => (min, max),
                });
            }
        }
        package.unwrap_or((FALLBACK_MIN_WATTS, FALLBACK_MAX_WATTS))
    }

    fn get_limit(&self, constraint: &str) -> u32 {
        // Go `getRAPLPath` returning "" makes the getter return 0; the first zone that
        // exposes the constraint is the one reported.
        self.domains
            .iter()
            .find_map(|domain| {
                self.constraint_index_in(domain, constraint).map(|i| {
                    (self
                        .root
                        .read_u64(&Self::limit_uw_path(domain, i))
                        .unwrap_or(0)
                        / 1_000_000) as u32
                })
            })
            .unwrap_or(0)
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
        self.bounds_of(LONG_TERM)
    }

    /// Discovered `(min, max)` Watts of the PL2 (`short_term`) constraint, if any.
    fn pl2_bounds_watts(&self) -> Option<(u32, u32)> {
        self.bounds_of(SHORT_TERM)
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

    /// Zone name used by the unit tests. RAPL discovery is name-agnostic; the Intel
    /// spelling is kept here only because it is what the two measured machines expose.
    const RAPL_BASE: &str = "sys/class/powercap/intel-rapl:0";

    /// A powercap class holding one discoverable zone with the given name.
    fn root_with_zone(tag: &str, name: &str) -> (SysfsRoot, String) {
        let dir = std::env::temp_dir().join(format!("ww_rapl_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let rel = format!("{POWERCAP_BASE}/{name}");
        fs::create_dir_all(dir.join(&rel)).unwrap();
        // A powercap zone is one that exposes a constraint table.
        fs::write(dir.join(&rel).join("constraint_0_name"), "long_term\n").unwrap();
        (SysfsRoot::new(dir), rel)
    }

    fn root(tag: &str) -> SysfsRoot {
        root_with_zone(tag, "intel-rapl:0").0
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
        // No usable constraint range at all -> Go returns 0 instead of erroring.
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

    /// The zone is discovered by *content*, not by a vendor name: an AMD/future
    /// spelling (`amd-rapl:0`, `intel-rapl-mmio:0`, …) works exactly like
    /// `intel-rapl:0`.
    #[test]
    fn a_non_intel_powercap_zone_name_is_discovered() {
        let (root, zone) = root_with_zone("amdnamed", "amd-rapl:0");
        write(
            &root,
            &format!("{zone}/constraint_0_max_power_uw"),
            "65000000\n",
        );
        write(
            &root,
            &format!("{zone}/constraint_0_power_limit_uw"),
            "45000000\n",
        );

        let rapl = LinuxRapl::with_root(root.clone()).unwrap();
        assert_eq!(rapl.domains().len(), 1);
        assert_eq!(rapl.domains()[0], zone);
        assert_eq!(rapl.pl1_bounds_watts(), Some((0, 65)));
        assert_eq!(rapl.pl1_watts().unwrap(), 45);
        // 999 clamps to *this* zone's ceiling, never the Intel 115 W literal.
        assert_eq!(rapl.write_constraint(LONG_TERM, 999), Some(65));
        assert_eq!(
            root.read(&format!("{zone}/constraint_0_power_limit_uw")),
            "65000000"
        );
    }

    /// Several zones (multi-package) are all discovered and all written, each inside
    /// its own range.
    #[test]
    fn every_discovered_zone_is_written_within_its_own_range() {
        let dir = std::env::temp_dir().join(format!("ww_rapl_multi_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = SysfsRoot::new(&dir);

        for (name, max_uw) in [
            ("intel-rapl:0", 45_000_000u64),
            ("intel-rapl:1", 15_000_000),
        ] {
            let zone = format!("{POWERCAP_BASE}/{name}");
            write(&root, &format!("{zone}/constraint_0_name"), "long_term\n");
            write(
                &root,
                &format!("{zone}/constraint_0_max_power_uw"),
                &format!("{max_uw}\n"),
            );
            write(
                &root,
                &format!("{zone}/constraint_0_power_limit_uw"),
                "1000000\n",
            );
        }

        let rapl = LinuxRapl::with_root(root.clone()).unwrap();
        assert_eq!(rapl.domains().len(), 2);
        // Union for display: floor of the floors, ceiling of the ceilings.
        assert_eq!(rapl.pl1_bounds_watts(), Some((0, 45)));

        rapl.set_pl1_watts(999).unwrap();
        assert_eq!(
            root.read(&format!(
                "{POWERCAP_BASE}/intel-rapl:0/constraint_0_power_limit_uw"
            )),
            "45000000"
        );
        assert_eq!(
            root.read(&format!(
                "{POWERCAP_BASE}/intel-rapl:1/constraint_0_power_limit_uw"
            )),
            "15000000"
        );
    }

    /// Entries under `/sys/class/powercap` that are not constraint zones are ignored:
    /// the presence of the class alone must not make the controller claim RAPL.
    #[test]
    fn powercap_entries_without_constraints_are_not_zones() {
        let dir = std::env::temp_dir().join(format!("ww_rapl_nozone_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(POWERCAP_BASE).join("some-other-controller")).unwrap();
        // A zone with `name`/`enabled` but no constraint table is not a control zone.
        write(
            &SysfsRoot::new(&dir),
            &format!("{POWERCAP_BASE}/some-other-controller/name"),
            "not-rapl\n",
        );

        assert!(LinuxRapl::with_root(SysfsRoot::new(dir)).is_err());
    }
}
