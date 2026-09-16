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

    fn find_threshold_path() -> Option<PathBuf> {
        let base = Path::new("/sys/class/power_supply");
        let candidates = ["BAT0", "BAT1", "BAT2", "BATT"];
        for bat in &candidates {
            let p = base.join(bat).join("charge_control_end_threshold");
            if p.exists() {
                return Some(p);
            }
            // ASUS alternate path
            let asus_p = base.join(bat).join("charge_control_limit_max");
            if asus_p.exists() {
                return Some(asus_p);
            }
        }

        // Generic fallback scan
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let p = entry.path().join("charge_control_end_threshold");
                if p.exists() {
                    return Some(p);
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
