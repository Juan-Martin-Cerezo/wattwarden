//! macOS backend — the Go reference (`hal/backend_darwin.go`) ported function by
//! function. See `PARITY.md` §6 for the Go → Rust table and what is deliberately
//! left unsupported.
//!
//! This module is compiled on **every** host, not just `target_os = "macos"`: the
//! backend only ever shells out to `pmset`/`ioreg`/`networksetup`/… (no macOS-only
//! API), so its Go-parity test runs against fake binaries on `PATH` from Linux too.
//! `lib.rs` still only re-exports it as `PlatformBackend` on macOS.
//!
//! No hardware is required to boot: every read falls back to the Go default and
//! every write is a no-op when the command is missing (AGENTS.md).

use crate::exec;
use crate::parse;
use wattwarden_core::*;

pub struct MacOsBattery;

impl PowerSource for MacOsBattery {
    /// Go `GetBatteryPercentage`.
    fn battery_percentage(&self) -> Result<u8> {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        Ok(parse::mac_battery_percent(&out))
    }

    /// Go `IsCharging`.
    fn is_charging(&self) -> Result<bool> {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        Ok(parse::mac_is_charging(&out))
    }

    /// Go `GetPowerConsumptionWatts`.
    fn consumption_watts(&self) -> Result<f64> {
        let out = exec::run_capture("ioreg", &["-rn", "AppleSmartBattery"]);
        Ok(parse::mac_power_watts(&out))
    }

    /// Go `GetBatteryTime`.
    fn time_remaining(&self) -> Result<String> {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        let charging = parse::mac_is_charging(&out);
        Ok(parse::mac_time_remaining(&out, charging))
    }

    /// Rust-only capability probe (Go has no `IsStationary`): a Mac without an
    /// internal battery runs on AC mains. A failed read also reports stationary so
    /// the app boots on any Mac (AGENTS.md).
    fn is_stationary(&self) -> bool {
        let out = exec::run_capture("pmset", &["-g", "batt"]);
        !out.contains("InternalBattery")
    }
}

pub struct MacOsCpu;

impl CpuGovernor for MacOsCpu {
    /// Go `GetNumCPUs`.
    fn num_cpus(&self) -> usize {
        std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
    }

    /// Go `GetCores`.
    fn online_cores(&self) -> Result<usize> {
        Ok(self.num_cpus())
    }

    /// Go `SetCores` is a no-op: macOS does not expose core offlining.
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

    /// Go `GetFreqLimit` has no meaningful value here (no frequency control).
    fn freq_limit(&self) -> Result<u32> {
        Err(WattWardenError::Unsupported(
            "macOS exposes no CPU frequency scaling interface".into(),
        ))
    }

    /// Go `SetFreqLimit` is a no-op.
    fn set_freq_limit(&self, _mhz: u32) -> Result<()> {
        Ok(())
    }

    /// Go `GetTurbo` returns `true` on macOS.
    fn turbo_enabled(&self) -> Result<bool> {
        Ok(true)
    }

    /// Go `SetTurbo` is a no-op.
    fn set_turbo_enabled(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Go `GetEPP` returns `"default"`.
    fn energy_performance_preference(&self) -> Result<String> {
        Ok("default".into())
    }

    /// Go `SetEPP` is a no-op.
    fn set_energy_performance_preference(&self, _pref: &str) -> Result<()> {
        Ok(())
    }
}

impl CStateTelemetry for MacOsCpu {
    /// Go has no C-state telemetry on macOS.
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        Ok(Vec::new())
    }
}

pub struct MacOsDisplay;

impl DisplayManager for MacOsDisplay {
    /// Go `GetLCDBrightness`.
    fn brightness_percent(&self) -> Result<u8> {
        Ok(parse::mac_brightness_percent(&exec::run_capture(
            "brightness",
            &["-l"],
        )))
    }

    /// Go `SetLCDBrightness`: clamp `1..100`, write the `0.00..1.00` fraction.
    fn set_brightness_percent(&self, percent: u8) -> Result<()> {
        let p = percent.clamp(1, 100);
        exec::run_ignored("brightness", &[&format!("{:.2}", f64::from(p) / 100.0)]);
        Ok(())
    }
}

pub struct MacOsThreshold;

impl ChargeThreshold for MacOsThreshold {
    /// Go has no `GetChargeThreshold`; charging is managed by macOS itself.
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
    /// Go `GetKbdBacklight` returns `false`.
    fn kbd_backlight(&self) -> Result<bool> {
        Ok(false)
    }

    /// Go `SetKbdBacklight` is a no-op.
    fn set_kbd_backlight(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Go `GetBluetooth`.
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

    /// Go `SetBluetooth`: the defaults key *and* `blueutil --power`.
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

    /// Go `GetWifiEnable`.
    fn wifi_enabled(&self) -> Result<bool> {
        let dev = parse::mac_wifi_device(&exec::run_capture(
            "networksetup",
            &["-listallhardwareports"],
        ));
        let out = exec::run_capture("networksetup", &["-getairportpower", &dev]);
        Ok(out.to_lowercase().contains("on"))
    }

    /// Go `SetWifiEnable`.
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
    /// Go `GetWifiPowerSave` returns `false`.
    fn wifi_power_save(&self) -> Result<bool> {
        Ok(false)
    }

    /// Go `SetWifiPowerSave` is a no-op.
    fn set_wifi_power_save(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Go `GetAudioPowerSave` returns `false`.
    fn audio_power_save(&self) -> Result<bool> {
        Ok(false)
    }

    /// Go `SetAudioPowerSave` is a no-op.
    fn set_audio_power_save(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Go `GetAutosuspend` returns `false`.
    fn autosuspend(&self) -> Result<bool> {
        Ok(false)
    }

    /// Go `SetAutosuspend` is a no-op.
    fn set_autosuspend(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Go `GetWatchdog` returns `true`.
    fn nmi_watchdog(&self) -> Result<bool> {
        Ok(true)
    }

    /// Go `SetWatchdog` is a no-op.
    fn set_nmi_watchdog(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Go `GetVMWriteback` returns `500` centisecs = `5` s, which is what the
    /// inherited [`SystemTweaksController::vm_writeback_centisecs`] scales back to.
    fn vm_writeback_seconds(&self) -> Result<u32> {
        Ok(5)
    }

    /// Go `SetVMWriteback` is a no-op.
    fn set_vm_writeback_seconds(&self, _seconds: u32) -> Result<()> {
        Ok(())
    }

    /// Go `ProcessPurge`.
    fn process_purge(&self) -> Result<()> {
        exec::run_ignored("purge", &[]);
        Ok(())
    }
}

pub struct MacOsCompositor;

impl CompositorFocus for MacOsCompositor {
    /// Go has no active-window tracker outside Hyprland/X11.
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

    /// 1-minute load average from `sysctl -n vm.loadavg`, in the units Linux
    /// `/proc/loadavg` reports so the shared adaptive ladder can normalize it by the
    /// CPU count. Go `getMacLoad()` divides by `NumCPU` itself and its caller uses
    /// that directly, which is the same power level. `0.0` — the idle step — when the
    /// query fails, exactly like Go.
    pub fn load_average(&self) -> f64 {
        let out = exec::run_capture("sysctl", &["-n", "vm.loadavg"]);
        let cleaned = out.trim().trim_matches(['{', '}']).trim();
        cleaned
            .split_whitespace()
            .next()
            .and_then(|first| first.parse::<f64>().ok())
            .unwrap_or(0.0)
    }

    /// Go `ApplyModePerformance`.
    pub fn apply_mode_performance(&self) {
        exec::run_ignored("pmset", &["-a", "lowpowermode", "0"]);
        exec::run_ignored("pmset", &["-a", "tcpkeepalive", "1"]);
        exec::run_ignored("pmset", &["-a", "displaysleep", "10"]);
    }

    /// Go `ApplyModeExtreme`.
    pub fn apply_mode_extreme(&self) {
        exec::run_ignored("pmset", &["-a", "lowpowermode", "1"]);
        exec::run_ignored("pmset", &["-a", "tcpkeepalive", "0"]);
        exec::run_ignored("pmset", &["-a", "displaysleep", "3"]);
    }

    /// Go `ApplyModeRestore`.
    pub fn apply_mode_restore(&self) {
        exec::run_ignored("pmset", &["-a", "lowpowermode", "0"]);
        exec::run_ignored("pmset", &["-a", "tcpkeepalive", "1"]);
        exec::run_ignored("pmset", &["-a", "displaysleep", "10"]);
    }

    /// Go `GetOS()` (`backend_darwin.go:42`). The dashboard uses it to pick the menu
    /// rows and the summary line.
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

    /// Maps the shared [`PowerProfile`] onto the Go mode functions. Go has no
    /// "Auto Extreme" profile, so it shares the restore/performance pmset set.
    pub fn apply_profile(&self, profile: &PowerProfile) -> Result<()> {
        match profile {
            PowerProfile::Performance => self.apply_mode_performance(),
            PowerProfile::Extreme => self.apply_mode_extreme(),
            PowerProfile::Normal | PowerProfile::AutoExtreme => self.apply_mode_restore(),
        }
        Ok(())
    }
}

/// Port of Go `TestDarwinBackendNativeCommands` (`hal/backend_darwin_test.go`).
///
/// Same fake `pmset`/`ioreg`/`purge` shell scripts on `PATH`, same assertions. The
/// scripts are POSIX `sh`, so the test runs here on Linux *and* in the macOS CI;
/// only the module's `pub use` is gated, the logic is not.
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
    /// `body` under the process-wide env lock (the Go test uses `t.Setenv`).
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
    fn os_name_matches_go() {
        assert_eq!(MacOsBackend::new().unwrap().os_name(), "macOS");
    }

    #[test]
    fn native_commands_drive_the_backend() {
        let dir = std::env::temp_dir().join(format!("ww_mac_go_{}", std::process::id()));
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
