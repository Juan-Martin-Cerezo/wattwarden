use crate::sysfs::{read_sysfs_u64, write_sysfs_u64};
use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{DisplayManager, Result, WattWardenError};

pub struct LinuxBacklight {
    device_path: PathBuf,
    max_brightness: u64,
}

impl LinuxBacklight {
    pub fn new() -> Result<Self> {
        let base = Path::new("/sys/class/backlight");
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let p = entry.path();
                let max_path = p.join("max_brightness");
                if let Ok(max) = read_sysfs_u64(&max_path) {
                    if max > 0 {
                        return Ok(Self {
                            device_path: p,
                            max_brightness: max,
                        });
                    }
                }
            }
        }

        Err(WattWardenError::InterfaceNotFound(
            "No controllable backlight found under /sys/class/backlight".into(),
        ))
    }
}

impl DisplayManager for LinuxBacklight {
    fn brightness_percent(&self) -> Result<u8> {
        let cur = read_sysfs_u64(self.device_path.join("brightness"))?;
        let percent = ((cur as f64) / (self.max_brightness as f64) * 100.0).round() as u8;
        Ok(percent.min(100))
    }

    fn set_brightness_percent(&self, percent: u8) -> Result<()> {
        let p = percent.clamp(1, 100);
        let target_raw = ((p as f64) / 100.0 * (self.max_brightness as f64)).round() as u64;
        let final_raw = target_raw.clamp(1, self.max_brightness);
        write_sysfs_u64(self.device_path.join("brightness"), final_raw)
    }
}
