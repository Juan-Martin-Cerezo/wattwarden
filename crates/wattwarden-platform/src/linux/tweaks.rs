use crate::sysfs::{read_sysfs_string, read_sysfs_u32, write_sysfs_string};
use std::fs;
use std::process::Command;
use wattwarden_core::{Result, SystemTweaksController};

#[derive(Debug, Clone, Default)]
pub struct LinuxSystemTweaks;

impl LinuxSystemTweaks {
    pub fn new() -> Self {
        Self
    }

    fn find_wifi_interfaces() -> Vec<String> {
        let mut ifaces = Vec::new();
        if let Ok(output) = Command::new("iw").arg("dev").output() {
            let out_str = String::from_utf8_lossy(&output.stdout);
            for line in out_str.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[0] == "Interface" {
                    ifaces.push(parts[1].to_string());
                }
            }
        }
        ifaces
    }
}

impl SystemTweaksController for LinuxSystemTweaks {
    fn wifi_power_save(&self) -> Result<bool> {
        // First check iw dev
        for iface in Self::find_wifi_interfaces() {
            if let Ok(output) = Command::new("iw")
                .args(["dev", &iface, "get", "power_save"])
                .output()
            {
                let out = String::from_utf8_lossy(&output.stdout);
                if out.contains("Power save: on") {
                    return Ok(true);
                }
            }
        }
        // Fallback to driver parameter
        if let Ok(val) = read_sysfs_string("/sys/module/iwlwifi/parameters/power_save") {
            return Ok(val.trim() == "Y" || val.trim() == "1");
        }
        Ok(false)
    }

    fn set_wifi_power_save(&self, enabled: bool) -> Result<()> {
        let iw_state = if enabled { "on" } else { "off" };
        for iface in Self::find_wifi_interfaces() {
            let _ = Command::new("iw")
                .args(["dev", &iface, "set", "power_save", iw_state])
                .status();
        }
        let driver_val = if enabled { "Y" } else { "N" };
        let _ = write_sysfs_string("/sys/module/iwlwifi/parameters/power_save", driver_val);
        Ok(())
    }

    fn audio_power_save(&self) -> Result<bool> {
        if let Ok(val) = read_sysfs_string("/sys/module/snd_hda_intel/parameters/power_save") {
            return Ok(val.trim() != "0");
        }
        Ok(false)
    }

    fn set_audio_power_save(&self, enabled: bool) -> Result<()> {
        let ps_val = if enabled { "1" } else { "0" };
        let ctrl_val = if enabled { "Y" } else { "N" };
        let _ = write_sysfs_string("/sys/module/snd_hda_intel/parameters/power_save", ps_val);
        let _ = write_sysfs_string(
            "/sys/module/snd_hda_intel/parameters/power_save_controller",
            ctrl_val,
        );
        Ok(())
    }

    fn autosuspend(&self) -> Result<bool> {
        if let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") {
            for entry in entries.flatten() {
                let p = entry.path().join("power/control");
                if let Ok(ctrl) = read_sysfs_string(p) {
                    if ctrl.trim() == "auto" {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn set_autosuspend(&self, enabled: bool) -> Result<()> {
        let val = if enabled { "auto" } else { "on" };

        if let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") {
            for entry in entries.flatten() {
                let _ = write_sysfs_string(entry.path().join("power/control"), val);
            }
        }
        if let Ok(entries) = fs::read_dir("/sys/bus/pci/devices") {
            for entry in entries.flatten() {
                let _ = write_sysfs_string(entry.path().join("power/control"), val);
            }
        }
        Ok(())
    }

    fn nmi_watchdog(&self) -> Result<bool> {
        if let Ok(val) = read_sysfs_string("/proc/sys/kernel/nmi_watchdog") {
            return Ok(val.trim() == "1");
        }
        Ok(true)
    }

    fn set_nmi_watchdog(&self, enabled: bool) -> Result<()> {
        let val = if enabled { "1" } else { "0" };
        write_sysfs_string("/proc/sys/kernel/nmi_watchdog", val)
    }

    fn vm_writeback_seconds(&self) -> Result<u32> {
        let cs = read_sysfs_u32("/proc/sys/vm/dirty_writeback_centisecs")?;
        Ok(cs / 100)
    }

    fn set_vm_writeback_seconds(&self, seconds: u32) -> Result<()> {
        let cs = (seconds * 100).clamp(100, 6000);
        write_sysfs_string("/proc/sys/vm/dirty_writeback_centisecs", &cs.to_string())
    }

    fn process_purge(&self) -> Result<()> {
        write_sysfs_string("/proc/sys/vm/drop_caches", "3")
    }
}
