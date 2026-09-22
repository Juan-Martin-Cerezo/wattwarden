use crate::sysfs::{read_sysfs_u64, write_sysfs_u64, SysfsRoot};
use std::path::PathBuf;
use wattwarden_core::{ChargeThreshold, Result, WattWardenError};

/// Battery charge ceiling. The discovery ladder is the Rust-side extension (Go master only
/// exposes the kbd/wifi knobs through rfkill); the *paths* must still resolve through the
/// relocated `SysfsRoot` so the whole backend stays testable without touching real `/sys`.
pub struct LinuxChargeThreshold {
    threshold_path: Option<PathBuf>,
    root: SysfsRoot,
}

impl LinuxChargeThreshold {
    pub fn new() -> Self {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Self {
        let threshold_path = Self::find_threshold_path(&root);
        Self {
            threshold_path,
            root,
        }
    }

    pub fn with_path(threshold_path: Option<PathBuf>) -> Self {
        Self {
            threshold_path,
            root: SysfsRoot::from_env(),
        }
    }

    fn find_threshold_path(root: &SysfsRoot) -> Option<PathBuf> {
        let base = "sys/class/power_supply";
        let candidates = ["BAT0", "BAT1", "BAT2", "BATT"];
        let suffixes = [
            // Linux kernel generic ACPI standard
            "charge_control_end_threshold",
            // Lenovo ThinkPad (tp_smapi / thinkpad_acpi)
            "charge_stop_threshold",
            // ASUS alternate path
            "charge_control_limit_max",
        ];
        for bat in &candidates {
            for suffix in &suffixes {
                let p = root.path(&format!("{base}/{bat}/{suffix}"));
                if p.exists() {
                    return Some(p);
                }
            }
        }

        // Platform-driver direct paths (ASUS / Huawei)
        for direct in [
            "sys/devices/platform/asus-nb-wmi/charge_control_end_threshold",
            "sys/devices/platform/huawei-wmi/charge_thresholds",
        ] {
            let p = root.path(direct);
            if p.exists() {
                return Some(p);
            }
        }

        // Generic fallback scan across all power supply entries
        for entry in root.entries(base) {
            for suffix in ["charge_control_end_threshold", "charge_stop_threshold"] {
                let p = entry.join(suffix);
                if p.exists() {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Root the backend was built against (kept for diagnostics/tests).
    pub fn root(&self) -> &SysfsRoot {
        &self.root
    }
}

impl Default for LinuxChargeThreshold {
    fn default() -> Self {
        Self::new()
    }
}

impl ChargeThreshold for LinuxChargeThreshold {
    fn supports_threshold(&self) -> bool {
        self.threshold_path.is_some()
    }

    fn charge_threshold(&self) -> Result<u8> {
        if let Some(p) = &self.threshold_path {
            let val = read_sysfs_u64(p)?;
            Ok(val.clamp(1, 100) as u8)
        } else {
            Err(WattWardenError::Unsupported(
                "Hardware does not expose charge_control_end_threshold".into(),
            ))
        }
    }

    fn set_charge_threshold(&self, threshold: u8) -> Result<()> {
        let clamped = threshold.clamp(20, 100);
        if let Some(p) = &self.threshold_path {
            write_sysfs_u64(p, clamped as u64)
        } else {
            Err(WattWardenError::Unsupported(
                "Hardware does not support setting battery charge limit threshold".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn discovery_and_io_go_through_the_relocated_root() {
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
}
