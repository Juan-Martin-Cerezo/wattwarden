use std::time::{Duration, Instant};
use wattwarden_core::*;
use wattwarden_platform_linux::LinuxBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Profile,
    Cores,
    FreqLimit,
    Brightness,
    ChargeLimit,
    Turbo,
    Epp,
    RaplPl1,
}

pub struct App {
    pub backend: LinuxBackend,
    pub config: Config,
    pub power_history: Vec<u64>, // Watts * 100 for integer sparkline
    pub selected_menu: usize,
    pub menu_items: Vec<MenuItem>,
    pub toast: Option<(String, Instant)>,
    pub should_quit: bool,
    pub last_update: Instant,
    pub update_interval: Duration,
}

impl App {
    pub fn new(backend: LinuxBackend, config: Config) -> Self {
        let mut menu_items = vec![
            MenuItem::Profile,
            MenuItem::Cores,
            MenuItem::FreqLimit,
            MenuItem::Brightness,
        ];

        if backend.threshold.supports_threshold() {
            menu_items.push(MenuItem::ChargeLimit);
        }
        if backend.cpu.turbo_enabled().is_ok() {
            menu_items.push(MenuItem::Turbo);
        }
        if backend.cpu.energy_performance_preference().is_ok() {
            menu_items.push(MenuItem::Epp);
        }
        if backend.rapl.is_some() {
            menu_items.push(MenuItem::RaplPl1);
        }

        Self {
            backend,
            config,
            power_history: Vec::with_capacity(60),
            selected_menu: 0,
            menu_items,
            toast: None,
            should_quit: false,
            last_update: Instant::now(),
            update_interval: Duration::from_millis(500),
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
            let scaled = (watts * 10.0).round() as u64;
            self.power_history.push(scaled);
            if self.power_history.len() > 60 {
                self.power_history.remove(0);
            }
        }
    }

    pub fn next_menu(&mut self) {
        if self.selected_menu + 1 < self.menu_items.len() {
            self.selected_menu += 1;
        } else {
            self.selected_menu = 0;
        }
    }

    pub fn prev_menu(&mut self) {
        if self.selected_menu > 0 {
            self.selected_menu -= 1;
        } else {
            self.selected_menu = self.menu_items.len().saturating_sub(1);
        }
    }

    pub fn handle_enter(&mut self) {
        let item = &self.menu_items[self.selected_menu];
        match item {
            MenuItem::Profile => {
                let next = match self.config.profile {
                    PowerProfile::Normal => PowerProfile::Performance,
                    PowerProfile::Performance => PowerProfile::Extreme,
                    PowerProfile::Extreme => PowerProfile::AutoExtreme,
                    PowerProfile::AutoExtreme => PowerProfile::Normal,
                };
                let _ = self.backend.apply_profile(&next);
                self.config.profile = next.clone();
                let _ = self.config.save(None);
                self.set_toast(format!("Applied profile: {}", next));
            }
            MenuItem::Turbo => {
                if let Ok(cur) = self.backend.cpu.turbo_enabled() {
                    let _ = self.backend.cpu.set_turbo_enabled(!cur);
                    self.set_toast(format!("Turbo Boost: {}", if !cur { "ENABLED" } else { "DISABLED" }));
                }
            }
            MenuItem::Epp => {
                if let Ok(cur) = self.backend.cpu.energy_performance_preference() {
                    let next = match cur.as_str() {
                        "performance" => "balance_performance",
                        "balance_performance" => "balance_power",
                        "balance_power" => "power",
                        _ => "performance",
                    };
                    let _ = self.backend.cpu.set_energy_performance_preference(next);
                    self.set_toast(format!("EPP set to: {}", next));
                }
            }
            _ => {}
        }
    }

    pub fn handle_left(&mut self) {
        let item = &self.menu_items[self.selected_menu];
        match item {
            MenuItem::Cores => {
                if let Ok(cur) = self.backend.cpu.online_cores() {
                    let next = cur.saturating_sub(1).max(1);
                    let _ = self.backend.cpu.set_online_cores(next);
                    self.set_toast(format!("Online cores: {}/{}", next, self.backend.cpu.num_cpus()));
                }
            }
            MenuItem::FreqLimit => {
                if let Ok(cur) = self.backend.cpu.freq_limit() {
                    let (min_b, _) = self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let next = cur.saturating_sub(200).max(min_b);
                    let _ = self.backend.cpu.set_freq_limit(next);
                    self.set_toast(format!("Max CPU Freq: {} MHz", next));
                }
            }
            MenuItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = cur.saturating_sub(5).max(1);
                        let _ = bl.set_brightness_percent(next);
                        self.set_toast(format!("Brightness: {}%", next));
                    }
                }
            }
            MenuItem::ChargeLimit => {
                if let Ok(cur) = self.backend.threshold.charge_threshold() {
                    let next = cur.saturating_sub(5).max(50);
                    let _ = self.backend.threshold.set_charge_threshold(next);
                    self.set_toast(format!("Battery charge ceiling: {}%", next));
                }
            }
            MenuItem::RaplPl1 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl1_watts() {
                        let next = cur.saturating_sub(5).max(5);
                        let _ = rapl.set_pl1_watts(next);
                        self.set_toast(format!("RAPL PL1 limit: {} W", next));
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_right(&mut self) {
        let item = &self.menu_items[self.selected_menu];
        match item {
            MenuItem::Cores => {
                if let Ok(cur) = self.backend.cpu.online_cores() {
                    let next = (cur + 1).min(self.backend.cpu.num_cpus());
                    let _ = self.backend.cpu.set_online_cores(next);
                    self.set_toast(format!("Online cores: {}/{}", next, self.backend.cpu.num_cpus()));
                }
            }
            MenuItem::FreqLimit => {
                if let Ok(cur) = self.backend.cpu.freq_limit() {
                    let (_, max_b) = self.backend.cpu.freq_bounds().unwrap_or((400, 3500));
                    let next = (cur + 200).min(max_b);
                    let _ = self.backend.cpu.set_freq_limit(next);
                    self.set_toast(format!("Max CPU Freq: {} MHz", next));
                }
            }
            MenuItem::Brightness => {
                if let Some(bl) = &self.backend.backlight {
                    if let Ok(cur) = bl.brightness_percent() {
                        let next = (cur + 5).min(100);
                        let _ = bl.set_brightness_percent(next);
                        self.set_toast(format!("Brightness: {}%", next));
                    }
                }
            }
            MenuItem::ChargeLimit => {
                if let Ok(cur) = self.backend.threshold.charge_threshold() {
                    let next = (cur + 5).min(100);
                    let _ = self.backend.threshold.set_charge_threshold(next);
                    self.set_toast(format!("Battery charge ceiling: {}%", next));
                }
            }
            MenuItem::RaplPl1 => {
                if let Some(rapl) = &self.backend.rapl {
                    if let Ok(cur) = rapl.pl1_watts() {
                        let (_, max_w) = rapl.rapl_bounds().unwrap_or((5, 115));
                        let next = (cur + 5).min(max_w);
                        let _ = rapl.set_pl1_watts(next);
                        self.set_toast(format!("RAPL PL1 limit: {} W", next));
                    }
                }
            }
            _ => {}
        }
    }
}
