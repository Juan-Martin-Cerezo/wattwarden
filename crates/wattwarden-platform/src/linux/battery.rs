//! Battery / AC telemetry.
//!
//! Faithful transcription of `hal/backend_linux.go`:
//!
//! * `getBatteryPath()` -> `BAT0`, `BAT1`, `BAT2`, `BATT`; else any `power_supply`
//!   whose `type` is exactly `Battery`; else the literal `BAT0` (never fails, cached
//!   for the lifetime of the object)
//! * `GetBatteryPercentage()` -> `capacity` (missing/unparsable -> 0)
//! * `IsCharging()` -> first scan every `power_supply` for `type` in
//!   {`Mains`, `USB_C`, `USB`} with `online == "1"`; otherwise `status` in
//!   {`Charging`, `Full`}
//! * `GetBatteryTime()` -> `Charging`, else the `energy_now`/`power_now` path, else the
//!   uevent `POWER_SUPPLY_ENERGY_NOW`/`POWER_SUPPLY_POWER_NOW` path, else the
//!   charge/current/voltage path with `energy = e*(v/1e6)` / `power = |c|*(v/1e6)`,
//!   formatted `"%dh %02dm"`, else `Calculating...`
//! * `GetPowerConsumptionWatts()` -> `power_now`/1e6, else `|current_now*voltage_now|`/1e12,
//!   else uevent, else 0.0

use crate::linux::sysfs::SysfsRoot;
use std::collections::HashMap;
use std::path::PathBuf;
use wattwarden_core::{PowerSource, Result};

const POWER_SUPPLY_BASE: &str = "sys/class/power_supply";
const DEFAULT_BATTERY: &str = "BAT0";

pub struct LinuxBattery {
    root: SysfsRoot,
    /// Battery directory relative to the sysfs root (e.g. `class/power_supply/BAT0`).
    battery_rel: String,
    discovered: bool,
}

impl LinuxBattery {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Self {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Self {
        let (battery_rel, discovered) = Self::discover(&root);
        Self {
            root,
            battery_rel,
            discovered,
        }
    }

    /// Compatibility hook: point the battery (and AC) at explicit paths.
    ///
    /// The battery path is used verbatim; AC detection still follows the Go scan over
    /// `/sys/class/power_supply` (the `ac_path` argument is therefore accepted for
    /// signature compatibility but unused, matching the Go reference).
    pub fn from_paths(battery_path: Option<PathBuf>, _ac_path: Option<PathBuf>) -> Self {
        let root = SysfsRoot::new("/");
        match battery_path {
            Some(p) => Self {
                root,
                battery_rel: p
                    .to_string_lossy()
                    .trim_start_matches('/')
                    .trim_end_matches('/')
                    .to_string(),
                discovered: true,
            },
            None => Self {
                root,
                battery_rel: format!("{POWER_SUPPLY_BASE}/{DEFAULT_BATTERY}"),
                discovered: false,
            },
        }
    }

    /// Go `getBatteryPath()`: standard names, then a `type == "Battery"` scan, then `BAT0`.
    fn discover(root: &SysfsRoot) -> (String, bool) {
        for name in ["BAT0", "BAT1", "BAT2", "BATT"] {
            let rel = format!("{POWER_SUPPLY_BASE}/{name}");
            if root.exists(&rel) {
                return (rel, true);
            }
        }

        for name in root.names(POWER_SUPPLY_BASE) {
            let rel = format!("{POWER_SUPPLY_BASE}/{name}");
            if root.read(&format!("{rel}/type")) == "Battery" {
                return (rel, true);
            }
        }

        (format!("{POWER_SUPPLY_BASE}/{DEFAULT_BATTERY}"), false)
    }

    fn bat(&self, leaf: &str) -> String {
        format!("{}/{leaf}", self.battery_rel)
    }

    /// Go `parseUevent()`: `KEY=VALUE` lines of `<battery>/uevent`.
    fn parse_uevent(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let content = self.root.read(&self.bat("uevent"));
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        map
    }

    fn uevent_get(map: &HashMap<String, String>, key: &str) -> String {
        map.get(key).cloned().unwrap_or_default()
    }
}

impl Default for LinuxBattery {
    fn default() -> Self {
        Self::new()
    }
}

/// Go `fmt.Sprintf("%dh %02dm", h, m)` with `int()` truncation.
fn format_hours(hours: f64) -> String {
    let h = hours as i64;
    let m = ((hours - h as f64) * 60.0) as i64;
    format!("{h}h {m:02}m")
}

impl LinuxBattery {
    /// True when a battery directory was actually found.
    pub fn has_battery(&self) -> bool {
        self.discovered
    }
}

impl PowerSource for LinuxBattery {
    fn battery_percentage(&self) -> Result<u8> {
        let v = self.root.read_i64(&self.bat("capacity")).unwrap_or(0);
        Ok(v.clamp(0, 100) as u8)
    }

    fn is_charging(&self) -> Result<bool> {
        // 1. Any Mains / USB-C / USB supply with `online == "1"`.
        for name in self.root.names(POWER_SUPPLY_BASE) {
            let rel = format!("{POWER_SUPPLY_BASE}/{name}");
            let supply_type = self.root.read(&format!("{rel}/type"));
            if (supply_type == "Mains" || supply_type == "USB_C" || supply_type == "USB")
                && self.root.read(&format!("{rel}/online")) == "1"
            {
                return Ok(true);
            }
        }

        // 2. Battery status.
        let status = self.root.read(&self.bat("status"));
        Ok(status == "Charging" || status == "Full")
    }

    fn consumption_watts(&self) -> Result<f64> {
        // 1. power_now in microwatts.
        let power_now = self.root.read(&self.bat("power_now"));
        if !power_now.is_empty() {
            if let Ok(p) = power_now.parse::<f64>() {
                return Ok(p.abs() / 1_000_000.0);
            }
        }

        // 2. current_now (uA) * voltage_now (uV).
        let current_now = self.root.read(&self.bat("current_now"));
        let voltage_now = self.root.read(&self.bat("voltage_now"));
        if !current_now.is_empty() && !voltage_now.is_empty() {
            let c = current_now.parse::<f64>().unwrap_or(0.0);
            let v = voltage_now.parse::<f64>().unwrap_or(0.0);
            return Ok((c * v).abs() / 1_000_000_000_000.0);
        }

        // 3. uevent fallback.
        let uevent = self.parse_uevent();
        let p_str = Self::uevent_get(&uevent, "POWER_SUPPLY_POWER_NOW");
        if !p_str.is_empty() {
            if let Ok(p) = p_str.parse::<f64>() {
                return Ok(p.abs() / 1_000_000.0);
            }
        }
        let c_str = Self::uevent_get(&uevent, "POWER_SUPPLY_CURRENT_NOW");
        let v_str = Self::uevent_get(&uevent, "POWER_SUPPLY_VOLTAGE_NOW");
        if !c_str.is_empty() && !v_str.is_empty() {
            let c = c_str.parse::<f64>().unwrap_or(0.0);
            let v = v_str.parse::<f64>().unwrap_or(0.0);
            return Ok((c * v).abs() / 1_000_000_000_000.0);
        }

        Ok(0.0)
    }

    fn time_remaining(&self) -> Result<String> {
        if self.is_charging()? {
            return Ok("Charging".to_string());
        }

        let mut energy_str = self.root.read(&self.bat("energy_now"));
        let mut power_str = self.root.read(&self.bat("power_now"));

        if energy_str.is_empty() || power_str.is_empty() {
            let uevent = self.parse_uevent();
            energy_str = Self::uevent_get(&uevent, "POWER_SUPPLY_ENERGY_NOW");
            power_str = Self::uevent_get(&uevent, "POWER_SUPPLY_POWER_NOW");

            if energy_str.is_empty() || power_str.is_empty() {
                energy_str = Self::uevent_get(&uevent, "POWER_SUPPLY_CHARGE_NOW");
                power_str = Self::uevent_get(&uevent, "POWER_SUPPLY_CURRENT_NOW");
                let mut voltage_str = Self::uevent_get(&uevent, "POWER_SUPPLY_VOLTAGE_NOW");

                if energy_str.is_empty() || power_str.is_empty() || voltage_str.is_empty() {
                    energy_str = self.root.read(&self.bat("charge_now"));
                    power_str = self.root.read(&self.bat("current_now"));
                    voltage_str = self.root.read(&self.bat("voltage_now"));
                }

                if !energy_str.is_empty() && !power_str.is_empty() && !voltage_str.is_empty() {
                    let e = energy_str.parse::<f64>().unwrap_or(0.0);
                    let c = power_str.parse::<f64>().unwrap_or(0.0);
                    let v = voltage_str.parse::<f64>().unwrap_or(0.0);

                    let energy = e * (v / 1_000_000.0);
                    let power = c.abs() * (v / 1_000_000.0);

                    if power > 0.0 {
                        return Ok(format_hours(energy / power));
                    }
                }
                return Ok("Calculating...".to_string());
            }
        }

        let energy = energy_str.parse::<f64>().unwrap_or(0.0);
        let power = power_str.parse::<f64>().unwrap_or(0.0).abs();

        if power > 0.0 {
            return Ok(format_hours(energy / power));
        }

        Ok("Calculating...".to_string())
    }

    fn is_stationary(&self) -> bool {
        !self.discovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(tag: &str) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_bat_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sys/class/power_supply/BAT0")).unwrap();
        SysfsRoot::new(dir)
    }

    #[test]
    fn percentage_and_charging_follow_go_rules() {
        let root = root("basic");
        fs::write(root.path("sys/class/power_supply/BAT0/capacity"), "75\n").unwrap();
        fs::write(root.path("sys/class/power_supply/BAT0/status"), "Discharging\n").unwrap();

        let bat = LinuxBattery::with_root(root.clone());
        assert!(bat.has_battery());
        assert!(!bat.is_stationary());
        assert_eq!(bat.battery_percentage().unwrap(), 75);
        assert!(!bat.is_charging().unwrap());

        // Mains supply takes priority and is read with `online == "1"`.
        fs::create_dir_all(root.path("sys/class/power_supply/AC")).unwrap();
        fs::write(root.path("sys/class/power_supply/AC/type"), "Mains\n").unwrap();
        fs::write(root.path("sys/class/power_supply/AC/online"), "1\n").unwrap();
        assert!(bat.is_charging().unwrap());

        // Full / Charging statuses also count.
        fs::write(root.path("sys/class/power_supply/AC/online"), "0\n").unwrap();
        fs::write(root.path("sys/class/power_supply/BAT0/status"), "Full\n").unwrap();
        assert!(bat.is_charging().unwrap());
    }

    #[test]
    fn percentage_falls_back_to_zero_without_battery() {
        let root = root("nobat");
        let bat = LinuxBattery::with_root(SysfsRoot::new(root.path("nowhere")));
        assert!(!bat.has_battery());
        assert!(bat.is_stationary());
        assert_eq!(bat.battery_percentage().unwrap(), 0);
        assert!(!bat.is_charging().unwrap());
    }

    #[test]
    fn time_remaining_uses_charge_now_formula() {
        let root = root("chargeformula");
        // charge_now (uAh) * voltage_now (uV) / 1e6 -> energy, current_now (uA) * voltage (uV) / 1e6
        fs::write(root.path("sys/class/power_supply/BAT0/status"), "Discharging\n").unwrap();
        fs::write(root.path("sys/class/power_supply/BAT0/charge_now"), "4000000").unwrap();
        fs::write(root.path("sys/class/power_supply/BAT0/current_now"), "-1000000").unwrap();
        fs::write(root.path("sys/class/power_supply/BAT0/voltage_now"), "10000000").unwrap();

        let bat = LinuxBattery::with_root(root);
        // energy = 4e6 * (1e7/1e6) = 4e7 ; power = 1e6 * (1e7/1e6) = 1e7 -> 4h
        assert_eq!(bat.time_remaining().unwrap(), "4h 00m");
    }

    #[test]
    fn time_remaining_reports_charging_and_calculating() {
        let root = root("calc");
        fs::write(root.path("sys/class/power_supply/BAT0/status"), "Discharging\n").unwrap();
        // No energy/power information anywhere.
        let bat = LinuxBattery::with_root(root.clone());
        assert_eq!(bat.time_remaining().unwrap(), "Calculating...");

        fs::write(root.path("sys/class/power_supply/BAT0/status"), "Charging\n").unwrap();
        assert_eq!(bat.time_remaining().unwrap(), "Charging");
    }

    #[test]
    fn uevent_is_the_second_path_for_time_remaining() {
        let root = root("uevent");
        fs::write(root.path("sys/class/power_supply/BAT0/status"), "Discharging\n").unwrap();
        fs::write(
            root.path("sys/class/power_supply/BAT0/uevent"),
            "POWER_SUPPLY_NAME=BAT0\nPOWER_SUPPLY_ENERGY_NOW=30000000\nPOWER_SUPPLY_POWER_NOW=10000000\n",
        )
        .unwrap();

        let bat = LinuxBattery::with_root(root);
        assert_eq!(bat.time_remaining().unwrap(), "3h 00m");
        assert!((bat.consumption_watts().unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn consumption_watts_paths() {
        let root = root("watts");
        let bat = LinuxBattery::with_root(root.clone());
        // 1. power_now
        fs::write(root.path("sys/class/power_supply/BAT0/power_now"), "15000000").unwrap();
        assert!((bat.consumption_watts().unwrap() - 15.0).abs() < 1e-9);
        // 2. current * voltage
        fs::remove_file(root.path("sys/class/power_supply/BAT0/power_now")).unwrap();
        fs::write(root.path("sys/class/power_supply/BAT0/current_now"), "2000000").unwrap();
        fs::write(root.path("sys/class/power_supply/BAT0/voltage_now"), "10000000").unwrap();
        assert!((bat.consumption_watts().unwrap() - 20.0).abs() < 1e-9);
        // 3. nothing
        fs::remove_file(root.path("sys/class/power_supply/BAT0/current_now")).unwrap();
        assert_eq!(bat.consumption_watts().unwrap(), 0.0);
    }

    #[test]
    fn discovery_scan_finds_non_bat0_names() {
        let dir = std::env::temp_dir().join(format!("ww_bat_scan_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sys/class/power_supply/C1")).unwrap();
        fs::write(dir.join("sys/class/power_supply/C1/type"), "Battery\n").unwrap();
        fs::write(dir.join("sys/class/power_supply/C1/capacity"), "42\n").unwrap();

        let bat = LinuxBattery::with_root(SysfsRoot::new(&dir));
        assert!(bat.has_battery());
        assert_eq!(bat.battery_percentage().unwrap(), 42);
        let _ = fs::remove_dir_all(dir);
    }
}
