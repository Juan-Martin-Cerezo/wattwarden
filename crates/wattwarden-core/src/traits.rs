use crate::error::Result;

/// Battery and power telemetry provider
pub trait PowerSource: Send + Sync {
    /// Current battery capacity percentage [0, 100]
    fn battery_percentage(&self) -> Result<u8>;

    /// Returns true if connected to AC power
    fn is_charging(&self) -> Result<bool>;

    /// Instantaneous power discharge rate in Watts
    fn consumption_watts(&self) -> Result<f64>;

    /// Formatted time remaining estimate (e.g. "4h 32m" or "Charging")
    fn time_remaining(&self) -> Result<String>;
}

/// CPU cores, frequency limits, and energy governors
pub trait CpuGovernor: Send + Sync {
    /// Total number of physical/logical CPUs
    fn num_cpus(&self) -> usize;

    /// Number of currently online active cores
    fn online_cores(&self) -> Result<usize>;

    /// Set the number of online active cores (CPU0 always stays online)
    fn set_online_cores(&self, count: usize) -> Result<()>;

    /// Minimum and maximum hardware frequency bounds in MHz
    fn freq_bounds(&self) -> Result<(u32, u32)>;

    /// Current frequency scaling limit in MHz
    fn freq_limit(&self) -> Result<u32>;

    /// Set maximum frequency scaling limit in MHz
    fn set_freq_limit(&self, mhz: u32) -> Result<()>;

    /// Query whether Turbo Boost is enabled
    fn turbo_enabled(&self) -> Result<bool>;

    /// Enable or disable Turbo Boost
    fn set_turbo_enabled(&self, enabled: bool) -> Result<()>;

    /// Energy Performance Preference (EPP)
    fn energy_performance_preference(&self) -> Result<String>;

    /// Set Energy Performance Preference (e.g. "performance", "balance_performance", "power")
    fn set_energy_performance_preference(&self, pref: &str) -> Result<()>;
}

/// Intel/AMD RAPL (Running Average Power Limit) package controls
pub trait RaplController: Send + Sync {
    /// Long-term (PL1) and Short-term (PL2) power limit bounds in Watts
    fn rapl_bounds(&self) -> Result<(u32, u32)>;

    /// Get PL1 long term power limit in Watts
    fn pl1_watts(&self) -> Result<u32>;

    /// Set PL1 long term power limit in Watts
    fn set_pl1_watts(&self, watts: u32) -> Result<()>;

    /// Get PL2 short term power limit in Watts
    fn pl2_watts(&self) -> Result<u32>;

    /// Set PL2 short term power limit in Watts
    fn set_pl2_watts(&self, watts: u32) -> Result<()>;
}

/// Display backlight brightness controller
pub trait DisplayManager: Send + Sync {
    /// Current brightness percentage [0, 100]
    fn brightness_percent(&self) -> Result<u8>;

    /// Set display brightness percentage [1, 100]
    fn set_brightness_percent(&self, percent: u8) -> Result<()>;
}

/// Battery Management System (BMS) charge threshold controller
pub trait ChargeThreshold: Send + Sync {
    /// Checks if the hardware firmware supports battery charge thresholds
    fn supports_threshold(&self) -> bool;

    /// Get current stop charge threshold percentage (e.g. 80)
    fn charge_threshold(&self) -> Result<u8>;

    /// Set stop charge threshold percentage [50, 100]
    fn set_charge_threshold(&self, threshold: u8) -> Result<()>;
}

/// Active desktop compositor focus tracker (Hyprland, Wayland, X11)
pub trait CompositorFocus: Send + Sync {
    /// Returns the window class of the current active window (e.g. "kitty", "google-chrome")
    fn active_window_class(&self) -> Option<String>;
}

/// C-State idle residency telemetry
#[derive(Debug, Clone, PartialEq)]
pub struct CStateInfo {
    pub name: String,
    pub time_microseconds: u64,
    pub usage_count: u64,
}

pub trait CStateTelemetry: Send + Sync {
    /// Returns idle C-states for a specific CPU core or the package average
    fn cstates(&self) -> Result<Vec<CStateInfo>>;
}

/// Integrated/Discrete GPU frequency scaling controller
pub trait GpuController: Send + Sync {
    /// Hardware minimum and maximum frequency boundaries in MHz
    fn gpu_bounds(&self) -> Result<(u32, u32)>;

    /// Current maximum GPU frequency in MHz
    fn gpu_freq(&self) -> Result<u32>;

    /// Set maximum GPU frequency in MHz
    fn set_gpu_freq(&self, mhz: u32) -> Result<()>;
}

/// PCIe Active State Power Management (ASPM) controller
pub trait AspmController: Send + Sync {
    /// Read the currently active ASPM policy (e.g. "powersave", "performance", "default")
    fn aspm_policy(&self) -> Result<String>;

    /// Set the PCIe ASPM policy
    fn set_aspm_policy(&self, policy: &str) -> Result<()>;
}

/// Hardware peripherals controller (keyboard backlight, Bluetooth, Wi-Fi radio)
pub trait PeripheralsController: Send + Sync {
    /// Read whether the keyboard backlight is illuminated
    fn kbd_backlight(&self) -> Result<bool>;

    /// Enable or disable the keyboard backlight
    fn set_kbd_backlight(&self, enabled: bool) -> Result<()>;

    /// Check if the Bluetooth radio is unblocked/active
    fn bluetooth_enabled(&self) -> Result<bool>;

    /// Enable or disable Bluetooth via rfkill
    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()>;

    /// Check if Wi-Fi radio is unblocked/active
    fn wifi_enabled(&self) -> Result<bool>;

    /// Enable or disable Wi-Fi radio via rfkill
    fn set_wifi_enabled(&self, enabled: bool) -> Result<()>;
}

/// System and kernel energy tweaks (audio power save, autosuspend, watchdog, writeback)
pub trait SystemTweaksController: Send + Sync {
    /// Check whether Wi-Fi link power management is enabled
    fn wifi_power_save(&self) -> Result<bool>;

    /// Enable or disable Wi-Fi link power management
    fn set_wifi_power_save(&self, enabled: bool) -> Result<()>;

    /// Check whether audio codec power saving is enabled
    fn audio_power_save(&self) -> Result<bool>;

    /// Enable or disable audio codec power saving
    fn set_audio_power_save(&self, enabled: bool) -> Result<()>;

    /// Check whether USB/PCI runtime autosuspend is active
    fn autosuspend(&self) -> Result<bool>;

    /// Enable or disable USB/PCI runtime autosuspend
    fn set_autosuspend(&self, enabled: bool) -> Result<()>;

    /// Check whether kernel NMI Watchdog is enabled
    fn nmi_watchdog(&self) -> Result<bool>;

    /// Enable or disable kernel NMI Watchdog
    fn set_nmi_watchdog(&self, enabled: bool) -> Result<()>;

    /// Get dirty VM writeback interval in seconds
    fn vm_writeback_seconds(&self) -> Result<u32>;

    /// Set dirty VM writeback interval in seconds
    fn set_vm_writeback_seconds(&self, seconds: u32) -> Result<()>;

    /// Drop filesystem caches from memory (drop_caches = 3)
    fn process_purge(&self) -> Result<()>;
}

