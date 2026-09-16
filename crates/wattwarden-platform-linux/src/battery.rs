use crate::sysfs::{read_sysfs_i64, read_sysfs_string, read_sysfs_u64};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{PowerSource, Result, WattWardenError};

pub struct LinuxBattery {
    battery_path: PathBuf,
    ac_path: Option<PathBuf>,
}

impl LinuxBattery {
    pub fn new() -> Result<Self> {
        let battery_path = Self::find_battery_path()?;
        let ac_path = Self::find_ac_path();
        Ok(Self {
            battery_path,
            ac_path,
        })
    }

    fn find_battery_path() -> Result<PathBuf> {
        let base = Path::new("/sys/class/power_supply");
        for candidate in &["BAT0", "BAT1", "BAT2", "BATT"] {
            let p = base.join(candidate);
            if p.exists() {
                return Ok(p);
            }
        }

        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let p = entry.path();
                let type_file = p.join("type");
                if let Ok(content) = fs::read_to_string(&type_file) {
                    if content.trim().eq_ignore_ascii_case("battery") {
                        return Ok(p);
                    }
                }
            }
        }

        Err(WattWardenError::InterfaceNotFound(
            "No active battery found under /sys/class/power_supply".into(),
        ))
    }

    fn find_ac_path() -> Option<PathBuf> {
        let base = Path::new("/sys/class/power_supply");
        for candidate in &["AC", "ACAD", "ADP0", "ADP1"] {
            let p = base.join(candidate);
            if p.exists() {
                return Some(p);
            }
        }
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let p = entry.path();
                let type_file = p.join("type");
                if let Ok(content) = fs::read_to_string(&type_file) {
                    if content.trim().eq_ignore_ascii_case("mains") {
                        return Some(p);
                    }
                }
            }
        }
        None
    }

    fn parse_uevent(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let uevent_path = self.battery_path.join("uevent");
        if let Ok(content) = fs::read_to_string(uevent_path) {
            for line in content.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    map.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        map
    }
}

impl PowerSource for LinuxBattery {
    fn battery_percentage(&self) -> Result<u8> {
        let cap = read_sysfs_u64(self.battery_path.join("capacity"))
            .or_else(|_| {
                let uevent = self.parse_uevent();
                uevent
                    .get("POWER_SUPPLY_CAPACITY")
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or_else(|| {
                        WattWardenError::InterfaceNotFound("POWER_SUPPLY_CAPACITY not found".into())
                    })
            })?;
        Ok(cap.min(100) as u8)
    }

    fn is_charging(&self) -> Result<bool> {
        if let Some(ac) = &self.ac_path {
            if let Ok(online) = read_sysfs_u64(ac.join("online")) {
                return Ok(online == 1);
            }
        }

        let status = read_sysfs_string(self.battery_path.join("status"))
            .unwrap_or_else(|_| {
                let uevent = self.parse_uevent();
                uevent.get("POWER_SUPPLY_STATUS").cloned().unwrap_or_default()
            });

        Ok(status.eq_ignore_ascii_case("charging") || status.eq_ignore_ascii_case("full"))
    }

    fn consumption_watts(&self) -> Result<f64> {
        // 1. Try direct power_now in microwatts
        if let Ok(power_uw) = read_sysfs_i64(self.battery_path.join("power_now")) {
            return Ok((power_uw.abs() as f64) / 1_000_000.0);
        }

        // 2. Fallback: current_now (microamperes) * voltage_now (microvolts)
        let cur = read_sysfs_i64(self.battery_path.join("current_now")).ok();
        let volt = read_sysfs_i64(self.battery_path.join("voltage_now")).ok();
        if let (Some(c), Some(v)) = (cur, volt) {
            let watts = (c.abs() as f64) * (v.abs() as f64) / 1_000_000_000_000.0;
            return Ok(watts);
        }

        // 3. Last fallback: parse uevent
        let uevent = self.parse_uevent();
        if let Some(p_str) = uevent.get("POWER_SUPPLY_POWER_NOW") {
            if let Ok(p_uw) = p_str.parse::<i64>() {
                return Ok((p_uw.abs() as f64) / 1_000_000.0);
            }
        }
        if let (Some(c_str), Some(v_str)) = (
            uevent.get("POWER_SUPPLY_CURRENT_NOW"),
            uevent.get("POWER_SUPPLY_VOLTAGE_NOW"),
        ) {
            if let (Ok(c), Ok(v)) = (c_str.parse::<i64>(), v_str.parse::<i64>()) {
                return Ok((c.abs() as f64) * (v.abs() as f64) / 1_000_000_000_000.0);
            }
        }

        Ok(0.0)
    }

    fn time_remaining(&self) -> Result<String> {
        if self.is_charging()? {
            return Ok("Charging (AC)".into());
        }

        let watts = self.consumption_watts()?;
        if watts <= 0.05 {
            return Ok("Calculating...".into());
        }

        // Try reading energy_now (micro-watt-hours)
        let energy_uwh: u64 = if let Ok(e) = read_sysfs_u64(self.battery_path.join("energy_now")) {
            e
        } else if let (Ok(c), Ok(v)) = (
            read_sysfs_u64(self.battery_path.join("charge_now")),
            read_sysfs_u64(self.battery_path.join("voltage_now")),
        ) {
            c * v / 1_000_000
        } else {
            let uevent = self.parse_uevent();
            if let Some(e) = uevent.get("POWER_SUPPLY_ENERGY_NOW").and_then(|s| s.parse::<u64>().ok()) {
                e
            } else if let (Some(c), Some(v)) = (
                uevent.get("POWER_SUPPLY_CHARGE_NOW").and_then(|s| s.parse::<u64>().ok()),
                uevent.get("POWER_SUPPLY_VOLTAGE_NOW").and_then(|s| s.parse::<u64>().ok()),
            ) {
                c * v / 1_000_000
            } else {
                return Err(WattWardenError::InterfaceNotFound("No energy metric available".into()));
            }
        };

        let energy_wh = (energy_uwh as f64) / 1_000_000.0;
        let hours_total = energy_wh / watts;
        let h = hours_total.floor() as u32;
        let m = ((hours_total - (h as f64)) * 60.0).round() as u32;

        Ok(format!("{}h {:02}m", h, m))
    }
}
