use crate::sysfs::{read_sysfs_u64, write_sysfs_u64};
use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{ChargeThreshold, Result, WattWardenError};

pub struct LinuxChargeThreshold {
    threshold_path: Option<PathBuf>,
}

impl LinuxChargeThreshold {
    pub fn new() -> Self {
        let threshold_path = Self::find_threshold_path();
        Self { threshold_path }
    }

    pub fn with_path(threshold_path: Option<PathBuf>) -> Self {
        Self { threshold_path }
    }

    fn find_threshold_path() -> Option<PathBuf> {
        let base = Path::new("/sys/class/power_supply");
        let candidates = ["BAT0", "BAT1", "BAT2", "BATT"];
        for bat in &candidates {
            // Linux kernel generic ACPI standard
            let p = base.join(bat).join("charge_control_end_threshold");
            if p.exists() {
                return Some(p);
            }
            // Lenovo ThinkPad (tp_smapi / thinkpad_acpi)
            let tp_p = base.join(bat).join("charge_stop_threshold");
            if tp_p.exists() {
                return Some(tp_p);
            }
            // ASUS alternate path
            let asus_p = base.join(bat).join("charge_control_limit_max");
            if asus_p.exists() {
                return Some(asus_p);
            }
        }

        // ASUS platform driver direct path
        let asus_wmi = Path::new("/sys/devices/platform/asus-nb-wmi/charge_control_end_threshold");
        if asus_wmi.exists() {
            return Some(asus_wmi.to_path_buf());
        }

        // Huawei platform driver direct path
        let huawei_wmi = Path::new("/sys/devices/platform/huawei-wmi/charge_thresholds");
        if huawei_wmi.exists() {
            return Some(huawei_wmi.to_path_buf());
        }

        // Generic fallback scan across all power supply entries
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let p1 = entry.path().join("charge_control_end_threshold");
                if p1.exists() {
                    return Some(p1);
                }
                let p2 = entry.path().join("charge_stop_threshold");
                if p2.exists() {
                    return Some(p2);
                }
            }
        }
        None
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
    use std::fs;

    #[test]
    fn test_mock_threshold() {
        let tmp_dir = std::env::temp_dir().join(format!("ww_thresh_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp_dir);
        let test_file = tmp_dir.join("charge_control_end_threshold");

        fs::write(&test_file, "80\n").unwrap();

        let thresh = LinuxChargeThreshold::with_path(Some(test_file.clone()));
        assert!(thresh.supports_threshold());
        assert_eq!(thresh.charge_threshold().unwrap(), 80);

        thresh.set_charge_threshold(85).unwrap();
        assert_eq!(thresh.charge_threshold().unwrap(), 85);

        // Unsupported threshold
        let none_thresh = LinuxChargeThreshold::with_path(None);
        assert!(!none_thresh.supports_threshold());
        assert!(none_thresh.charge_threshold().is_err());
        assert!(none_thresh.set_charge_threshold(80).is_err());

        let _ = fs::remove_dir_all(tmp_dir);
    }
}
