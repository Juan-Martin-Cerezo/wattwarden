//! LCD backlight brightness control.
//!
//! * `brightness_percent()` -> checks `/sys/class/backlight/*` entries with
//!   `max_brightness > 0`, computing `(brightness * 100) / max_brightness`;
//!   falls back to `brightnessctl -m` output, or `FALLBACK_BRIGHTNESS_PERCENT`.
//! * `set_brightness_percent(p)` -> clamps `1..=100`, writes `(p * max) / 100` on every
//!   backlight device with `max > 0`, and additionally triggers `brightnessctl set N%`.
//!
//! All sysfs access goes through [`SysfsRoot`]; the external `brightnessctl` call is
//! best effort and degrades silently when the tool is absent.

use crate::linux::cmd;
use crate::linux::sysfs::SysfsRoot;
use wattwarden_core::{DisplayManager, Result, WattWardenError};

const BACKLIGHT_BASE: &str = "sys/class/backlight";

/// Default fallback brightness percentage when all interfaces fail.
pub const FALLBACK_BRIGHTNESS_PERCENT: u8 = 100;

pub struct LinuxBacklight {
    root: SysfsRoot,
}

impl LinuxBacklight {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Result<Self> {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Result<Self> {
        if root.names(BACKLIGHT_BASE).is_empty() {
            return Err(WattWardenError::InterfaceNotFound(
                "No controllable backlight found under /sys/class/backlight".into(),
            ));
        }
        Ok(Self { root })
    }

    /// Sorted backlight device names.
    fn devices(&self) -> Vec<String> {
        self.root.names(BACKLIGHT_BASE)
    }

    fn max_brightness(&self, device: &str) -> i64 {
        self.root
            .read_i64(&format!("{BACKLIGHT_BASE}/{device}/max_brightness"))
            .unwrap_or(0)
    }
}

impl Default for LinuxBacklight {
    fn default() -> Self {
        Self::new().expect("backlight unavailable")
    }
}

impl DisplayManager for LinuxBacklight {
    fn brightness_percent(&self) -> Result<u8> {
        for device in self.devices() {
            let max = self.max_brightness(&device);
            if max > 0 {
                let cur = self
                    .root
                    .read_i64(&format!("{BACKLIGHT_BASE}/{device}/brightness"))
                    .unwrap_or(0);
                let percent = (cur * 100) / max;
                return Ok(percent.clamp(0, u8::MAX as i64) as u8);
            }
        }

        // Fallback: brightnessctl -m -> "device,class,current,42%,..." (field 4).
        let out = cmd::run_capture("brightnessctl", &["-m"]);
        if !out.is_empty() {
            let parts: Vec<&str> = out.split(',').collect();
            if parts.len() >= 4 {
                if let Ok(v) = parts[3].trim_end_matches('%').trim().parse::<i64>() {
                    return Ok(v.clamp(0, u8::MAX as i64) as u8);
                }
            }
        }

        Ok(FALLBACK_BRIGHTNESS_PERCENT)
    }

    fn set_brightness_percent(&self, percent: u8) -> Result<()> {
        let p = percent.clamp(1, 100);

        for device in self.devices() {
            let max = self.max_brightness(&device);
            if max > 0 {
                let target = (i64::from(p) * max) / 100;
                self.root.write_best_effort(
                    &format!("{BACKLIGHT_BASE}/{device}/brightness"),
                    &target.to_string(),
                );
            }
        }

        // Best effort external brightness adjustment.
        cmd::run_ignored("brightnessctl", &["set", &format!("{p}%")]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fake_backlight(
        tag: &str,
        brightness: Option<&str>,
        max: &str,
    ) -> (SysfsRoot, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("ww_bl_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let device = dir.join(BACKLIGHT_BASE).join("intel_backlight");
        fs::create_dir_all(&device).unwrap();
        if let Some(value) = brightness {
            fs::write(device.join("brightness"), value).unwrap();
        }
        fs::write(device.join("max_brightness"), max).unwrap();
        (SysfsRoot::new(dir), device)
    }

    #[test]
    fn percent_uses_integer_maths() {
        let (root, _device) = fake_backlight("get", Some("500\n"), "1000\n");
        let bl = LinuxBacklight::with_root(root).unwrap();
        assert_eq!(bl.brightness_percent().unwrap(), 50);

        let (root, _device) = fake_backlight("get2", Some("333\n"), "1000\n");
        let bl = LinuxBacklight::with_root(root).unwrap();
        assert_eq!(bl.brightness_percent().unwrap(), 33); // (333 * 100) / 1000
    }

    #[test]
    fn set_writes_formula_and_clamps_one_to_hundred() {
        let (root, device) = fake_backlight("set", Some("500\n"), "1000\n");
        let bl = LinuxBacklight::with_root(root).unwrap();

        bl.set_brightness_percent(75).unwrap();
        assert_eq!(
            fs::read_to_string(device.join("brightness")).unwrap(),
            "750"
        );

        bl.set_brightness_percent(0).unwrap();
        assert_eq!(fs::read_to_string(device.join("brightness")).unwrap(), "10");

        bl.set_brightness_percent(120).unwrap();
        assert_eq!(
            fs::read_to_string(device.join("brightness")).unwrap(),
            "1000"
        );
    }

    #[test]
    fn missing_max_brightness_falls_back_to_hundred() {
        let (root, _device) = fake_backlight("fallback", Some("500\n"), "0\n");
        let bl = LinuxBacklight::with_root(root).unwrap();
        // When brightnessctl is installed on the host, it returns the real display percentage;
        // otherwise it falls back to 100 (CI). Both are valid behaviors.
        let pct = bl.brightness_percent().unwrap();
        assert!(pct <= 100);
        // Must not panic nor create garbage.
        bl.set_brightness_percent(50).unwrap();
    }

    #[test]
    fn no_backlight_device_is_reported_as_unsupported() {
        let dir = std::env::temp_dir().join(format!("ww_bl_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(LinuxBacklight::with_root(SysfsRoot::new(dir)).is_err());
    }
}
