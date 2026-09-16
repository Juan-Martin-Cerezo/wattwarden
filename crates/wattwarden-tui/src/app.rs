use std::time::{Duration, Instant};
use wattwarden_core::*;
use wattwarden_platform_linux::{sysfs, LinuxBackend};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionItem {
    Header(String),
    ProfilePerformance,
    ProfileExtreme,
    ProfileAutoExtreme,
    ProfileRestore,
    Cores,
    FreqLimit,
    RaplPl1,
    RaplPl2,
    Turbo,
    Epp,
    Brightness,
    ChargeLimit,
    AutoBrightness,
    DropCaches,
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
    pub toast: Option<(String, Instant)>,
    pub should_quit: bool,
    pub last_update: Instant,
}

impl App {
    pub fn new(backend: LinuxBackend, config: Config) -> Self {
        let mut items = vec![
            ActionItem::Header("── MODES & AUTOMATION ──".into()),
            ActionItem::ProfilePerformance,
            ActionItem::ProfileExtreme,
            ActionItem::ProfileAutoExtreme,
            ActionItem::ProfileRestore,
            ActionItem::Header("── HARDWARE BOUNDARIES ──".into()),
            ActionItem::Cores,
            ActionItem::FreqLimit,
        ];

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
        if backend.backlight.is_some() {
            items.push(ActionItem::Brightness);
        }
        if backend.threshold.supports_threshold() {
            items.push(ActionItem::ChargeLimit);
        }

        items.push(ActionItem::Header("── POWER SAVING TOGGLES ──".into()));
        items.push(ActionItem::AutoBrightness);
        items.push(ActionItem::DropCaches);

        // Find initial selectable index (skip first header)
        let initial_selected = items.iter().position(|i| !i.is_header()).unwrap_or(0);

        Self {
            backend,
            config,
            history: Vec::with_capacity(400),
            selected: initial_selected,
            scroll_offset: 0,
            items,
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
        let mut prev = if self.selected == 0 { len - 1 } else { self.selected - 1 };
        while self.items[prev].is_header() {
            prev = if prev == 0 { len - 1 } else { prev - 1 };
        }
        self.selected = prev;
    }

    pub fn handle_enter(&mut self) {
        let item = &self.items[self.selected];
        match item {
            ActionItem::ProfilePerformance => {
                let _ = self.backend.apply_profile(&PowerProfile::Performance);
                self.config.profile = PowerProfile::Performance;
                self.config.auto_extreme_enabled = false;
                let _ = self.config.save(None);
                self.set_toast("Applied: High Performance Profile");
            }
            ActionItem::ProfileExtreme => {
                let _ = self.backend.apply_profile(&PowerProfile::Extreme);
                self.config.profile = PowerProfile::Extreme;
                self.config.auto_extreme_enabled = false;
                let _ = self.config.save(None);
                self.set_toast("Applied: Extreme Battery Saver");
            }
            ActionItem::ProfileAutoExtreme => {
                let _ = self.backend.apply_profile(&PowerProfile::AutoExtreme);
                self.config.profile = PowerProfile::AutoExtreme;
                self.config.auto_extreme_enabled = true;
                let _ = self.config.save(None);
                self.set_toast("Applied: Auto Extreme Mode (Adaptive)");
            }
            ActionItem::ProfileRestore => {
                let _ = self.backend.apply_profile(&PowerProfile::Normal);
                self.config.profile = PowerProfile::Normal;
                self.config.auto_extreme_enabled = false;
                let _ = self.config.save(None);
                self.set_toast("Restored: Factory OS Defaults");
            }
            ActionItem::Turbo => {
                if let Ok(cur) = self.backend.cpu.turbo_enabled() {
                    let _ = self.backend.cpu.set_turbo_enabled(!cur);
                    self.set_toast(format!("CPU Turbo: {}", if !cur { "ENABLED" } else { "DISABLED" }));
                }
            }
            ActionItem::AutoBrightness => {
                self.config.auto_brightness = !self.config.auto_brightness;
                let _ = self.config.save(None);
                self.set_toast(format!(
                    "Auto-Brightness (Hyprland): {}",
                    if self.config.auto_brightness { "ACTIVE" } else { "OFF" }
                ));
            }
            ActionItem::DropCaches => {
                let _ = sysfs::write_sysfs_string("/proc/sys/vm/drop_caches", "3");
                self.set_toast("Memory Purge: Filesystem caches dropped (RAM freed)");
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
                    self.set_toast(format!("EPP preference set to: {}", next));
                }
            }
            _ => {}
        }
    }

    pub fn handle_left(&mut self) {
        let item = &self.items[self.selected];
        match item {
            ActionItem::Cores => {
                if let Ok(cur) = self.backend.cpu.online_cores() {
                    let next = cur.saturating_sub(1).max(1);
                    let _ = self.backend.cpu.set_online_cores(next);
                    self.set_toast(format!("Online cores: {}/{}", next, self.backend.cpu.num_cpus()));
                }
            }
            ActionItem::FreqLimit => {
                if let Ok(cur) = self.backend.cpu.freq_limit() {
                    let (min_b, _) = self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let next = cur.saturating_sub(200).max(min_b);
                    let _ = self.backend.cpu.set_freq_limit(next);
                    self.set_toast(format!("Max CPU Freq: {} MHz", next));
                }
            }
            ActionItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = cur.saturating_sub(5).max(1);
                        let _ = bl.set_brightness_percent(next);
                        self.set_toast(format!("Brightness: {}%", next));
                    }
                }
            }
            ActionItem::ChargeLimit => {
                if let Ok(cur) = self.backend.threshold.charge_threshold() {
                    let next = cur.saturating_sub(5).max(50);
                    let _ = self.backend.threshold.set_charge_threshold(next);
                    self.set_toast(format!("Battery charge ceiling: {}%", next));
                }
            }
            ActionItem::RaplPl1 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl1_watts() {
                        let next = cur.saturating_sub(5).max(5);
                        let _ = rapl.set_pl1_watts(next);
                        self.set_toast(format!("RAPL PL1 Limit: {} W", next));
                    }
                }
            }
            ActionItem::RaplPl2 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl2_watts() {
                        let next = cur.saturating_sub(5).max(5);
                        let _ = rapl.set_pl2_watts(next);
                        self.set_toast(format!("RAPL PL2 Boost: {} W", next));
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_right(&mut self) {
        let item = &self.items[self.selected];
        match item {
            ActionItem::Cores => {
                if let Ok(cur) = self.backend.cpu.online_cores() {
                    let next = (cur + 1).min(self.backend.cpu.num_cpus());
                    let _ = self.backend.cpu.set_online_cores(next);
                    self.set_toast(format!("Online cores: {}/{}", next, self.backend.cpu.num_cpus()));
                }
            }
            ActionItem::FreqLimit => {
                if let Ok(cur) = self.backend.cpu.freq_limit() {
                    let (_, max_b) = self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let next = (cur + 200).min(max_b);
                    let _ = self.backend.cpu.set_freq_limit(next);
                    self.set_toast(format!("Max CPU Freq: {} MHz", next));
                }
            }
            ActionItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = (cur + 5).min(100);
                        let _ = bl.set_brightness_percent(next);
                        self.set_toast(format!("Brightness: {}%", next));
                    }
                }
            }
            ActionItem::ChargeLimit => {
                if let Ok(cur) = self.backend.threshold.charge_threshold() {
                    let next = (cur + 5).min(100);
                    let _ = self.backend.threshold.set_charge_threshold(next);
                    self.set_toast(format!("Battery charge ceiling: {}%", next));
                }
            }
            ActionItem::RaplPl1 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl1_watts() {
                        let (_, max_w) = rapl.rapl_bounds().unwrap_or((5, 115));
                        let next = (cur + 5).min(max_w);
                        let _ = rapl.set_pl1_watts(next);
                        self.set_toast(format!("RAPL PL1 Limit: {} W", next));
                    }
                }
            }
            ActionItem::RaplPl2 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl2_watts() {
                        let (_, max_w) = rapl.rapl_bounds().unwrap_or((5, 115));
                        let next = (cur + 5).min(max_w);
                        let _ = rapl.set_pl2_watts(next);
                        self.set_toast(format!("RAPL PL2 Boost: {} W", next));
                    }
                }
            }
            _ => {}
        }
    }
}
