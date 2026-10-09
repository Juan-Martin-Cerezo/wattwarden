// Only Linux (systemd) shells out from the dashboard; macOS
// drives everything through its backend.
#[cfg(target_os = "linux")]
use std::process::Command;
use std::time::{Duration, Instant};
use wattwarden_core::*;
use wattwarden_daemon::{spawn, PidManager};
use wattwarden_platform::PlatformBackend as LinuxBackend;

/// Default and bounds for the telemetry graph sampling interval.
const DEFAULT_REFRESH_DELAY: Duration = Duration::from_millis(2000);
const MIN_REFRESH_DELAY: Duration = Duration::from_millis(500);
const MAX_REFRESH_DELAY: Duration = Duration::from_secs(10);
const REFRESH_STEP: Duration = Duration::from_millis(500);

/// How often the (cheap, fork-free) daemon liveness probe is refreshed.
const DAEMON_STATE_TTL: Duration = Duration::from_secs(1);

const VM_WRITEBACK_MIN_CENTISECS: i64 = 100;
const VM_WRITEBACK_MAX_CENTISECS: i64 = 6000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionItem {
    Header(String),
    ProfilePerformance,
    ProfileExtreme,
    ProfileAutoExtreme,
    AutoExtremeLevel,
    AutoBrightness,
    ProfileRestore,
    Cores,
    FreqLimit,
    GpuFreq,
    RaplPl1,
    RaplPl2,
    Turbo,
    Epp,
    Aspm,
    Brightness,
    KbdBacklight,
    Bluetooth,
    WifiEnable,
    WifiPowerSave,
    AudioPowerSave,
    Autosuspend,
    Watchdog,
    VmWriteback,
    ProcessPurge,
}

impl ActionItem {
    pub fn is_header(&self) -> bool {
        matches!(self, ActionItem::Header(_))
    }

    /// Manual hardware adjustment pauses the background daemon to prioritize user control.
    fn stops_daemon_on_adjust(&self) -> bool {
        matches!(
            self,
            ActionItem::Cores
                | ActionItem::FreqLimit
                | ActionItem::GpuFreq
                | ActionItem::RaplPl1
                | ActionItem::RaplPl2
                | ActionItem::Turbo
                | ActionItem::KbdBacklight
                | ActionItem::Bluetooth
                | ActionItem::WifiEnable
                | ActionItem::WifiPowerSave
                | ActionItem::AudioPowerSave
                | ActionItem::Autosuspend
                | ActionItem::Watchdog
                | ActionItem::VmWriteback
        )
    }
}

/// Checks whether the background daemon is running via fork-free PID-file check.
fn daemon_is_active() -> bool {
    PidManager::new().is_running()
}

/// Formats a `std::time::Duration` as human-readable string (500ms, 1s, 1.5s ... 10s).
pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let mut secs = format!("{:.3}", d.as_secs_f64());
    while secs.ends_with('0') {
        secs.pop();
    }
    if secs.ends_with('.') {
        secs.pop();
    }
    format!("{secs}s")
}

/// Builds the menu layout dynamically based on the detected operating system name.
fn build_menu(os_name: &str) -> Vec<ActionItem> {
    let mut items = vec![
        ActionItem::Header("─── [ PROFILES ] ───────────────────────".into()),
        ActionItem::ProfilePerformance,
        ActionItem::ProfileExtreme,
        ActionItem::ProfileAutoExtreme,
        ActionItem::AutoExtremeLevel,
        ActionItem::AutoBrightness,
        ActionItem::ProfileRestore,
        ActionItem::Header(String::new()),
    ];

    match os_name {
        "Linux" => {
            items.push(ActionItem::Header(
                "─── [ HARDWARE LIMITS ] ────────────────".into(),
            ));
            items.extend([
                ActionItem::Cores,
                ActionItem::FreqLimit,
                ActionItem::GpuFreq,
                ActionItem::RaplPl1,
                ActionItem::RaplPl2,
                ActionItem::Turbo,
                ActionItem::Epp,
                ActionItem::Aspm,
            ]);
            items.push(ActionItem::Header(String::new()));
            items.push(ActionItem::Header(
                "─── [ PERIPHERALS ] ────────────────────".into(),
            ));
            items.extend([
                ActionItem::Brightness,
                ActionItem::KbdBacklight,
                ActionItem::Bluetooth,
                ActionItem::WifiEnable,
            ]);
            items.push(ActionItem::Header(String::new()));
            items.push(ActionItem::Header(
                "─── [ SYSTEM TWEAKS ] ──────────────────".into(),
            ));
            items.extend([
                ActionItem::WifiPowerSave,
                ActionItem::AudioPowerSave,
                ActionItem::Autosuspend,
                ActionItem::Watchdog,
                ActionItem::VmWriteback,
                ActionItem::ProcessPurge,
            ]);
        }
        "macOS" => {
            items.push(ActionItem::Header(
                "─── [ PERIPHERALS & NETWORKING ] ──────".into(),
            ));
            items.extend([
                ActionItem::Brightness,
                ActionItem::WifiEnable,
                ActionItem::Bluetooth,
            ]);
            items.push(ActionItem::Header(String::new()));
            items.push(ActionItem::Header(
                "─── [ SYSTEM MEMORY ] ──────────────────".into(),
            ));
            items.push(ActionItem::ProcessPurge);
        }
        _ => {}
    }

    items
}

pub struct App {
    pub backend: LinuxBackend,
    pub config: Config,
    pub history: Vec<f64>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub items: Vec<ActionItem>,
    pub confirm_extreme: bool,
    pub toast: Option<(String, Instant)>,
    pub should_quit: bool,
    pub last_update: Instant,
    /// Graph sampling interval, adjustable with `+`/`-`.
    pub refresh_delay: Duration,
    /// Real daemon state, refreshed with a short TTL so the "Auto Extreme Mode" row
    /// reflects whether the daemon is actively running rather than `config.profile`.
    daemon_active: bool,
    daemon_checked: Instant,
}

impl App {
    pub fn new(backend: LinuxBackend, config: Config) -> Self {
        // Select the row set based on the host OS.
        let items = build_menu(backend.os_name());

        // Select the first non-header entry, skipping the PROFILES title.
        let initial_selected = items.iter().position(|i| !i.is_header()).unwrap_or(0);

        Self {
            backend,
            config,
            history: Vec::with_capacity(400),
            selected: initial_selected,
            scroll_offset: 0,
            items,
            confirm_extreme: false,
            toast: None,
            should_quit: false,
            last_update: Instant::now(),
            refresh_delay: DEFAULT_REFRESH_DELAY,
            daemon_active: false,
            daemon_checked: Instant::now() - DAEMON_STATE_TTL,
        }
    }

    pub fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    pub fn toast_message(&self) -> Option<&str> {
        if let Some((msg, time)) = &self.toast {
            if time.elapsed() < Duration::from_secs(3) {
                return Some(msg);
            }
        }
        None
    }

    pub fn is_daemon_active(&self) -> bool {
        self.daemon_active
    }

    fn refresh_daemon_state(&mut self) {
        if self.daemon_checked.elapsed() < DAEMON_STATE_TTL {
            return;
        }
        self.daemon_active = daemon_is_active();
        self.daemon_checked = Instant::now();
    }

    /// Stops the running daemon process, unconditionally without altering persisted configuration.
    /// Every manual parameter adjustment pairs with stopping the automated daemon loop.
    pub fn stop_daemon(&mut self) {
        let pid = PidManager::new();
        if let Some(p) = pid.read_pid() {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(p),
                nix::sys::signal::Signal::SIGTERM,
            );
        }
        pid.release();

        self.daemon_active = false;
        self.daemon_checked = Instant::now();
    }

    /// Stops the background daemon: sets `auto_extreme_enabled = false`, stops the
    /// running process, stops the systemd unit, and releases the PID lock.
    pub fn stop_background_daemon(&mut self) {
        self.config.auto_extreme_enabled = false;
        let _ = self.config.save(None);

        self.stop_daemon();

        #[cfg(target_os = "linux")]
        {
            let _ = Command::new("systemctl")
                .args(["stop", "wattwarden.service"])
                .status();
        }
        self.daemon_checked = Instant::now();
    }

    /// Starts the background daemon: persists `auto_extreme_enabled = true`, restarts
    /// the systemd unit if present, or spawns a detached daemon process.
    pub fn start_daemon(&mut self) {
        self.config.auto_extreme_enabled = true;
        let _ = self.config.save(None);

        #[cfg(target_os = "linux")]
        {
            if std::path::Path::new("/etc/systemd/system/wattwarden.service").exists() {
                let _ = Command::new("systemctl")
                    .args(["restart", "wattwarden.service"])
                    .status();
            }
        }

        self.daemon_active = daemon_is_active();
        if !self.daemon_active {
            // Detached daemon: the child process runs detached with standard streams redirected.
            if spawn::spawn_detached_daemon().is_ok() {
                self.daemon_active = true;
            }
        }
        self.daemon_checked = Instant::now();
    }

    pub fn tick(&mut self) {
        self.refresh_daemon_state();
        if self.last_update.elapsed() < self.refresh_delay {
            return;
        }
        self.last_update = Instant::now();
        if let Ok(watts) = self.backend.battery.consumption_watts() {
            if watts > 0.0 {
                self.history.push(watts);
                if self.history.len() > 400 {
                    self.history.remove(0);
                }
            }
        }
    }

    /// Increases graph sampling rate (minimum 500ms), toast the new interval.
    pub fn speed_up(&mut self) {
        if self.refresh_delay > MIN_REFRESH_DELAY {
            self.refresh_delay = (self.refresh_delay - REFRESH_STEP).max(MIN_REFRESH_DELAY);
            self.set_toast(format!(
                "Update Speed: {}",
                format_duration(self.refresh_delay)
            ));
        }
    }

    /// Decreases graph sampling rate (maximum 10s), toast the new interval.
    pub fn speed_down(&mut self) {
        if self.refresh_delay < MAX_REFRESH_DELAY {
            self.refresh_delay = (self.refresh_delay + REFRESH_STEP).min(MAX_REFRESH_DELAY);
            self.set_toast(format!(
                "Update Speed: {}",
                format_duration(self.refresh_delay)
            ));
        }
    }

    pub fn next_menu(&mut self) {
        let len = self.items.len();
        let mut next = (self.selected + 1) % len;
        while self.items[next].is_header() {
            next = (next + 1) % len;
        }
        self.selected = next;
    }

    pub fn prev_menu(&mut self) {
        let len = self.items.len();
        let mut prev = if self.selected == 0 {
            len - 1
        } else {
            self.selected - 1
        };
        while self.items[prev].is_header() {
            prev = if prev == 0 { len - 1 } else { prev - 1 };
        }
        self.selected = prev;
    }

    pub fn confirm_extreme_mode(&mut self) {
        self.confirm_extreme = false;
        // Confirming Extreme mode stops the background daemon before applying profile.
        self.stop_background_daemon();
        let _ = self.backend.apply_profile(&PowerProfile::Extreme);
        self.config.profile = Some(PowerProfile::Extreme);
        let _ = self.config.save(None);
        self.set_toast("EXTREME MODE ACTIVATED");
    }

    pub fn cancel_extreme_mode(&mut self) {
        self.confirm_extreme = false;
    }

    /// Restores the default system power profile.
    pub fn restore(&mut self, message: &str) {
        self.confirm_extreme = false;
        // Stop background daemon prior to reverting hardware limits.
        self.stop_background_daemon();
        let _ = self.backend.apply_profile(&PowerProfile::Normal);
        self.config.profile = Some(PowerProfile::Normal);
        let _ = self.config.save(None);
        self.set_toast(message);
    }

    pub fn handle_enter(&mut self) {
        let item = self.items[self.selected].clone();
        match item {
            ActionItem::ProfilePerformance => {
                // Stop background daemon prior to applying performance settings.
                self.stop_background_daemon();
                let _ = self.backend.apply_profile(&PowerProfile::Performance);
                self.config.profile = Some(PowerProfile::Performance);
                let _ = self.config.save(None);
                self.set_toast("PERFORMANCE MODE ACTIVATED");
            }
            ActionItem::ProfileExtreme => {
                self.confirm_extreme = true;
            }
            ActionItem::ProfileAutoExtreme => {
                self.refresh_daemon_state();
                if self.daemon_active {
                    // Stop background daemon without altering current hardware limits.
                    self.stop_background_daemon();
                    self.set_toast("AUTO EXTREME DAEMON STOPPED");
                } else {
                    // Start daemon loop in the background.
                    self.start_daemon();
                    self.set_toast("AUTO EXTREME RUNNING (BACKGROUND)");
                }
            }
            ActionItem::AutoExtremeLevel => {
                let next = self.config.auto_extreme_level.next();
                self.config.auto_extreme_level = next;
                let _ = self.config.save(None);
                self.set_toast(format!(
                    "AUTO EXTREME LEVEL: {}",
                    next.to_string().to_uppercase()
                ));
            }
            ActionItem::AutoBrightness => {
                self.toggle_boolean(&ActionItem::AutoBrightness);
            }
            ActionItem::ProfileRestore => {
                self.restore("RESTORE MODE ACTIVATED");
            }
            ActionItem::ProcessPurge => {
                let _ = self.backend.tweaks.process_purge();
                self.set_toast("PROCESSES PURGED");
            }
            // Non-actionable rows ignore Enter.
            _ => {}
        }
    }

    /// Reads the raw centisecond value displayed as "VM Writeback (s)".
    fn vm_writeback_centisecs(&self) -> i64 {
        self.backend.tweaks.vm_writeback_centisecs()
    }

    fn write_vm_writeback_centisecs(&self, value: i64) {
        let clamped = value.clamp(VM_WRITEBACK_MIN_CENTISECS, VM_WRITEBACK_MAX_CENTISECS);
        let _ = self.backend.tweaks.set_vm_writeback_centisecs(clamped);
    }

    fn toggle_boolean(&mut self, item: &ActionItem) {
        match item {
            ActionItem::AutoBrightness => {
                self.config.auto_brightness = !self.config.auto_brightness;
                let _ = self.config.save(None);
                self.set_toast(if self.config.auto_brightness {
                    "AUTO BRIGHTNESS: ON"
                } else {
                    "AUTO BRIGHTNESS: OFF"
                });
            }
            ActionItem::Turbo => {
                if let Ok(cur) = self.backend.cpu.turbo_enabled() {
                    self.stop_daemon();
                    let _ = self.backend.cpu.set_turbo_enabled(!cur);
                    self.set_toast(format!("TURBO BOOST: {}", if !cur { "ON" } else { "OFF" }));
                }
            }
            ActionItem::KbdBacklight => {
                if let Ok(cur) = self.backend.peripherals.kbd_backlight() {
                    self.stop_daemon();
                    let _ = self.backend.peripherals.set_kbd_backlight(!cur);
                    self.set_toast(format!(
                        "KEYBOARD LIGHT: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::Bluetooth => {
                if let Ok(cur) = self.backend.peripherals.bluetooth_enabled() {
                    self.stop_daemon();
                    let _ = self.backend.peripherals.set_bluetooth_enabled(!cur);
                    self.set_toast(format!("BLUETOOTH: {}", if !cur { "ON" } else { "OFF" }));
                }
            }
            ActionItem::WifiEnable => {
                if let Ok(cur) = self.backend.peripherals.wifi_enabled() {
                    self.stop_daemon();
                    let _ = self.backend.peripherals.set_wifi_enabled(!cur);
                    self.set_toast(format!("WIFI ENABLE: {}", if !cur { "ON" } else { "OFF" }));
                }
            }
            ActionItem::WifiPowerSave => {
                if let Ok(cur) = self.backend.tweaks.wifi_power_save() {
                    self.stop_daemon();
                    let _ = self.backend.tweaks.set_wifi_power_save(!cur);
                    self.set_toast(format!(
                        "WIFI POWER SAVE: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::AudioPowerSave => {
                if let Ok(cur) = self.backend.tweaks.audio_power_save() {
                    self.stop_daemon();
                    let _ = self.backend.tweaks.set_audio_power_save(!cur);
                    self.set_toast(format!(
                        "AUDIO POWER SAVE: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::Autosuspend => {
                if let Ok(cur) = self.backend.tweaks.autosuspend() {
                    self.stop_daemon();
                    let _ = self.backend.tweaks.set_autosuspend(!cur);
                    self.set_toast(format!(
                        "AUTOSUSPEND PCI/USB: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::Watchdog => {
                if let Ok(cur) = self.backend.tweaks.nmi_watchdog() {
                    self.stop_daemon();
                    let _ = self.backend.tweaks.set_nmi_watchdog(!cur);
                    self.set_toast(format!(
                        "WATCHDOG KERNEL: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            _ => {}
        }
    }

    pub fn handle_left(&mut self) {
        let item = self.items[self.selected].clone();
        if item.stops_daemon_on_adjust() {
            self.stop_daemon();
        }
        match item {
            ActionItem::AutoBrightness => self.toggle_boolean(&item),
            ActionItem::Cores => {
                if let Ok(cur) = self.backend.cpu.online_cores() {
                    let next = cur.saturating_sub(1).max(1);
                    let _ = self.backend.cpu.set_online_cores(next);
                    self.set_toast(format!(
                        "ACTIVE CORES: {} / {}",
                        next,
                        self.backend.cpu.num_cpus()
                    ));
                }
            }
            ActionItem::FreqLimit => {
                if let Ok(cur) = self.backend.cpu.freq_limit() {
                    let (min_b, _) = self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let next = cur.saturating_sub(100).max(min_b);
                    let _ = self.backend.cpu.set_freq_limit(next);
                    self.set_toast(format!("CPU FREQ: {} MHz", next));
                }
            }
            ActionItem::GpuFreq => {
                if let Some(gpu) = &self.backend.gpu {
                    if let Ok(cur) = gpu.gpu_freq() {
                        let (min_g, _) = gpu.gpu_bounds().unwrap_or((300, 1100));
                        let next = cur.saturating_sub(50).max(min_g);
                        let _ = gpu.set_gpu_freq(next);
                        self.set_toast(format!("FREQ IGPU: {} MHz", next));
                    }
                }
            }
            ActionItem::RaplPl1 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl1_watts() {
                        let (min_w, _) = rapl.rapl_bounds().unwrap_or((5, 115));
                        let next = cur.saturating_sub(2).max(min_w);
                        let _ = rapl.set_pl1_watts(next);
                        self.set_toast(format!("RAPL PL1: {} W", next));
                    }
                }
            }
            ActionItem::RaplPl2 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl2_watts() {
                        let (min_w, _) = rapl.rapl_bounds().unwrap_or((5, 115));
                        let next = cur.saturating_sub(2).max(min_w);
                        let _ = rapl.set_pl2_watts(next);
                        self.set_toast(format!("RAPL PL2: {} W", next));
                    }
                }
            }
            ActionItem::Turbo
            | ActionItem::KbdBacklight
            | ActionItem::Bluetooth
            | ActionItem::WifiEnable
            | ActionItem::WifiPowerSave
            | ActionItem::AudioPowerSave
            | ActionItem::Autosuspend
            | ActionItem::Watchdog => self.toggle_boolean(&item),
            ActionItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = cur.saturating_sub(5).max(1);
                        let _ = bl.set_brightness_percent(next);
                        if self.config.auto_brightness {
                            self.config.auto_brightness = false;
                            let _ = self.config.save(None);
                            self.set_toast("AUTO BRIGHTNESS: OFF");
                        }
                    }
                }
            }
            ActionItem::VmWriteback => {
                let cur = self.vm_writeback_centisecs();
                self.write_vm_writeback_centisecs(cur - 1);
                self.set_toast(format!("VM WRITEBACK: {}", self.vm_writeback_centisecs()));
            }
            // EPP and ASPM are read-only in the dashboard: never write hardware here.
            _ => {}
        }
    }

    pub fn handle_right(&mut self) {
        let item = self.items[self.selected].clone();
        if item.stops_daemon_on_adjust() {
            self.stop_daemon();
        }
        match item {
            ActionItem::AutoBrightness => self.toggle_boolean(&item),
            ActionItem::Cores => {
                if let Ok(cur) = self.backend.cpu.online_cores() {
                    let next = (cur + 1).min(self.backend.cpu.num_cpus());
                    let _ = self.backend.cpu.set_online_cores(next);
                    self.set_toast(format!(
                        "ACTIVE CORES: {} / {}",
                        next,
                        self.backend.cpu.num_cpus()
                    ));
                }
            }
            ActionItem::FreqLimit => {
                if let Ok(cur) = self.backend.cpu.freq_limit() {
                    let (_, max_b) = self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let next = (cur + 100).min(max_b);
                    let _ = self.backend.cpu.set_freq_limit(next);
                    self.set_toast(format!("CPU FREQ: {} MHz", next));
                }
            }
            ActionItem::GpuFreq => {
                if let Some(gpu) = &self.backend.gpu {
                    if let Ok(cur) = gpu.gpu_freq() {
                        let (_, max_g) = gpu.gpu_bounds().unwrap_or((300, 1100));
                        let next = (cur + 50).min(max_g);
                        let _ = gpu.set_gpu_freq(next);
                        self.set_toast(format!("FREQ IGPU: {} MHz", next));
                    }
                }
            }
            ActionItem::RaplPl1 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl1_watts() {
                        let (_, max_w) = rapl.rapl_bounds().unwrap_or((5, 115));
                        let next = (cur + 2).min(max_w);
                        let _ = rapl.set_pl1_watts(next);
                        self.set_toast(format!("RAPL PL1: {} W", next));
                    }
                }
            }
            ActionItem::RaplPl2 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl2_watts() {
                        let (_, max_w) = rapl.rapl_bounds().unwrap_or((5, 115));
                        let next = (cur + 2).min(max_w);
                        let _ = rapl.set_pl2_watts(next);
                        self.set_toast(format!("RAPL PL2: {} W", next));
                    }
                }
            }
            ActionItem::Turbo
            | ActionItem::KbdBacklight
            | ActionItem::Bluetooth
            | ActionItem::WifiEnable
            | ActionItem::WifiPowerSave
            | ActionItem::AudioPowerSave
            | ActionItem::Autosuspend
            | ActionItem::Watchdog => self.toggle_boolean(&item),
            ActionItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = (cur + 5).min(100);
                        let _ = bl.set_brightness_percent(next);
                        if self.config.auto_brightness {
                            self.config.auto_brightness = false;
                            let _ = self.config.save(None);
                            self.set_toast("AUTO BRIGHTNESS: OFF");
                        }
                    }
                }
            }
            ActionItem::VmWriteback => {
                let cur = self.vm_writeback_centisecs();
                self.write_vm_writeback_centisecs(cur + 1);
                self.set_toast(format!("VM WRITEBACK: {}", self.vm_writeback_centisecs()));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_item_is_header() {
        assert!(ActionItem::Header("TEST".into()).is_header());
        assert!(ActionItem::Header(String::new()).is_header());
        assert!(!ActionItem::ProfilePerformance.is_header());
        assert!(!ActionItem::ProfileExtreme.is_header());
        assert!(!ActionItem::AutoBrightness.is_header());
        assert!(!ActionItem::Cores.is_header());
        assert!(!ActionItem::Brightness.is_header());
    }

    #[test]
    fn format_duration_matches_expected_string_format() {
        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_duration(Duration::from_secs(1)), "1s");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(format_duration(Duration::from_secs(2)), "2s");
        assert_eq!(format_duration(Duration::from_secs(10)), "10s");
    }

    #[test]
    fn hardware_adjustments_stop_the_daemon() {
        // All hardware control rows pair with stopping the daemon.
        for item in [
            ActionItem::Cores,
            ActionItem::FreqLimit,
            ActionItem::GpuFreq,
            ActionItem::RaplPl1,
            ActionItem::RaplPl2,
            ActionItem::Turbo,
            ActionItem::KbdBacklight,
            ActionItem::Bluetooth,
            ActionItem::WifiEnable,
            ActionItem::WifiPowerSave,
            ActionItem::AudioPowerSave,
            ActionItem::Autosuspend,
            ActionItem::Watchdog,
            ActionItem::VmWriteback,
        ] {
            assert!(
                item.stops_daemon_on_adjust(),
                "{item:?} must stop the daemon before writing"
            );
        }

        // Brightness turns auto-brightness off instead; EPP/ASPM are read-only.
        for item in [
            ActionItem::Brightness,
            ActionItem::AutoBrightness,
            ActionItem::Epp,
            ActionItem::Aspm,
            ActionItem::ProcessPurge,
        ] {
            assert!(
                !item.stops_daemon_on_adjust(),
                "{item:?} must not stop the daemon"
            );
        }
    }

    /// The dashboard shows a consistent row set per OS with the exact section headers.
    #[test]
    fn golden_menu_matches_per_os() {
        use ActionItem::*;

        let h_profiles = Header("─── [ PROFILES ] ───────────────────────".into());
        let h_hw = Header("─── [ HARDWARE LIMITS ] ────────────────".into());
        let h_per = Header("─── [ PERIPHERALS ] ────────────────────".into());
        let h_per_net = Header("─── [ PERIPHERALS & NETWORKING ] ──────".into());
        let h_tweaks = Header("─── [ SYSTEM TWEAKS ] ──────────────────".into());
        let h_mem = Header("─── [ SYSTEM MEMORY ] ──────────────────".into());
        let blank = Header(String::new());

        // The PROFILES block is shared by every OS.
        let profiles = vec![
            h_profiles,
            ProfilePerformance,
            ProfileExtreme,
            ProfileAutoExtreme,
            AutoExtremeLevel,
            AutoBrightness,
            ProfileRestore,
            blank.clone(),
        ];

        let mut linux = profiles.clone();
        linux.extend([
            h_hw,
            Cores,
            FreqLimit,
            GpuFreq,
            RaplPl1,
            RaplPl2,
            Turbo,
            Epp,
            Aspm,
            blank.clone(),
            h_per,
            Brightness,
            KbdBacklight,
            Bluetooth,
            WifiEnable,
            blank.clone(),
            h_tweaks,
            WifiPowerSave,
            AudioPowerSave,
            Autosuspend,
            Watchdog,
            VmWriteback,
            ProcessPurge,
        ]);
        assert_eq!(build_menu("Linux"), linux);

        let mut macos = profiles.clone();
        macos.extend([
            h_per_net,
            Brightness,
            WifiEnable,
            Bluetooth,
            blank,
            h_mem,
            ProcessPurge,
        ]);
        assert_eq!(build_menu("macOS"), macos);

        // An unrecognised OS name adds no platform section.
        assert_eq!(build_menu("FreeBSD"), profiles);
    }
}
