//! Windows backend — the Go reference (`hal/backend_windows.go`) ported function by
//! function. See `PARITY.md` §6 for the Go → Rust table and what is deliberately
//! left unsupported.
//!
//! This module is compiled on **every** host, not just `target_os = "windows"`: the
//! only Windows-specific piece is `GetSystemPowerStatus` (via `kernel32`, the same
//! call Go's `getPowerStatus()` makes — no child process in the battery hot path,
//! AGENTS.md), which is `cfg`-gated out elsewhere so the Go-parity test can run
//! against fake binaries on `PATH` from Linux. `lib.rs` still only re-exports it as
//! `PlatformBackend` on Windows.
//!
//! No hardware is required to boot: a machine without a battery reports the Go
//! defaults and every write is a no-op when the command is missing.

use crate::exec;
use crate::parse;
use wattwarden_core::*;

/// Go `GetPowerConsumptionWatts` query: `BatteryStatus.PowerOnline`, `.Voltage` and
/// `.DischargeRate` from `root\wmi`.
const BATTERY_STATUS_PS: &str = r"(Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1).PowerOnline, (Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1).Voltage, (Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus | Select-Object -First 1).DischargeRate";

/// Win32 `SYSTEM_POWER_STATUS`: the `kernel32!GetSystemPowerStatus` layout, which is
/// what Go reads through `syscall`. Field order/type match the C struct exactly.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    // `SystemStatusFlag` and `BatteryFullLifeTime` are part of the Win32 layout but
    // not surfaced by Go; kept only to size the struct correctly for the FFI call.
    _system_status_flag: u8,
    battery_life_time: u32,
    _battery_full_life_time: u32,
}

/// Go `getPowerStatus()`: `GetSystemPowerStatus`, `None` when the call fails.
#[cfg(target_os = "windows")]
fn get_power_status() -> Option<SystemPowerStatus> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemPowerStatus(system_power_status: *mut SystemPowerStatus) -> i32;
    }

    let mut sps = SystemPowerStatus::default();
    // SAFETY: the FFI writes into a correctly sized, `repr(C)` struct mirroring the
    // Win32 `SYSTEM_POWER_STATUS`; the pointer is only handed to the call.
    let ret = unsafe { GetSystemPowerStatus(std::ptr::addr_of_mut!(sps)) };
    if ret == 0 {
        None
    } else {
        Some(sps)
    }
}

/// Non-Windows stub so the module still compiles (and its tests still run) on the
/// other hosts. Go returns `nil` when the call fails, which is exactly the fallback
/// the callers already handle.
#[cfg(not(target_os = "windows"))]
fn get_power_status() -> Option<SystemPowerStatus> {
    None
}

pub struct WindowsBattery;

impl PowerSource for WindowsBattery {
    /// Go `GetBatteryPercentage`.
    fn battery_percentage(&self) -> Result<u8> {
        Ok(match get_power_status() {
            Some(sps) => parse::win_battery_percent(sps.battery_life_percent),
            None => 100,
        })
    }

    /// Go `IsCharging`: `ACLineStatus == 1`, `true` when the call fails.
    fn is_charging(&self) -> Result<bool> {
        Ok(match get_power_status() {
            Some(sps) => sps.ac_line_status == 1,
            None => true,
        })
    }

    /// Go `GetPowerConsumptionWatts`.
    fn consumption_watts(&self) -> Result<f64> {
        let out = exec::run_capture("powershell", &["-NoProfile", "-Command", BATTERY_STATUS_PS]);
        Ok(parse::win_power_watts(&out))
    }

    /// Go `GetBatteryTime`.
    fn time_remaining(&self) -> Result<String> {
        Ok(parse::win_time_remaining(get_power_status().map(|sps| {
            (sps.ac_line_status == 1, sps.battery_life_time)
        })))
    }

    /// Rust-only capability probe (Go has no `IsStationary`): the Win32 `BatteryFlag`
    /// bit `0x80` ("No system battery", also `0xFF` = unknown) means AC mains. The
    /// app boots on a desktop/VM either way (AGENTS.md).
    fn is_stationary(&self) -> bool {
        match get_power_status() {
            Some(sps) => sps.battery_flag & 0x80 != 0 || sps.battery_flag == 0xFF,
            None => true,
        }
    }
}

pub struct WindowsCpu;

impl CpuGovernor for WindowsCpu {
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

    /// Go `SetCores` is a stub.
    fn set_online_cores(&self, _count: usize) -> Result<()> {
        Ok(())
    }

    /// Windows manages frequency through `powercfg` processor percentages, not a
    /// MHz interface, so there is no range to discover. `Unsupported` (not a
    /// fabricated `Ok`) is what keeps the shared ladder from writing a frequency.
    fn freq_bounds(&self) -> Result<(u32, u32)> {
        Err(WattWardenError::Unsupported(
            "Windows manages CPU frequency through powercfg processor percentages".into(),
        ))
    }

    /// Go `GetFreqLimit` returns `0`; there is no MHz limit to report.
    fn freq_limit(&self) -> Result<u32> {
        Err(WattWardenError::Unsupported(
            "Windows manages CPU frequency through powercfg processor percentages".into(),
        ))
    }

    /// Go `SetFreqLimit` is a no-op.
    fn set_freq_limit(&self, _mhz: u32) -> Result<()> {
        Ok(())
    }

    /// Go `GetTurbo`: `PERFBOOSTMODE` is not `0x00000000`.
    fn turbo_enabled(&self) -> Result<bool> {
        let out = exec::run_capture(
            "powercfg",
            &["/query", "SCHEME_CURRENT", "SUB_PROCESSOR", "PERFBOOSTMODE"],
        );
        Ok(!out.contains("0x00000000"))
    }

    /// Go `SetTurbo`: `2` (aggressive) / `0` (disabled) on **both** AC and DC.
    fn set_turbo_enabled(&self, enabled: bool) -> Result<()> {
        let val = if enabled { "2" } else { "0" };
        exec::run_ignored(
            "powercfg",
            &[
                "-setacvalueindex",
                "SCHEME_CURRENT",
                "SUB_PROCESSOR",
                "PERFBOOSTMODE",
                val,
            ],
        );
        exec::run_ignored(
            "powercfg",
            &[
                "-setdcvalueindex",
                "SCHEME_CURRENT",
                "SUB_PROCESSOR",
                "PERFBOOSTMODE",
                val,
            ],
        );
        exec::run_ignored("powercfg", &["-setactive", "SCHEME_CURRENT"]);
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

impl CStateTelemetry for WindowsCpu {
    /// Go has no C-state telemetry on Windows.
    fn cstates(&self) -> Result<Vec<CStateInfo>> {
        Ok(Vec::new())
    }
}

pub struct WindowsDisplay;

impl DisplayManager for WindowsDisplay {
    /// Go `GetLCDBrightness`.
    fn brightness_percent(&self) -> Result<u8> {
        let out = exec::run_capture(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightness).CurrentBrightness",
            ],
        );
        Ok(parse::win_brightness(&out))
    }

    /// Go `SetLCDBrightness`: clamp to `0..100` (Go allows `0`).
    fn set_brightness_percent(&self, percent: u8) -> Result<()> {
        let p = percent.min(100);
        exec::run_ignored(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                &format!("(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1, {p})"),
            ],
        );
        Ok(())
    }
}

pub struct WindowsThreshold;

impl ChargeThreshold for WindowsThreshold {
    /// Go has no `GetChargeThreshold`; vendors manage it (Lenovo Vantage, …).
    fn supports_threshold(&self) -> bool {
        false
    }

    fn charge_threshold(&self) -> Result<u8> {
        Err(WattWardenError::Unsupported(
            "Windows battery thresholds managed via vendor software (Lenovo Vantage, MyASUS, Dell Command)".into(),
        ))
    }

    fn set_charge_threshold(&self, _threshold: u8) -> Result<()> {
        Err(WattWardenError::Unsupported(
            "Windows battery thresholds managed via vendor software (Lenovo Vantage, MyASUS, Dell Command)".into(),
        ))
    }
}

pub struct WindowsPeripherals;

impl PeripheralsController for WindowsPeripherals {
    /// Go `GetKbdBacklight` returns `false`.
    fn kbd_backlight(&self) -> Result<bool> {
        Ok(false)
    }

    /// Go `SetKbdBacklight` is a no-op.
    fn set_kbd_backlight(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    /// Go `GetBluetooth` returns `true`.
    fn bluetooth_enabled(&self) -> Result<bool> {
        Ok(true)
    }

    /// Go `SetBluetooth`: start/stop the `bthserv` service via PowerShell.
    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()> {
        let status = if enabled { "Running" } else { "Stopped" };
        exec::run_ignored(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                &format!(
                    "Set-Service -Name bthserv -Status {status} -ErrorAction SilentlyContinue"
                ),
            ],
        );
        Ok(())
    }

    /// Go `GetWifiEnable`.
    fn wifi_enabled(&self) -> Result<bool> {
        let out = exec::run_capture("netsh", &["interface", "show", "interface"]);
        Ok(parse::win_wifi_enabled(&out))
    }

    /// Go `SetWifiEnable`.
    fn set_wifi_enabled(&self, enabled: bool) -> Result<()> {
        let admin = if enabled { "ENABLED" } else { "DISABLED" };
        exec::run_ignored(
            "netsh",
            &[
                "interface",
                "set",
                "interface",
                "name=\"Wi-Fi\"",
                &format!("admin={admin}"),
            ],
        );
        Ok(())
    }
}

pub struct WindowsTweaks;

impl SystemTweaksController for WindowsTweaks {
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

    /// Go `ProcessPurge`: force a .NET GC from PowerShell.
    fn process_purge(&self) -> Result<()> {
        exec::run_ignored(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "[System.GC]::Collect(); [System.GC]::WaitForPendingFinalizers()",
            ],
        );
        Ok(())
    }
}

pub struct WindowsCompositor;

impl CompositorFocus for WindowsCompositor {
    /// Go has no active-window tracker outside Hyprland/X11.
    fn active_window_class(&self) -> Option<String> {
        None
    }
}

/// Go `setWinProcThrottle`: `PROCTHROTTLEMAX` (clamped `1..100`) on both AC and DC.
fn set_proc_throttle(max_percent: i32) {
    let p = max_percent.clamp(1, 100);
    exec::run_ignored(
        "powercfg",
        &[
            "-setacvalueindex",
            "SCHEME_CURRENT",
            "SUB_PROCESSOR",
            "PROCTHROTTLEMAX",
            &p.to_string(),
        ],
    );
    exec::run_ignored(
        "powercfg",
        &[
            "-setdcvalueindex",
            "SCHEME_CURRENT",
            "SUB_PROCESSOR",
            "PROCTHROTTLEMAX",
            &p.to_string(),
        ],
    );
    exec::run_ignored("powercfg", &["-setactive", "SCHEME_CURRENT"]);
}

pub struct WindowsBackend {
    pub battery: WindowsBattery,
    pub cpu: WindowsCpu,
    pub rapl: Option<Box<dyn RaplController>>,
    pub gpu: Option<Box<dyn GpuController>>,
    pub aspm: Option<Box<dyn AspmController>>,
    pub backlight: Option<WindowsDisplay>,
    pub threshold: WindowsThreshold,
    pub peripherals: WindowsPeripherals,
    pub tweaks: WindowsTweaks,
    pub hyprland: WindowsCompositor,
}

impl WindowsBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            battery: WindowsBattery,
            cpu: WindowsCpu,
            rapl: None,
            gpu: None,
            aspm: None,
            backlight: Some(WindowsDisplay),
            threshold: WindowsThreshold,
            peripherals: WindowsPeripherals,
            tweaks: WindowsTweaks,
            hyprland: WindowsCompositor,
        })
    }

    /// Approximate CPU load, expressed in the units Linux `/proc/loadavg` reports so
    /// the shared adaptive ladder can normalize it by the CPU count.
    ///
    /// Go `getWinLoad()` measures `\Processor Information(_Total)\% Processor Time`
    /// with `typeperf` and returns the `0.0..1.0` fraction its caller uses directly;
    /// multiplying that fraction by the CPU count makes the ladder's `load / ncpu`
    /// reproduce the exact same power level. `0.0` — the idle step — when `typeperf`
    /// is unavailable or unparsable, exactly like Go.
    pub fn load_average(&self) -> f64 {
        let out = exec::run_capture(
            "typeperf",
            &[
                r"\Processor Information(_Total)\% Processor Time",
                "-sc",
                "1",
            ],
        );
        parse::win_load_fraction(&out) * self.cpu.num_cpus() as f64
    }

    /// Go `ApplyModePerformance`.
    pub fn apply_mode_performance(&self) {
        set_proc_throttle(100);
    }

    /// Go `ApplyModeExtreme`: `1%` throttle plus a dim display.
    pub fn apply_mode_extreme(&self) {
        set_proc_throttle(1);
        if let Some(bl) = &self.backlight {
            let _ = bl.set_brightness_percent(10);
        }
    }

    /// Go `ApplyModeRestore`.
    pub fn apply_mode_restore(&self) {
        set_proc_throttle(100);
    }

    /// Go `GetOS()` (`backend_windows.go:58`). The dashboard uses it to pick the menu
    /// rows and the summary line.
    pub fn os_name(&self) -> &'static str {
        "Windows"
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
    /// "Auto Extreme" profile, so it shares the restore/performance throttle.
    pub fn apply_profile(&self, profile: &PowerProfile) -> Result<()> {
        match profile {
            PowerProfile::Performance => self.apply_mode_performance(),
            PowerProfile::Extreme => self.apply_mode_extreme(),
            PowerProfile::Normal | PowerProfile::AutoExtreme => self.apply_mode_restore(),
        }
        Ok(())
    }
}

/// Port of Go `TestWindowsBackendNativeCommands` (`hal/backend_windows_test.go`).
///
/// Same fake `powershell`/`powercfg` scripts and assertions, but written as POSIX
/// `sh` so the test also runs on Linux/macOS. It is **not** run on Windows: Rust's
/// `std::process::Command` uses `CreateProcess`, which does not consult `PATHEXT`
/// the way Go's `exec.LookPath` does, so a fake `powershell.cmd` can never be
/// resolved from `Command::new("powershell")`. On Windows the pure parsers in
/// `crate::parse` are the coverage that runs.
#[cfg(all(test, not(target_os = "windows")))]
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
        assert_eq!(WindowsBackend::new().unwrap().os_name(), "Windows");
    }

    #[test]
    fn native_commands_drive_the_backend() {
        let dir = std::env::temp_dir().join(format!("ww_win_go_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("commands.log");

        write_script(
            &dir,
            "powershell",
            "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$WATTWARDEN_TEST_LOG\"\necho 50\n",
        );
        write_script(
            &dir,
            "powercfg",
            "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$WATTWARDEN_TEST_LOG\"\n",
        );

        let backend = WindowsBackend::new().unwrap();
        with_fake_path(&dir, &log, || {
            assert_eq!(
                backend
                    .backlight
                    .as_ref()
                    .unwrap()
                    .brightness_percent()
                    .unwrap(),
                50
            );

            backend.apply_mode_extreme();
            backend.apply_mode_restore();

            let output = fs::read_to_string(&log).unwrap();
            assert!(
                output.contains("powercfg"),
                "powercfg was not invoked: {output}"
            );
            // Extreme dims the display through PowerShell exactly like Go.
            assert!(
                output.contains("WmiSetBrightness(1, 10)"),
                "extreme brightness was not set: {output}"
            );
        });

        let _ = fs::remove_dir_all(&dir);
    }
}
