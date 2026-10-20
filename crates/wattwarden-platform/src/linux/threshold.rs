use crate::sysfs::{read_sysfs_u64, write_sysfs_u64, SysfsRoot};
use std::path::PathBuf;
use wattwarden_core::{ChargeThreshold, Result, WattWardenError};

/// One battery's charge-threshold control nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThresholdNode {
    pub end: PathBuf,
    pub start: Option<PathBuf>,
    /// Dell & co: `charge_types` selects the EC charging mode. User
    /// thresholds only bind in `Custom` mode — in `Adaptive`/`Standard` the
    /// firmware manages charging itself and silently ignores them (this is
    /// what made the limit "not work" on the Dell G15 despite clean writes).
    pub types: Option<PathBuf>,
}

/// End-threshold file names, in driver-precedence order.
const END_SUFFIXES: &[&str] = &[
    // Linux kernel generic ACPI standard
    "charge_control_end_threshold",
    // Lenovo ThinkPad (tp_smapi / thinkpad_acpi)
    "charge_stop_threshold",
    // ASUS alternate path
    "charge_control_limit_max",
];

/// Paired start-threshold file (the kernel rejects `end <= start`).
const START_SUFFIX: &str = "charge_control_start_threshold";

/// Charge-mode selector (`Trickle Fast Standard Adaptive Custom`, current
/// mode in brackets). Present on Dell and several other vendors.
const TYPES_SUFFIX: &str = "charge_types";

/// The mode in which vendor firmware honours user charge thresholds.
const TYPES_CUSTOM: &str = "Custom";

/// Platform-driver direct paths (ASUS / Huawei), outside the power_supply tree.
const DIRECT_END_PATHS: &[&str] = &[
    "sys/devices/platform/asus-nb-wmi/charge_control_end_threshold",
    "sys/devices/platform/huawei-wmi/charge_thresholds",
];

/// Battery charge ceiling. Discovery is **fresh on every operation**: battery
/// nodes that appear after boot (slow udev, late module load, hot-swapped pack)
/// are picked up, which is what makes the limit apply "always" instead of only
/// when the tree happened to be complete at process start.
pub struct LinuxChargeThreshold {
    /// Test override: operate on exactly these nodes instead of discovering.
    fixed: Option<Vec<ThresholdNode>>,
    root: SysfsRoot,
}

impl LinuxChargeThreshold {
    pub fn new() -> Self {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Self {
        Self { fixed: None, root }
    }

    pub fn with_path(threshold_path: Option<PathBuf>) -> Self {
        let fixed = threshold_path.map(|end| {
            let dir = end.parent().map(|p| p.to_path_buf());
            let sibling = |name: &str| dir.as_ref().map(|d| d.join(name)).filter(|p| p.exists());
            vec![ThresholdNode {
                end,
                start: sibling(START_SUFFIX),
                types: sibling(TYPES_SUFFIX),
            }]
        });
        Self {
            fixed,
            root: SysfsRoot::from_env(),
        }
    }

    /// Root the backend was built against (kept for diagnostics/tests).
    pub fn root(&self) -> &SysfsRoot {
        &self.root
    }

    /// Every threshold-capable node on this machine, resolved now.
    pub fn nodes(&self) -> Vec<ThresholdNode> {
        if let Some(fixed) = &self.fixed {
            return fixed.clone();
        }
        self.discover()
    }

    fn discover(&self) -> Vec<ThresholdNode> {
        let mut nodes = Vec::new();

        // Any power supply entry exposing a known end-threshold file, whatever
        // it is called (BAT0, BATT, cm_battery, macsmc-battery, ...).
        for dir in self.root.entries("sys/class/power_supply") {
            for suffix in END_SUFFIXES {
                let end = dir.join(suffix);
                if end.exists() {
                    let start = dir.join(START_SUFFIX);
                    let types = dir.join(TYPES_SUFFIX);
                    nodes.push(ThresholdNode {
                        end,
                        start: start.exists().then_some(start),
                        types: types.exists().then_some(types),
                    });
                    break;
                }
            }
        }

        for direct in DIRECT_END_PATHS {
            let end = self.root.path(direct);
            if end.exists() {
                let start = end
                    .parent()
                    .map(|d| d.join(START_SUFFIX))
                    .filter(|p| p.exists());
                let types = end
                    .parent()
                    .map(|d| d.join(TYPES_SUFFIX))
                    .filter(|p| p.exists());
                nodes.push(ThresholdNode { end, start, types });
            }
        }

        nodes
    }

    /// Writes `value` to one node and reads it back. When the paired start
    /// threshold would make the kernel reject the write (`end <= start`), the
    /// start is lowered first — the only case where it is touched.
    fn write_node(node: &ThresholdNode, value: u64) -> Result<u64> {
        // Dell-style ECs only honour the thresholds in `Custom` charging
        // mode; in `Adaptive` the firmware silently re-manages charging.
        if let Some(types_path) = &node.types {
            let current = std::fs::read_to_string(types_path)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if !current.contains(&format!("[{TYPES_CUSTOM}]")) {
                let _ = std::fs::write(types_path, TYPES_CUSTOM);
            }
        }
        if let Some(start_path) = &node.start {
            if matches!(read_sysfs_u64(start_path), Ok(s) if s >= value) {
                let new_start = value.saturating_sub(5).max(1);
                let _ = write_sysfs_u64(start_path, new_start);
            }
        }
        write_sysfs_u64(&node.end, value)?;
        let readback = read_sysfs_u64(&node.end)?;
        if readback != value {
            return Err(WattWardenError::Unsupported(format!(
                "charge threshold did not stick: wrote {value}, hardware reports {readback}"
            )));
        }
        Ok(readback)
    }
}

impl Default for LinuxChargeThreshold {
    fn default() -> Self {
        Self::new()
    }
}

impl ChargeThreshold for LinuxChargeThreshold {
    fn supports_threshold(&self) -> bool {
        !self.nodes().is_empty()
    }

    fn charge_threshold(&self) -> Result<u8> {
        for node in self.nodes() {
            if let Ok(val) = read_sysfs_u64(&node.end) {
                return Ok(val.clamp(1, 100) as u8);
            }
        }
        Err(WattWardenError::Unsupported(
            "Hardware does not expose a charge threshold".into(),
        ))
    }

    fn set_charge_threshold(&self, threshold: u8) -> Result<()> {
        let nodes = self.nodes();
        if nodes.is_empty() {
            return Err(WattWardenError::Unsupported(
                "Hardware does not support setting battery charge limit threshold".into(),
            ));
        }

        let clamped = threshold.clamp(20, 100) as u64;
        let mut applied = 0;
        let mut last_err = None;
        for node in &nodes {
            match Self::write_node(node, clamped) {
                Ok(_) => applied += 1,
                Err(e) => last_err = Some(e),
            }
        }

        if applied > 0 {
            Ok(())
        } else {
            Err(last_err.unwrap_or_else(|| {
                WattWardenError::Unsupported("No battery accepted a charge threshold".into())
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_root(tag: &str, with_threshold: bool) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_thresh_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = SysfsRoot::new(&dir);
        std::fs::create_dir_all(root.path("sys/class/power_supply/BAT0")).unwrap();
        std::fs::write(root.path("sys/class/power_supply/BAT0/type"), "Battery\n").unwrap();
        if with_threshold {
            std::fs::write(
                root.path("sys/class/power_supply/BAT0/charge_control_end_threshold"),
                "80\n",
            )
            .unwrap();
        }
        root
    }

    #[test]
    fn discovery_and_io_use_the_relocated_root() {
        let root = fake_root("disc", true);
        let thresh = LinuxChargeThreshold::with_root(root.clone());
        assert!(thresh.supports_threshold());
        assert_eq!(thresh.charge_threshold().unwrap(), 80);

        thresh.set_charge_threshold(85).unwrap();
        let raw = std::fs::read_to_string(
            root.path("sys/class/power_supply/BAT0/charge_control_end_threshold"),
        )
        .unwrap();
        assert_eq!(raw, "85");

        // Requesting 200 clamps to 100 before it ever reaches the file.
        thresh.set_charge_threshold(200).unwrap();
        let raw = std::fs::read_to_string(
            root.path("sys/class/power_supply/BAT0/charge_control_end_threshold"),
        )
        .unwrap();
        assert_eq!(raw, "100");

        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn missing_threshold_is_reported_as_unsupported_not_a_panic() {
        let root = fake_root("none", false);
        let thresh = LinuxChargeThreshold::with_root(root.clone());
        assert!(!thresh.supports_threshold());
        assert!(thresh.charge_threshold().is_err());
        assert!(thresh.set_charge_threshold(80).is_err());

        let _ = std::fs::remove_dir_all(root.root());
    }

    #[test]
    fn with_path_keeps_working_for_callers_that_already_resolved_the_node() {
        let root = fake_root("path", true);
        let node = root.path("sys/class/power_supply/BAT0/charge_control_end_threshold");
        let thresh = LinuxChargeThreshold::with_path(Some(node.clone()));
        assert_eq!(thresh.charge_threshold().unwrap(), 80);
        thresh.set_charge_threshold(70).unwrap();
        assert_eq!(std::fs::read_to_string(&node).unwrap(), "70");

        let _ = std::fs::remove_dir_all(root.root());
    }

    /// The whole point of lazy discovery: a node that appears after the
    /// backend was constructed must still be used.
    #[test]
    fn nodes_discovered_after_construction_are_used() {
        let root = fake_root("late", false);
        let thresh = LinuxChargeThreshold::with_root(root.clone());
        assert!(!thresh.supports_threshold(), "nothing there yet");

        let bat = root.path("sys/class/power_supply/BAT1");
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("charge_control_end_threshold"), "80\n").unwrap();

        assert!(thresh.supports_threshold(), "rediscovered on demand");
        thresh.set_charge_threshold(60).unwrap();
        assert_eq!(
            std::fs::read_to_string(bat.join("charge_control_end_threshold")).unwrap(),
            "60"
        );

        let _ = std::fs::remove_dir_all(root.root());
    }

    /// Odd battery names are found by the generic scan, not just BAT0..BAT3.
    #[test]
    fn any_power_supply_name_is_discovered() {
        let root = fake_root("names", false);
        let bat = root.path("sys/class/power_supply/macsmc-battery");
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("charge_stop_threshold"), "90\n").unwrap();

        let thresh = LinuxChargeThreshold::with_root(root.clone());
        assert!(thresh.supports_threshold());
        thresh.set_charge_threshold(75).unwrap();
        assert_eq!(
            std::fs::read_to_string(bat.join("charge_stop_threshold")).unwrap(),
            "75"
        );

        let _ = std::fs::remove_dir_all(root.root());
    }

    /// `end <= start` is what the kernel rejects: the start threshold is
    /// lowered only when it would block the write, and never otherwise.
    #[test]
    fn blocking_start_threshold_is_lowered_and_valid_one_is_left_alone() {
        let root = fake_root("start", false);
        let bat = root.path("sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat).unwrap();
        std::fs::write(bat.join("charge_control_end_threshold"), "80\n").unwrap();
        std::fs::write(bat.join("charge_control_start_threshold"), "80\n").unwrap();

        let thresh = LinuxChargeThreshold::with_root(root.clone());
        thresh.set_charge_threshold(70).unwrap();
        assert_eq!(
            std::fs::read_to_string(bat.join("charge_control_end_threshold")).unwrap(),
            "70"
        );
        assert_eq!(
            std::fs::read_to_string(bat.join("charge_control_start_threshold")).unwrap(),
            "65",
            "start lowered to keep start < end"
        );

        // A valid window (50-70) is untouched by a 100% ceiling write.
        thresh.set_charge_threshold(100).unwrap();
        assert_eq!(
            std::fs::read_to_string(bat.join("charge_control_start_threshold")).unwrap(),
            "65"
        );

        let _ = std::fs::remove_dir_all(root.root());
    }

    /// Every battery gets the ceiling, and one broken node cannot fail the rest.
    #[test]
    fn all_batteries_receive_the_limit() {
        let root = fake_root("multi", true);
        let bat1 = root.path("sys/class/power_supply/BAT1");
        std::fs::create_dir_all(&bat1).unwrap();
        std::fs::write(bat1.join("charge_control_end_threshold"), "100\n").unwrap();
        // A directory that exposes a threshold name but rejects writes.
        let ro = root.path("sys/class/power_supply/BAT2");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::write(ro.join("charge_control_end_threshold"), "100\n").unwrap();
        let _ = std::fs::set_permissions(
            ro.join("charge_control_end_threshold"),
            std::fs::Permissions::from_mode(0o444),
        );

        let thresh = LinuxChargeThreshold::with_root(root.clone());
        assert!(thresh.set_charge_threshold(80).is_ok());
        assert_eq!(
            std::fs::read_to_string(
                root.path("sys/class/power_supply/BAT0/charge_control_end_threshold")
            )
            .unwrap(),
            "80"
        );
        assert_eq!(
            std::fs::read_to_string(bat1.join("charge_control_end_threshold")).unwrap(),
            "80"
        );

        let _ = std::fs::remove_dir_all(root.root());
    }

    /// The Dell G15 regression: in `Adaptive` charging mode the EC ignores
    /// user thresholds, so setting a limit must switch the battery to
    /// `Custom` mode first — and stay there when it already is.
    #[test]
    fn setting_a_limit_switches_dell_style_charge_types_to_custom() {
        let root = fake_root("dell", true);
        let types = root.path("sys/class/power_supply/BAT0/charge_types");
        std::fs::write(&types, "Trickle Fast Standard [Adaptive] Custom\n").unwrap();

        let thresh = LinuxChargeThreshold::with_root(root.clone());
        thresh.set_charge_threshold(75).unwrap();
        assert_eq!(
            std::fs::read_to_string(&types).unwrap().trim(),
            "Custom",
            "Adaptive mode would silently ignore the threshold"
        );
        assert_eq!(
            std::fs::read_to_string(
                root.path("sys/class/power_supply/BAT0/charge_control_end_threshold")
            )
            .unwrap(),
            "75"
        );

        // Already custom: the mode file is not rewritten (EC churn).
        std::fs::write(&types, "Trickle Fast Standard Adaptive [Custom]\n").unwrap();
        thresh.set_charge_threshold(65).unwrap();
        assert_eq!(
            std::fs::read_to_string(&types).unwrap().trim(),
            "Trickle Fast Standard Adaptive [Custom]"
        );

        let _ = std::fs::remove_dir_all(root.root());
    }
}
