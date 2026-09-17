use std::time::{Duration, Instant};
use wattwarden_core::*;
use wattwarden_platform_linux::LinuxBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionItem {
    Header(String),
    ProfilePerformance,
    ProfileExtreme,
    ProfileAutoExtreme,
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
    ChargeLimit,
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
}

impl App {
    pub fn new(backend: LinuxBackend, config: Config) -> Self {
        let mut items = vec![
            ActionItem::Header("─── [ PROFILES ] ───────────────────────".into()),
            ActionItem::ProfilePerformance,
            ActionItem::ProfileExtreme,
            ActionItem::ProfileAutoExtreme,
            ActionItem::AutoBrightness,
            ActionItem::ProfileRestore,
            ActionItem::Header("─── [ HARDWARE LIMITS ] ────────────────".into()),
            ActionItem::Cores,
            ActionItem::FreqLimit,
        ];

        if backend.gpu.is_some() {
            items.push(ActionItem::GpuFreq);
        }
        if backend.rapl.is_some() {
            items.push(ActionItem::RaplPl1);
            items.push(ActionItem::RaplPl2);
        }
        if backend.cpu.turbo_enabled().is_ok() {
            items.push(ActionItem::Turbo);
        }
        if backend.cpu.energy_performance_preference().is_ok() {
            items.push(ActionItem::Epp);
        }
        if backend.aspm.is_some() {
            items.push(ActionItem::Aspm);
        }

        items.push(ActionItem::Header(
            "─── [ PERIPHERALS ] ────────────────────".into(),
        ));
        if backend.backlight.is_some() {
            items.push(ActionItem::Brightness);
        }
        items.push(ActionItem::KbdBacklight);
        items.push(ActionItem::Bluetooth);
        items.push(ActionItem::WifiEnable);
        if backend.threshold.supports_threshold() {
            items.push(ActionItem::ChargeLimit);
        }

        items.push(ActionItem::Header(
            "─── [ SYSTEM TWEAKS ] ──────────────────".into(),
        ));
        items.push(ActionItem::WifiPowerSave);
        items.push(ActionItem::AudioPowerSave);
        items.push(ActionItem::Autosuspend);
        items.push(ActionItem::Watchdog);
        items.push(ActionItem::VmWriteback);
        items.push(ActionItem::ProcessPurge);

        // Find initial selectable index (skip first header)
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

    pub fn tick(&mut self) {
        if let Ok(watts) = self.backend.battery.consumption_watts() {
            if watts > 0.0 {
                self.history.push(watts);
                if self.history.len() > 400 {
                    self.history.remove(0);
                }
            }
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
        let _ = self.backend.apply_profile(&PowerProfile::Extreme);
        self.config.profile = PowerProfile::Extreme;
        self.config.auto_extreme_enabled = false;
        let _ = self.config.save(None);
        self.set_toast("EXTREME MODE ACTIVATED");
    }

    pub fn cancel_extreme_mode(&mut self) {
        self.confirm_extreme = false;
    }

    pub fn restore_defaults(&mut self) {
        self.confirm_extreme = false;
        let _ = self.backend.apply_profile(&PowerProfile::Normal);
        self.config.profile = PowerProfile::Normal;
        self.config.auto_extreme_enabled = false;
        let _ = self.config.save(None);
        self.set_toast("RESTORE MODE ACTIVATED");
    }

    pub fn handle_enter(&mut self) {
        let item = &self.items[self.selected];
        match item {
            ActionItem::ProfilePerformance => {
                let _ = self.backend.apply_profile(&PowerProfile::Performance);
                self.config.profile = PowerProfile::Performance;
                self.config.auto_extreme_enabled = false;
                let _ = self.config.save(None);
                self.set_toast("PERFORMANCE MODE ACTIVATED");
            }
            ActionItem::ProfileExtreme => {
                self.confirm_extreme = true;
            }
            ActionItem::ProfileAutoExtreme => {
                let is_active = self.config.profile == PowerProfile::AutoExtreme;
                if is_active {
                    let _ = self.backend.apply_profile(&PowerProfile::Normal);
                    self.config.profile = PowerProfile::Normal;
                    self.config.auto_extreme_enabled = false;
                    let _ = self.config.save(None);
                    self.set_toast("AUTO EXTREME DAEMON STOPPED");
                } else {
                    let _ = self.backend.apply_profile(&PowerProfile::AutoExtreme);
                    self.config.profile = PowerProfile::AutoExtreme;
                    self.config.auto_extreme_enabled = true;
                    let _ = self.config.save(None);
                    self.set_toast("AUTO EXTREME RUNNING (BACKGROUND)");
                }
            }
            ActionItem::AutoBrightness => {
                self.config.auto_brightness = !self.config.auto_brightness;
                let _ = self.config.save(None);
                self.set_toast(if self.config.auto_brightness {
                    "AUTO BRIGHTNESS: ON"
                } else {
                    "AUTO BRIGHTNESS: OFF"
                });
            }
            ActionItem::ProfileRestore => {
                self.restore_defaults();
            }
            ActionItem::Turbo => {
                if let Ok(cur) = self.backend.cpu.turbo_enabled() {
                    let _ = self.backend.cpu.set_turbo_enabled(!cur);
                    self.set_toast(format!("TURBO BOOST: {}", if !cur { "ON" } else { "OFF" }));
                }
            }
            ActionItem::Epp => {
                if let Ok(cur) = self.backend.cpu.energy_performance_preference() {
                    let next = match cur.as_str() {
                        "performance" => "balance_performance",
                        "balance_performance" => "balance_power",
                        "balance_power" => "power",
                        _ => "performance",
                    };
                    let _ = self.backend.cpu.set_energy_performance_preference(next);
                    self.set_toast(format!("ENERGY PERF PREF: {}", next));
                }
            }
            ActionItem::Aspm => {
                if let Some(aspm) = &self.backend.aspm {
                    if let Ok(cur) = aspm.aspm_policy() {
                        let next = match cur.as_str() {
                            "powersave" => "performance",
                            "performance" => "default",
                            _ => "powersave",
                        };
                        let _ = aspm.set_aspm_policy(next);
                        self.set_toast(format!("PCIE ASPM POLICY: {}", next));
                    }
                }
            }
            ActionItem::KbdBacklight => {
                if let Ok(cur) = self.backend.peripherals.kbd_backlight() {
                    let _ = self.backend.peripherals.set_kbd_backlight(!cur);
                    self.set_toast(format!(
                        "KEYBOARD LIGHT: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::Bluetooth => {
                if let Ok(cur) = self.backend.peripherals.bluetooth_enabled() {
                    let _ = self.backend.peripherals.set_bluetooth_enabled(!cur);
                    self.set_toast(format!("BLUETOOTH: {}", if !cur { "ON" } else { "OFF" }));
                }
            }
            ActionItem::WifiEnable => {
                if let Ok(cur) = self.backend.peripherals.wifi_enabled() {
                    let _ = self.backend.peripherals.set_wifi_enabled(!cur);
                    self.set_toast(format!("WIFI ENABLE: {}", if !cur { "ON" } else { "OFF" }));
                }
            }
            ActionItem::WifiPowerSave => {
                if let Ok(cur) = self.backend.tweaks.wifi_power_save() {
                    let _ = self.backend.tweaks.set_wifi_power_save(!cur);
                    self.set_toast(format!(
                        "WIFI POWER SAVE: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::AudioPowerSave => {
                if let Ok(cur) = self.backend.tweaks.audio_power_save() {
                    let _ = self.backend.tweaks.set_audio_power_save(!cur);
                    self.set_toast(format!(
                        "AUDIO POWER SAVE: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::Autosuspend => {
                if let Ok(cur) = self.backend.tweaks.autosuspend() {
                    let _ = self.backend.tweaks.set_autosuspend(!cur);
                    self.set_toast(format!(
                        "AUTOSUSPEND PCI/USB: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::Watchdog => {
                if let Ok(cur) = self.backend.tweaks.nmi_watchdog() {
                    let _ = self.backend.tweaks.set_nmi_watchdog(!cur);
                    self.set_toast(format!(
                        "WATCHDOG KERNEL: {}",
                        if !cur { "ON" } else { "OFF" }
                    ));
                }
            }
            ActionItem::ProcessPurge => {
                let _ = self.backend.tweaks.process_purge();
                self.set_toast("PROCESSES PURGED");
            }
            _ => {}
        }
    }

    pub fn handle_left(&mut self) {
        let item = &self.items[self.selected];
        match item {
            ActionItem::AutoBrightness => {
                self.handle_enter();
            }
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
            | ActionItem::Epp
            | ActionItem::Aspm
            | ActionItem::KbdBacklight
            | ActionItem::Bluetooth
            | ActionItem::WifiEnable
            | ActionItem::WifiPowerSave
            | ActionItem::AudioPowerSave
            | ActionItem::Autosuspend
            | ActionItem::Watchdog => {
                self.handle_enter();
            }
            ActionItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = cur.saturating_sub(5).max(1);
                        let _ = bl.set_brightness_percent(next);
                        if self.config.auto_brightness {
                            self.config.auto_brightness = false;
                            let _ = self.config.save(None);
                        }
                        self.set_toast(format!("LCD BRIGHTNESS: {}%", next));
                    }
                }
            }
            ActionItem::ChargeLimit => {
                if let Ok(cur) = self.backend.threshold.charge_threshold() {
                    let next = cur.saturating_sub(5).max(50);
                    let _ = self.backend.threshold.set_charge_threshold(next);
                    self.set_toast(format!("CHARGE LIMIT: {}%", next));
                }
            }
            ActionItem::VmWriteback => {
                if let Ok(cur) = self.backend.tweaks.vm_writeback_seconds() {
                    let next = cur.saturating_sub(1).max(1);
                    let _ = self.backend.tweaks.set_vm_writeback_seconds(next);
                    self.set_toast(format!("VM WRITEBACK: {} s", next));
                }
            }
            _ => {}
        }
    }

    pub fn handle_right(&mut self) {
        let item = &self.items[self.selected];
        match item {
            ActionItem::AutoBrightness => {
                self.handle_enter();
            }
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
            | ActionItem::Epp
            | ActionItem::Aspm
            | ActionItem::KbdBacklight
            | ActionItem::Bluetooth
            | ActionItem::WifiEnable
            | ActionItem::WifiPowerSave
            | ActionItem::AudioPowerSave
            | ActionItem::Autosuspend
            | ActionItem::Watchdog => {
                self.handle_enter();
            }
            ActionItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = (cur + 5).min(100);
                        let _ = bl.set_brightness_percent(next);
                        if self.config.auto_brightness {
                            self.config.auto_brightness = false;
                            let _ = self.config.save(None);
                        }
                        self.set_toast(format!("LCD BRIGHTNESS: {}%", next));
                    }
                }
            }
            ActionItem::ChargeLimit => {
                if let Ok(cur) = self.backend.threshold.charge_threshold() {
                    let next = (cur + 5).min(100);
                    let _ = self.backend.threshold.set_charge_threshold(next);
                    self.set_toast(format!("CHARGE LIMIT: {}%", next));
                }
            }
            ActionItem::VmWriteback => {
                if let Ok(cur) = self.backend.tweaks.vm_writeback_seconds() {
                    let next = (cur + 1).min(60);
                    let _ = self.backend.tweaks.set_vm_writeback_seconds(next);
                    self.set_toast(format!("VM WRITEBACK: {} s", next));
                }
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
        assert!(!ActionItem::ProfilePerformance.is_header());
        assert!(!ActionItem::ProfileExtreme.is_header());
        assert!(!ActionItem::AutoBrightness.is_header());
        assert!(!ActionItem::Cores.is_header());
        assert!(!ActionItem::Brightness.is_header());
    }
}
