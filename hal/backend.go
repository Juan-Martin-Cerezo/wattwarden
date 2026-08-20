package hal // Hardware Abstraction Layer package for cross-platform hardware controls

// Backend defines the common interface for all supported Operating Systems
// This guarantees maximum adaptability across Linux, Windows, Mac, or any interface.
type Backend interface {
	GetOS() string // Returns the name of the operating system
	GetNumCPUs() int // Returns the total number of logical CPUs available
	GetCores() int // Returns the current number of active online cores
	SetCores(n int) // Sets the number of active online cores
	GetFreqLimit() int // Returns the current CPU frequency limit in MHz
	SetFreqLimit(mhz int) // Sets the maximum CPU frequency limit in MHz
	GetBatteryPercentage() int // Returns the current battery charge percentage
	IsCharging() bool // Returns true if the system is currently connected to AC power
	GetBatteryTime() string // Returns the estimated remaining battery time as a string
	GetPowerConsumptionWatts() float64 // Returns the current power consumption in Watts
	GetRAPLPL1() int // Returns the long-term power limit (PL1) via RAPL in Watts
	SetRAPLPL1(watts int) // Sets the long-term power limit (PL1) via RAPL in Watts
	GetRAPLPL2() int // Returns the short-term power limit (PL2) via RAPL in Watts
	SetRAPLPL2(watts int) // Sets the short-term power limit (PL2) via RAPL in Watts
	GetTurbo() bool // Returns true if CPU Turbo Boost / Performance Boost is enabled
	SetTurbo(enabled bool) // Enables or disables CPU Turbo Boost / Performance Boost
	GetEPP() string // Returns the current Energy Performance Preference (e.g., power, performance)
	SetEPP(pref string) // Sets the Energy Performance Preference
	GetGPUFreq() int // Returns the current maximum GPU frequency in MHz
	SetGPUFreq(mhz int) // Sets the maximum GPU frequency in MHz
	GetASPM() string // Returns the current PCIe Active State Power Management policy
	SetASPM(policy string) // Sets the PCIe ASPM policy (e.g., powersave)
	GetWifiPowerSave() bool // Returns true if WiFi power management is enabled
	SetWifiPowerSave(enabled bool) // Enables or disables WiFi power management
	GetKbdBacklight() bool // Returns true if the keyboard backlight is currently on
	SetKbdBacklight(enabled bool) // Enables or disables the keyboard backlight
	GetAudioPowerSave() bool // Returns true if audio codec power saving is active
	SetAudioPowerSave(enabled bool) // Enables or disables audio codec power saving

	// Additional methods ported from the legacy Python script
	GetLCDBrightness() int // Returns the current display brightness as a percentage
	SetLCDBrightness(percent int) // Sets the display brightness to a specific percentage
	GetBluetooth() bool // Returns true if the Bluetooth adapter is enabled
	SetBluetooth(enabled bool) // Enables or disables the Bluetooth adapter via rfkill/OS
	GetWifiEnable() bool // Returns true if the WiFi adapter is enabled
	SetWifiEnable(enabled bool) // Enables or disables the WiFi adapter via rfkill/OS
	GetAutosuspend() bool // Returns true if USB/PCI autosuspend is enabled
	SetAutosuspend(enabled bool) // Enables or disables USB/PCI autosuspend
	GetWatchdog() bool // Returns true if the kernel NMI Watchdog is active
	SetWatchdog(enabled bool) // Enables or disables the kernel NMI Watchdog to save power
	GetVMWriteback() int // Returns the VM dirty writeback interval in centisecs
	SetVMWriteback(centisecs int) // Sets the VM dirty writeback interval in centisecs
	ProcessPurge() // Aggressively kills background processes to save battery (Extreme Mode feature)

	SetBrightnessTarget(target string) // Sets a specific brightness target config string
	SetRefreshRate(target string) // Sets the display refresh rate
	SetHyprEffects(enabled bool) // Toggles desktop compositor effects (like Hyprland animations)
	SetNMIWatchdog(enabled bool) // Legacy alias for setting NMI Watchdog
	SetVMDirty(writeback int, expire int) // Advanced configuration for VM dirty ratios

	GetAutoBrightness() bool // Returns true if auto-brightness is enabled during Auto Extreme Mode
	SetAutoBrightness(enabled bool) // Enables or disables auto-brightness in Auto Extreme Mode
	IsDaemonRunning() bool // Returns true if the background daemon is active
	ApplyModePerformance() // Applies a High Performance profile to all hardware components
	ApplyModeExtreme() // Applies the absolute minimum values to all hardware to maximize battery life
	ApplyModeRestore() // Restores the hardware to its default factory/OS state
	StartAutoExtremeDaemon() // Starts a background service that adapts the system to the user's load automatically
	StopDaemon() // Stops the background daemon
}

// CurrentBackend holds the loaded backend implementation for the current Operating System
var CurrentBackend Backend
