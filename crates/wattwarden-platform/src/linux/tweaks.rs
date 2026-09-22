//! Kernel / driver energy tweaks.
//!
//! Faithful transcription of `hal/backend_linux.go`:
//!
//! * WiFi power save -> `iw dev` to list interfaces, `iw dev X get power_save`
//!   (contains `on`), then the `/sys/module/iwlwifi/parameters/power_save` driver
//!   parameter (`Y`/`N`) as fallback. `Set` writes the driver parameter first and
//!   then `iw dev X set power_save on|off` for every interface.
//! * Audio power save -> `/sys/module/snd_hda_intel/parameters/power_save`
//!   (`"0"` means off) plus `power_save_controller` (`Y`/`N`). Note Go's getter is
//!   `readSys(..) != "0"`, so a *missing* module parameter reads as `true`.
//! * Autosuspend -> globs `/sys/bus/usb/devices/*/power/control` (get) and both USB
//!   and PCI device globs (set): some device in `auto` means enabled; `set(true)`
//!   writes `"auto"`, `set(false)` writes `"on"`.
//! * NMI watchdog -> `/proc/sys/kernel/nmi_watchdog` (`"1"` = on).
//! * VM writeback -> `/proc/sys/vm/dirty_writeback_centisecs`, clamped `100..=6000`
//!   centiseconds.
//! * `ProcessPurge` -> write `"3"` to `/proc/sys/vm/drop_caches`.
//!
//! Every write is best effort (Go's `writeSys`), so running unprivileged or on a
//! machine without those knobs degrades silently instead of aborting.

use crate::linux::cmd;
use crate::linux::sysfs::SysfsRoot;
use tracing::debug;
use wattwarden_core::{Result, SystemTweaksController};

const IWLWIFI_POWER_SAVE: &str = "sys/module/iwlwifi/parameters/power_save";
const SND_HDA_POWER_SAVE: &str = "sys/module/snd_hda_intel/parameters/power_save";
const SND_HDA_POWER_SAVE_CONTROLLER: &str =
    "sys/module/snd_hda_intel/parameters/power_save_controller";
const USB_DEVICES: &str = "sys/bus/usb/devices";
const PCI_DEVICES: &str = "sys/bus/pci/devices";
const NMI_WATCHDOG: &str = "proc/sys/kernel/nmi_watchdog";
const DIRTY_WRITEBACK: &str = "proc/sys/vm/dirty_writeback_centisecs";
const DROP_CACHES: &str = "proc/sys/vm/drop_caches";

/// Go `SetVMWriteback` bounds (centiseconds).
pub const VM_WRITEBACK_MIN_CENTISECS: u32 = 100;
pub const VM_WRITEBACK_MAX_CENTISECS: u32 = 6000;

#[derive(Debug, Clone)]
pub struct LinuxSystemTweaks {
    root: SysfsRoot,
}

impl LinuxSystemTweaks {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Self {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Self {
        Self { root }
    }

    /// Go `iw dev | awk '$1=="Interface"{print $2}'` without the shell pipeline.
    fn wifi_interfaces() -> Vec<String> {
        cmd::run_capture("iw", &["dev"])
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                match (fields.next(), fields.next()) {
                    (Some("Interface"), Some(name)) => Some(name.to_string()),
                    _ => None,
                }
            })
            .collect()
    }

    /// Best-effort write that only touches a node the kernel actually exposes.
    ///
    /// Several of these knobs are module parameters or proc entries that simply do
    /// not exist on every machine (a desktop without `iwlwifi`, a container without
    /// `/proc/sys/kernel`). Writing to a nonexistent node is a no-op at best; skip it
    /// explicitly so the discovery is what decides, not a failed `fs::write`.
    fn write_node(&self, rel: &str, val: &str) {
        if self.root.exists(rel) {
            self.root.write_best_effort(rel, val);
        } else {
            debug!("tweaks: {rel} not exposed by the kernel; not writing");
        }
    }

    /// `power/control` nodes of every USB and PCI device, in glob order.
    fn control_nodes(&self) -> Vec<String> {
        let mut nodes: Vec<String> = Vec::new();
        for base in [USB_DEVICES, PCI_DEVICES] {
            for device in self.root.names(base) {
                nodes.push(format!("{base}/{device}/power/control"));
            }
        }
        nodes
    }
}

impl Default for LinuxSystemTweaks {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemTweaksController for LinuxSystemTweaks {
    fn wifi_power_save(&self) -> Result<bool> {
        for iface in Self::wifi_interfaces() {
            if cmd::run_capture("iw", &["dev", &iface, "get", "power_save"]).contains("on") {
                return Ok(true);
            }
        }
        // Fallback: Intel driver parameter, literal "Y" like Go.
        Ok(self.root.read(IWLWIFI_POWER_SAVE) == "Y")
    }

    fn set_wifi_power_save(&self, enabled: bool) -> Result<()> {
        self.write_node(IWLWIFI_POWER_SAVE, if enabled { "Y" } else { "N" });

        let state = if enabled { "on" } else { "off" };
        for iface in Self::wifi_interfaces() {
            cmd::run_ignored("iw", &["dev", &iface, "set", "power_save", state]);
        }
        Ok(())
    }

    fn audio_power_save(&self) -> Result<bool> {
        // Go: `readSys(..) != "0"` — a missing file therefore reads as enabled.
        Ok(self.root.read(SND_HDA_POWER_SAVE) != "0")
    }

    fn set_audio_power_save(&self, enabled: bool) -> Result<()> {
        self.write_node(SND_HDA_POWER_SAVE, if enabled { "1" } else { "0" });
        self.write_node(
            SND_HDA_POWER_SAVE_CONTROLLER,
            if enabled { "Y" } else { "N" },
        );
        Ok(())
    }

    fn autosuspend(&self) -> Result<bool> {
        for device in self.root.names(USB_DEVICES) {
            if self
                .root
                .read(&format!("{USB_DEVICES}/{device}/power/control"))
                == "auto"
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn set_autosuspend(&self, enabled: bool) -> Result<()> {
        let value = if enabled { "auto" } else { "on" };
        for node in self.control_nodes() {
            self.write_node(&node, value);
        }
        Ok(())
    }

    fn nmi_watchdog(&self) -> Result<bool> {
        Ok(self.root.read(NMI_WATCHDOG) == "1")
    }

    fn set_nmi_watchdog(&self, enabled: bool) -> Result<()> {
        self.write_node(NMI_WATCHDOG, if enabled { "1" } else { "0" });
        Ok(())
    }

    fn vm_writeback_seconds(&self) -> Result<u32> {
        // Go `GetVMWriteback` returns centiseconds (0 when unparsable); the Rust API
        // exposes whole seconds, hence the integer division.
        let centisecs = self.root.read_i64(DIRTY_WRITEBACK).unwrap_or(0);
        Ok((centisecs.max(0) / 100) as u32)
    }

    fn set_vm_writeback_seconds(&self, seconds: u32) -> Result<()> {
        let centisecs = seconds
            .saturating_mul(100)
            .clamp(VM_WRITEBACK_MIN_CENTISECS, VM_WRITEBACK_MAX_CENTISECS);
        self.write_node(DIRTY_WRITEBACK, &centisecs.to_string());
        Ok(())
    }

    fn process_purge(&self) -> Result<()> {
        self.write_node(DROP_CACHES, "3");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(tag: &str) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_tweaks_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        SysfsRoot::new(dir)
    }

    fn write(root: &SysfsRoot, rel: &str, value: &str) {
        let path = root.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }

    #[test]
    fn audio_power_save_matches_go_including_missing_file() {
        let root = root("audio");
        let tweaks = LinuxSystemTweaks::with_root(root.clone());

        // Missing /sys/module/snd_hda_intel -> "" != "0" -> true (Go behaviour).
        assert!(tweaks.audio_power_save().unwrap());

        write(&root, SND_HDA_POWER_SAVE, "0\n");
        assert!(!tweaks.audio_power_save().unwrap());
        write(&root, SND_HDA_POWER_SAVE_CONTROLLER, "N\n");

        tweaks.set_audio_power_save(true).unwrap();
        assert_eq!(root.read(SND_HDA_POWER_SAVE), "1");
        assert_eq!(root.read(SND_HDA_POWER_SAVE_CONTROLLER), "Y");
        assert!(tweaks.audio_power_save().unwrap());

        tweaks.set_audio_power_save(false).unwrap();
        assert_eq!(root.read(SND_HDA_POWER_SAVE), "0");
        assert_eq!(root.read(SND_HDA_POWER_SAVE_CONTROLLER), "N");
    }

    #[test]
    fn wifi_power_save_driver_fallback_uses_y_and_n() {
        let root = root("wifi");
        let tweaks = LinuxSystemTweaks::with_root(root.clone());
        write(&root, IWLWIFI_POWER_SAVE, "N\n");

        tweaks.set_wifi_power_save(true).unwrap();
        assert_eq!(root.read(IWLWIFI_POWER_SAVE), "Y");
        tweaks.set_wifi_power_save(false).unwrap();
        assert_eq!(root.read(IWLWIFI_POWER_SAVE), "N");
    }

    #[test]
    fn autosuspend_get_and_set_over_usb_and_pci() {
        let root = root("auto");
        let tweaks = LinuxSystemTweaks::with_root(root.clone());
        write(&root, "sys/bus/usb/devices/usb1/power/control", "on\n");
        write(&root, "sys/bus/usb/devices/1-1/power/control", "on\n");
        write(
            &root,
            "sys/bus/pci/devices/0000:00:14.0/power/control",
            "on\n",
        );

        assert!(!tweaks.autosuspend().unwrap());

        tweaks.set_autosuspend(true).unwrap();
        assert_eq!(root.read("sys/bus/usb/devices/usb1/power/control"), "auto");
        assert_eq!(root.read("sys/bus/usb/devices/1-1/power/control"), "auto");
        assert_eq!(
            root.read("sys/bus/pci/devices/0000:00:14.0/power/control"),
            "auto"
        );
        assert!(tweaks.autosuspend().unwrap());

        tweaks.set_autosuspend(false).unwrap();
        assert_eq!(root.read("sys/bus/usb/devices/usb1/power/control"), "on");
        assert_eq!(
            root.read("sys/bus/pci/devices/0000:00:14.0/power/control"),
            "on"
        );
        assert!(!tweaks.autosuspend().unwrap());
    }

    #[test]
    fn nmi_watchdog_and_vm_writeback() {
        let root = root("kernel");
        let tweaks = LinuxSystemTweaks::with_root(root.clone());

        // Missing files -> Go's "1" comparison fails and Atoi yields 0.
        assert!(!tweaks.nmi_watchdog().unwrap());
        assert_eq!(tweaks.vm_writeback_seconds().unwrap(), 0);

        write(&root, NMI_WATCHDOG, "1\n");
        write(&root, DIRTY_WRITEBACK, "500\n");
        assert!(tweaks.nmi_watchdog().unwrap());
        assert_eq!(tweaks.vm_writeback_seconds().unwrap(), 5);

        tweaks.set_nmi_watchdog(false).unwrap();
        assert_eq!(root.read(NMI_WATCHDOG), "0");
        assert!(!tweaks.nmi_watchdog().unwrap());

        // Clamp is applied on the centisecond value Go writes.
        tweaks.set_vm_writeback_seconds(6).unwrap();
        assert_eq!(root.read(DIRTY_WRITEBACK), "600");
        tweaks.set_vm_writeback_seconds(0).unwrap();
        assert_eq!(root.read(DIRTY_WRITEBACK), "100");
        tweaks.set_vm_writeback_seconds(1000).unwrap();
        assert_eq!(root.read(DIRTY_WRITEBACK), "6000");
    }

    #[test]
    fn process_purge_writes_three() {
        let root = root("purge");
        let tweaks = LinuxSystemTweaks::with_root(root.clone());
        write(&root, DROP_CACHES, "0\n");
        tweaks.process_purge().unwrap();
        assert_eq!(root.read(DROP_CACHES), "3");

        // Must not panic on a root without /proc (unprivileged / container).
        let missing = LinuxSystemTweaks::with_root(SysfsRoot::new("/definitely/not/a/root"));
        missing.process_purge().unwrap();
        missing.set_nmi_watchdog(true).unwrap();
        missing.set_autosuspend(true).unwrap();
        missing.set_audio_power_save(true).unwrap();
        missing.set_vm_writeback_seconds(5).unwrap();
        assert!(!missing.autosuspend().unwrap());
    }
}
