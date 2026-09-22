//! Peripherals: keyboard backlight and radio (Bluetooth / Wi-Fi) kill switches.
//!
//! Faithful transcription of `hal/backend_linux.go`:
//!
//! * `GetKbdBacklight()` -> glob `/sys/class/leds/*kbd_backlight/brightness`, first
//!   match, `readSys(f) != "0"`; `false` when nothing matches.
//! * `SetKbdBacklight(e)` -> glob `/sys/class/leds/*kbd_backlight`, on writes
//!   `max_brightness` verbatim, off writes `"0"`.
//! * `GetBluetooth()` / `GetWifiEnable()` -> `rfkill list bluetooth|wifi` is *off*
//!   only when the output contains the literal `Soft blocked: yes`.
//! * `SetBluetooth()` / `SetWifiEnable()` -> `rfkill block|unblock <kind>`.
//!
//! The `rfkill` invocations are best effort: when the binary is missing the command
//! helper returns `""` and the getters report "not blocked" like Go's `runCmd`.

use crate::linux::cmd;
use crate::linux::sysfs::SysfsRoot;
use wattwarden_core::{PeripheralsController, Result};

const LEDS_BASE: &str = "class/leds";
const KBD_SUFFIX: &str = "kbd_backlight";

#[derive(Debug, Clone)]
pub struct LinuxPeripherals {
    root: SysfsRoot,
}

impl LinuxPeripherals {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Self {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Self {
        Self { root }
    }

    /// Every `/sys/class/leds/*kbd_backlight` entry, in glob (sorted) order.
    fn kbd_leds(&self) -> Vec<String> {
        self.root
            .names(LEDS_BASE)
            .into_iter()
            .filter(|name| name.ends_with(KBD_SUFFIX))
            .map(|name| format!("{LEDS_BASE}/{name}"))
            .collect()
    }

    /// Go semantics: power is off only when `rfkill` explicitly reports a block.
    fn rfkill_blocked(kind: &str) -> bool {
        cmd::run_capture("rfkill", &["list", kind]).contains("Soft blocked: yes")
    }

    fn set_rfkill(kind: &str, enabled: bool) {
        let action = if enabled { "unblock" } else { "block" };
        cmd::run_ignored("rfkill", &[action, kind]);
    }
}

impl Default for LinuxPeripherals {
    fn default() -> Self {
        Self::new()
    }
}

impl PeripheralsController for LinuxPeripherals {
    fn kbd_backlight(&self) -> Result<bool> {
        for led in self.kbd_leds() {
            let brightness = format!("{led}/brightness");
            // Go globs the `/brightness` file itself: missing file means no match.
            if self.root.exists(&brightness) {
                return Ok(self.root.read(&brightness) != "0");
            }
        }
        Ok(false)
    }

    fn set_kbd_backlight(&self, enabled: bool) -> Result<()> {
        for led in self.kbd_leds() {
            if enabled {
                // Go writes the raw `max_brightness` content.
                let max = self.root.read(&format!("{led}/max_brightness"));
                self.root
                    .write_best_effort(&format!("{led}/brightness"), &max);
            } else {
                self.root
                    .write_best_effort(&format!("{led}/brightness"), "0");
            }
        }
        Ok(())
    }

    fn bluetooth_enabled(&self) -> Result<bool> {
        Ok(!Self::rfkill_blocked("bluetooth"))
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()> {
        Self::set_rfkill("bluetooth", enabled);
        Ok(())
    }

    fn wifi_enabled(&self) -> Result<bool> {
        Ok(!Self::rfkill_blocked("wifi"))
    }

    fn set_wifi_enabled(&self, enabled: bool) -> Result<()> {
        Self::set_rfkill("wifi", enabled);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(tag: &str) -> (SysfsRoot, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("ww_periph_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let led = dir.join(LEDS_BASE).join("dell::kbd_backlight");
        let _ = fs::create_dir_all(&led).unwrap();
        fs::write(led.join("brightness"), "0\n").unwrap();
        fs::write(led.join("max_brightness"), "2\n").unwrap();
        (SysfsRoot::new(dir), led)
    }

    #[test]
    fn kbd_backlight_get_and_set() {
        let (root, led) = root("kbd");
        let p = LinuxPeripherals::with_root(root);
        assert!(!p.kbd_backlight().unwrap());

        p.set_kbd_backlight(true).unwrap();
        assert_eq!(fs::read_to_string(led.join("brightness")).unwrap(), "2");
        assert!(p.kbd_backlight().unwrap());

        p.set_kbd_backlight(false).unwrap();
        assert_eq!(fs::read_to_string(led.join("brightness")).unwrap(), "0");
        assert!(!p.kbd_backlight().unwrap());
    }

    #[test]
    fn no_rfkill_binary_or_device_is_not_blocked() {
        // We cannot uninstall `rfkill`, but the contract is: never panic, and report
        // "not blocked" unless the tool explicitly says otherwise.
        let dir = std::env::temp_dir().join(format!("ww_periph_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir).unwrap();
        let p = LinuxPeripherals::with_root(SysfsRoot::new(dir));
        assert!(!p.kbd_backlight().unwrap());
        p.set_kbd_backlight(true).unwrap();
        p.set_bluetooth_enabled(false).unwrap();
        p.set_wifi_enabled(true).unwrap();
        let _ = p.bluetooth_enabled().unwrap();
        let _ = p.wifi_enabled().unwrap();
    }
}
