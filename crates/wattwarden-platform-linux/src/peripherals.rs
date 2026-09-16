use std::fs;
use std::path::PathBuf;
use std::process::Command;
use wattwarden_core::{PeripheralsController, Result};
use crate::sysfs::{read_sysfs_string, read_sysfs_u32, write_sysfs_string};

#[derive(Debug, Clone, Default)]
pub struct LinuxPeripherals;

impl LinuxPeripherals {
    pub fn new() -> Self {
        Self
    }

    fn find_kbd_leds() -> Vec<PathBuf> {
        let mut leds = Vec::new();
        if let Ok(entries) = fs::read_dir("/sys/class/leds") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with("kbd_backlight") {
                    leds.push(entry.path());
                }
            }
        }
        leds
    }

    fn get_rfkill_state(target_type: &str) -> Option<bool> {
        if let Ok(entries) = fs::read_dir("/sys/class/rfkill") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(t) = read_sysfs_string(path.join("type")) {
                    if t.trim() == target_type {
                        if let Ok(soft) = read_sysfs_string(path.join("soft")) {
                            return Some(soft.trim() == "0"); // 0 = unblocked (on), 1 = blocked (off)
                        }
                    }
                }
            }
        }
        // Fallback to rfkill command line
        if let Ok(output) = Command::new("rfkill").args(["list", target_type]).output() {
            let out_str = String::from_utf8_lossy(&output.stdout);
            if !out_str.is_empty() {
                return Some(!out_str.contains("Soft blocked: yes"));
            }
        }
        None
    }

    fn set_rfkill_state(target_type: &str, enabled: bool) -> Result<()> {
        let mut wrote = false;
        let val = if enabled { "0" } else { "1" };

        if let Ok(entries) = fs::read_dir("/sys/class/rfkill") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(t) = read_sysfs_string(path.join("type")) {
                    if t.trim() == target_type {
                        if write_sysfs_string(path.join("soft"), val).is_ok() {
                            wrote = true;
                        }
                    }
                }
            }
        }

        if !wrote {
            let action = if enabled { "unblock" } else { "block" };
            let _ = Command::new("rfkill").args([action, target_type]).status();
        }

        Ok(())
    }
}

impl PeripheralsController for LinuxPeripherals {
    fn kbd_backlight(&self) -> Result<bool> {
        let leds = Self::find_kbd_leds();
        for p in leds {
            if let Ok(v) = read_sysfs_u32(p.join("brightness")) {
                return Ok(v > 0);
            }
        }
        Ok(false)
    }

    fn set_kbd_backlight(&self, enabled: bool) -> Result<()> {
        let leds = Self::find_kbd_leds();
        for p in leds {
            if enabled {
                let max = read_sysfs_string(p.join("max_brightness")).unwrap_or_else(|_| "1".into());
                let _ = write_sysfs_string(p.join("brightness"), &max);
            } else {
                let _ = write_sysfs_string(p.join("brightness"), "0");
            }
        }
        Ok(())
    }

    fn bluetooth_enabled(&self) -> Result<bool> {
        Ok(Self::get_rfkill_state("bluetooth").unwrap_or(true))
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()> {
        Self::set_rfkill_state("bluetooth", enabled)
    }

    fn wifi_enabled(&self) -> Result<bool> {
        Ok(Self::get_rfkill_state("wlan").unwrap_or(true))
    }

    fn set_wifi_enabled(&self, enabled: bool) -> Result<()> {
        Self::set_rfkill_state("wlan", enabled)
    }
}
