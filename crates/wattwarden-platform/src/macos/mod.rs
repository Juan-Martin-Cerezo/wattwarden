//! macOS hardware abstraction layer.
//!
//! This module is compiled on **every** host, not just `target_os = "macos"`: the
//! backend only ever shells out to `pmset`/`ioreg`/`networksetup`/… (no macOS-only
//! API), so its integration tests can execute against mocked command runners on Linux hosts too.
//! `lib.rs` re-exports it as `PlatformBackend` on macOS.
//!
//! No physical hardware is required to initialise: missing commands and reads fall back
//! gracefully to safe defaults.

use crate::exec;
use crate::parse;
use wattwarden_core::*;

pub struct MacOsBattery;

impl PowerSource for MacOsBattery {
    /// Reads battery percentage via `pmset -g batt`.
    fn battery_percentage(&self) -> Result<u8> {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        Ok(parse::mac_battery_percent(&out))
    }

    /// Checks charging status via `pmset -g batt`.
    fn is_charging(&self) -> Result<bool> {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        Ok(parse::mac_is_charging(&out))
    }

    /// Measures current power consumption in Watts via `ioreg`.
    fn consumption_watts(&self) -> Result<f64> {
        let out = exec::run_capture("ioreg", &["-rn", "AppleSmartBattery"]);
        Ok(parse::mac_power_watts(&out))
    }

    /// Formats remaining battery time via `pmset -g batt`.
    fn time_remaining(&self) -> Result<String> {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        let charging = parse::mac_is_charging(&out);
        Ok(parse::mac_time_remaining(&out, charging))
    }

    /// Detects whether device is running without an internal battery.
    fn is_stationary(&self) -> bool {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        !out.contains("InternalBattery")
    }
}

pub struct MacOsCpu;

impl CpuGovernor for MacOsCpu {
    /// Number of logical CPUs available.
    fn num_cpus(&self) -> usize {
        std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
    }

    /// Number of active online CPU cores.
    fn online_cores(&self) -> Result<usize> {
        Ok(self.num_cpus())
    }

    /// Setting online cores is a no-op on macOS.
    fn set_online_cores(&self, _count: usize) -> Result<()> {
        Ok(())
    }

    /// macOS exposes no CPU frequency scaling interface, so there is no range to
    /// discover. `Unsupported` (not a fabricated `Ok`) is what keeps the shared
    /// ladder and the TUI from ever writing a frequency here.
    fn freq_bounds(&self) -> Result<(u32, u32)> {
        Err(WattWardenError::Unsupported(
            "macOS exposes no CPU frequency scaling interface".into(),
        ))
    }

    /// Frequency limit is unsupported on macOS.
    fn freq_limit(&self) -> Result<u32> {
        Err(WattWardenError::Unsupported(
            "macOS exposes no CPU frequency scaling interface".into(),
        ))
    }

    /// Setting frequency limit is a no-op on macOS.
    fn set_freq_limit(&self, _mhz: u32) -> Result<()> {
        Ok(())
    }

    /// Turbo boost telemetry placeholder.
    fn turbo_enabled(&self) -> Result<bool> {
        Ok(true)
    }

    /// Setting turbo boost is a no-op on macOS.
    fn set_turbo_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Energy performance preference placeholder.
    fn energy_performance_preference(&self) -> Result<String> {
        Ok("default".into())
    }

    /// Setting energy performance preference is a no-op on macOS.
    fn set_energy_performance_preference(&self, _pref: &str) -> Result<()> {
        Ok(())
    }
}

impl CStateTelemetry for MacOsCpu {
    /// macOS does not expose raw C-state telemetry.
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        Ok(Vec::new())
    }
}

pub struct MacOsDisplay;

impl DisplayManager for MacOsDisplay {
    /// Reads display brightness percentage.
    fn brightness_percent(&self) -> Result<u8> {
        Ok(parse::mac_brightness_percent(&exec::run_capture(
            "brightness",
            &["-l"],
        )))
    }

    /// Sets display brightness clamped between 1% and 100%.
    fn set_brightness_percent(&self, percent: u8) -> Result<()> {
        let p = percent.clamp(1, 100);
        exec::run_ignored("brightness", &[&format!("{:.2}", f64::from(p) / 100.0)]);
        Ok(())
    }
}

pub struct MacOsThreshold;

impl ChargeThreshold for MacOsThreshold {
    /// macOS manages charging thresholds natively; custom thresholds are unsupported.
    fn supports_threshold(&self) -> bool {
        false
    }

    fn charge_threshold(&self) -> Result<u8> {
        Err(WattWardenError::Unsupported(
            "macOS battery thresholds managed natively via Optimized Battery Charging".into(),
        ))
    }

    fn set_charge_threshold(&self, _threshold: u8) -> Result<()> {
        Err(WattWardenError::Unsupported(
            "macOS battery thresholds managed natively via Optimized Battery Charging".into(),
        ))
    }
}

pub struct MacOsPeripherals;

impl PeripheralsController for MacOsPeripherals {
    /// Keyboard backlight state.
    fn kbd_backlight(&self) -> Result<bool> {
        Ok(false)
    }

    /// Setting keyboard backlight is a no-op on macOS.
    fn set_kbd_backlight(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Queries Bluetooth state via system preferences.
    fn bluetooth_enabled(&self) -> Result<bool> {
        let out = exec::run_capture(
            "defaults",
            &[
                "read",
                "/Library/Preferences/com.apple.Bluetooth",
                "ControllerPowerState",
            ],
        );
        Ok(out.trim() != "0")
    }

    /// Configures Bluetooth state via defaults key and blueutil.
    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()> {
        let val = if enabled { "1" } else { "0" };
        exec::run_ignored(
            "defaults",
            &[
                "write",
                "/Library/Preferences/com.apple.Bluetooth",
                "ControllerPowerState",
                "-int",
                val,
            ],
        );
        exec::run_ignored("blueutil", &["--power", val]);
        Ok(())
    }

    /// Queries Wi-Fi power status.
    fn wifi_enabled(&self) -> Result<bool> {
        let dev = parse::mac_wifi_device(&exec::run_capture(
            "networksetup",
            &["-listallhardwareports"],
        ));
        let out = exec::run_capture("networksetup", &["-getairportpower", &dev]);
        Ok(out.to_lowercase().contains("on"))
    }

    /// Configures Wi-Fi power status.
    fn set_wifi_enabled(&self, enabled: bool) -> Result<()> {
        let dev = parse::mac_wifi_device(&exec::run_capture(
            "networksetup",
            &["-listallhardwareports"],
        ));
        let val = if enabled { "on" } else { "off" };
        exec::run_ignored("networksetup", &["-setairportpower", &dev, val]);
        Ok(())
    }
}

pub struct MacOsTweaks;

impl SystemTweaksController for MacOsTweaks {
    /// Wi-Fi power saving status.
    fn wifi_power_save(&self) -> Result<bool> {
        Ok(false)
    }

    /// Setting Wi-Fi power saving is a no-op on macOS.
    fn set_wifi_power_save(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Audio power saving status.
    fn audio_power_save(&self) -> Result<bool> {
        Ok(false)
    }

    /// Setting audio power saving is a no-op on macOS.
    fn set_audio_power_save(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// USB autosuspend status.
    fn autosuspend(&self) -> Result<bool> {
        Ok(false)
    }

    /// Setting USB autosuspend is a no-op on macOS.
    fn set_autosuspend(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Hardware watchdog status.
    fn nmi_watchdog(&self) -> Result<bool> {
        Ok(true)
    }

    /// Setting hardware watchdog is a no-op on macOS.
    fn set_nmi_watchdog(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// VM writeback cache delay in seconds (defaults to 5 seconds).
    fn vm_writeback_seconds(&self) -> Result<u32> {
        Ok(5)
    }

    /// Setting VM writeback delay is a no-op on macOS.
    fn set_vm_writeback_seconds(&self, _seconds: u32) -> Result<()> {
        Ok(())
    }

    /// Purges inactive memory caches via the `purge` command.
    fn process_purge(&self) -> Result<()> {
        exec::run_ignored("purge", &[]);
        Ok(())
    }
}

pub struct MacOsCompositor;

impl CompositorFocus for MacOsCompositor {
    /// Window focus tracking is unsupported outside Linux X11/Wayland compositors.
    fn active_window_class(&self) -> Option<String> {
        None
    }
}

pub struct MacOsBackend {
    pub battery: MacOsBattery,
    pub cpu: MacOsCpu,
    pub rapl: Option<Box<dyn RaplController>>,
    pub gpu: Option<Box<dyn GpuController>>,
    pub aspm: Option<Box<dyn AspmController>>,
    pub backlight: Option<MacOsDisplay>,
    pub threshold: MacOsThreshold,
    pub peripherals: MacOsPeripherals,
    pub tweaks: MacOsTweaks,
    pub hyprland: MacOsCompositor,
}

impl MacOsBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            battery: MacOsBattery,
            cpu: MacOsCpu,
            rapl: None,
            gpu: None,
            aspm: None,
            backlight: Some(MacOsDisplay),
            threshold: MacOsThreshold,
            peripherals: MacOsPeripherals,
            tweaks: MacOsTweaks,
            hyprland: MacOsCompositor,
        })
    }

    /// 1-minute load average from `sysctl -n vm.loadavg`, formatted for
    /// normalized scaling across logical CPU count. Returns `0.0` on error.
    pub fn load_average(&self) -> f64 {
        let out = exec::run_capture("sysctl", &["-n", "vm.loadavg"]);
        let cleaned = out.trim().trim_matches(['{', '}']).trim();
        cleaned
            .split_whitespace()
            .next()
            .and_then(|first| first.parse::<f64>().ok())
            .unwrap_or(0.0)
    }

    /// Applies performance power management mode.
    pub fn apply_mode_performance(&self) {
        exec::run_ignored("pmset", &["-a", "lowpowermode", "0"]);
        exec::run_ignored("pmset", &["-a", "tcpkeepalive", "1"]);
        exec::run_ignored("pmset", &["-a", "displaysleep", "10"]);
    }

    /// Applies extreme power saving mode.
    pub fn apply_mode_extreme(&self) {
        exec::run_ignored("pmset", &["-a", "lowpowermode", "1"]);
        exec::run_ignored("pmset", &["-a", "tcpkeepalive", "0"]);
        exec::run_ignored("pmset", &["-a", "displaysleep", "3"]);
    }

    /// Restores standard power management mode.
    pub fn apply_mode_restore(&self) {
        exec::run_ignored("pmset", &["-a", "lowpowermode", "0"]);
        exec::run_ignored("pmset", &["-a", "tcpkeepalive", "1"]);
        exec::run_ignored("pmset", &["-a", "displaysleep", "10"]);
    }

    /// Operating system identifier.
    pub fn os_name(&self) -> &'static str {
        "macOS"
    }

    pub fn capabilities(&self) -> HardwareCapabilities {
        HardwareCapabilities {
            has_battery: !self.battery.is_stationary(),
            is_stationary_mains: self.battery.is_stationary(),
            has_cpu_frequency_control: self.cpu.freq_bounds().is_ok(),
            has_cpu_core_control: false,
            has_rapl: false,
            has_gpu_control: false,
            has_backlight_control: true,
            has_charge_threshold: false,
            has_peripherals_control: true,
            has_system_tweaks: true,
            has_compositor_focus: false,
        }
    }

    /// Maps [`PowerProfile`] to native power management modes.
    pub fn apply_profile(&self, profile: &PowerProfile) -> Result<()> {
        match profile {
            PowerProfile::Performance => self.apply_mode_performance(),
            PowerProfile::Extreme => self.apply_mode_extreme(),
            PowerProfile::Normal | PowerProfile::AutoExtreme => self.apply_mode_restore(),
        }
        Ok(())
    }
}

/// Integration tests for native macOS command runner mocks.
///
/// Uses mock `pmset`/`ioreg`/`purge` shell scripts on `PATH`. The
/// scripts are POSIX `sh`, so the test runs here on Linux as well as on macOS.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Prepends `dir` to `PATH` and points `WATTWARDEN_TEST_LOG` at the log, running
    /// `body` under the process-wide env lock.
    fn with_fake_path<T>(dir: &Path, log: &Path, body: impl FnOnce() -> T) -> T {
        let _guard = crate::exec::env_lock();
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs = vec![dir.to_path_buf()];
        dirs.extend(std::env::split_paths(&old_path));
        std::env::set_var("PATH", std::env::join_paths(dirs).unwrap());
        std::env::set_var("WATTWARDEN_TEST_LOG", log);

        let result = body();

        std::env::set_var("PATH", &old_path);
        std::env::remove_var("WATTWARDEN_TEST_LOG");
        result
    }

    #[test]
    fn os_name_identifies_macos() {
        assert_eq!(MacOsBackend::new().unwrap().os_name(), "macOS");
    }

    #[test]
    fn native_commands_drive_the_backend() {
        let dir = std::env::temp_dir().join(format!("ww_mac_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("commands.log");

        write_script(
            &dir,
            "pmset",
            "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$WATTWARDEN_TEST_LOG\"\n\
             if [ \"$1\" = \"-g\" ]; then printf '%s\\n' \"Now drawing from 'Battery Power'\" \
             ' -InternalBattery-0 (id=1) 82%; discharging; 4:12 remaining; present: true'\nfi\n",
        );
        write_script(
            &dir,
            "ioreg",
            "#!/bin/sh\nprintf '%s\\n' '\"Current\" = 1000' '\"Voltage\" = 12000'\n",
        );
        write_script(
            &dir,
            "purge",
            "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$WATTWARDEN_TEST_LOG\"\n",
        );

        let backend = MacOsBackend::new().unwrap();
        with_fake_path(&dir, &log, || {
            assert_eq!(backend.battery.battery_percentage().unwrap(), 82);
            assert!(!backend.battery.is_charging().unwrap());
            assert_eq!(backend.battery.time_remaining().unwrap(), "4:12");
            assert_eq!(backend.battery.consumption_watts().unwrap(), 12.0);

            backend.apply_mode_extreme();
            backend.tweaks.process_purge().unwrap();

            let output = fs::read_to_string(&log).unwrap();
            assert!(
                output.contains("pmset -a lowpowermode 1"),
                "native commands were not invoked: {output}"
            );
            assert!(output.contains("purge"), "purge was not invoked: {output}");
        });

        let _ = fs::remove_dir_all(&dir);
    }
}
