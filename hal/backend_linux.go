//go:build linux
// +build linux

package hal // Hardware Abstraction Layer for Linux

import (
	"fmt" // Formatting library for string manipulation
	"os" // OS library to read and write files
	"os/exec" // Exec library to run shell commands
	"path/filepath" // Filepath library to find files via Glob
	"runtime" // Runtime library to check system info
	"strconv" // String conversion library to parse numbers
	"strings" // String manipulation library
	"sync" // Sync library to protect shared state
	"time" // Time library for delays
)

type LinuxBackend struct{} // Struct representing the Linux backend implementation

func init() { CurrentBackend = &LinuxBackend{} } // Automatically register this backend when compiled for Linux

// runCmd executes a shell command and returns its output as a string
func runCmd(cmd string) string {
	out, _ := exec.Command("sh", "-c", cmd).Output() // Execute the command using sh -c
	return strings.TrimSpace(string(out)) // Return the output without trailing spaces or newlines
}

// readSys reads a file completely (usually from sysfs) and returns its content
func readSys(path string) string {
	d, _ := os.ReadFile(path) // Read the entire file into memory
	return strings.TrimSpace(string(d)) // Strip newlines from the read string
}

// writeSys writes a string value directly to a file path
func writeSys(path string, val string) {
	os.WriteFile(path, []byte(val), 0644) // Write the byte representation of the string with 644 permissions
}

func (b *LinuxBackend) GetOS() string { return "Linux" } // Return the OS identifier

// GetNumCPUs counts the total number of physical/logical CPUs using sysfs
func (b *LinuxBackend) GetNumCPUs() int {
	fs, _ := filepath.Glob("/sys/devices/system/cpu/cpu[0-9]*") // Find all cpu directories
	if len(fs) > 0 { // If directories are found
		return len(fs) // Return the count
	}
	return runtime.NumCPU() // Fallback to Go runtime count
}

// GetCores returns the number of currently active online cores
func (b *LinuxBackend) GetCores() int {
	cores := 1 // Assume at least CPU 0 is online
	for i := 1; i < b.GetNumCPUs(); i++ { // Iterate over all other CPUs
		if readSys(fmt.Sprintf("/sys/devices/system/cpu/cpu%d/online", i)) == "1" { // Check if online file reads 1
			cores++ // Increment online core count
		}
	}
	return cores // Return total online cores
}

// SetCores turns cores on or off based on the requested number
func (b *LinuxBackend) SetCores(n int) {
	if n < 1 { n = 1 } // Ensure at least 1 core is active
	if n > b.GetNumCPUs() { n = b.GetNumCPUs() } // Prevent exceeding max cores
	for i := 1; i < b.GetNumCPUs(); i++ { // CPU0 cannot be turned off, so start at 1
		val := "0" // Default to off
		if i < n { val = "1" } // If within requested limit, turn on
		writeSys(fmt.Sprintf("/sys/devices/system/cpu/cpu%d/online", i), val) // Write the status
	}
	
	mhz := b.GetFreqLimit() // Fetch the current frequency limit
	if mhz > 0 { // If a limit is set
		b.SetFreqLimit(mhz) // Re-apply the limit to ensure newly awakened cores get the limit
	}
}

// GetCPUFreqBounds reads the absolute hardware min and max frequencies allowed
func (b *LinuxBackend) GetCPUFreqBounds() (int, int) {
	minVal := readSys("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq") // Read hardware min
	maxVal := readSys("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq") // Read hardware max
	minMhz, maxMhz := 400, 1600 // Safe fallback limits
	if v, err := strconv.Atoi(minVal); err == nil { minMhz = v / 1000 } // Convert kHz to MHz
	if v, err := strconv.Atoi(maxVal); err == nil { maxMhz = v / 1000 } // Convert kHz to MHz
	return minMhz, maxMhz // Return boundaries
}

// GetFreqLimit returns the user-defined max frequency limit
func (b *LinuxBackend) GetFreqLimit() int {
	v, _ := strconv.Atoi(readSys("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")) // Read limit in kHz
	if v == 0 { return 0 } // Return 0 if failed
	return v / 1000 // Return in MHz
}

// SetFreqLimit applies a maximum CPU frequency limit across all cores
func (b *LinuxBackend) SetFreqLimit(mhz int) {
	minMhz, maxMhz := b.GetCPUFreqBounds() // Check boundaries
	if mhz < minMhz { mhz = minMhz } // Clamp minimum
	if mhz > maxMhz { mhz = maxMhz } // Clamp maximum
	
	khz := strconv.Itoa(mhz * 1000) // Convert MHz to kHz string
	fs, _ := filepath.Glob("/sys/devices/system/cpu/cpu*/cpufreq") // Find cpufreq folder for all CPUs
	for _, f := range fs { // For each core
		writeSys(filepath.Join(f, "scaling_min_freq"), strconv.Itoa(minMhz*1000)) // Enforce absolute minimum
		writeSys(filepath.Join(f, "scaling_max_freq"), khz) // Enforce user-defined maximum
	}
}

// GetBatteryPercentage returns the current battery level
func (b *LinuxBackend) GetBatteryPercentage() int {
	v, _ := strconv.Atoi(readSys("/sys/class/power_supply/BAT0/capacity")) // Read 0-100 percentage
	return v // Return it
}

// IsCharging checks if the device is plugged in
func (b *LinuxBackend) IsCharging() bool {
	status := readSys("/sys/class/power_supply/BAT0/status") // Read charging state
	return status == "Charging" || status == "Full" || status == "Not charging" // Return true if plugged in
}

// GetBatteryTime estimates remaining time or time to full
func (b *LinuxBackend) GetBatteryTime() string {
	if b.IsCharging() { // If on AC power
		return "Charging" // Simple string
	}
	
	energyStr := readSys("/sys/class/power_supply/BAT0/energy_now") // Attempt to read energy in micro-watt-hours
	powerStr := readSys("/sys/class/power_supply/BAT0/power_now") // Attempt to read power in micro-watts
	
	if energyStr == "" || powerStr == "" { // If hardware reports in amps/charge instead
		energyStr = readSys("/sys/class/power_supply/BAT0/charge_now") // Read micro-amp-hours
		powerStr = readSys("/sys/class/power_supply/BAT0/current_now") // Read micro-amps
		voltageStr := readSys("/sys/class/power_supply/BAT0/voltage_now") // Read micro-volts
		
		if energyStr != "" && powerStr != "" && voltageStr != "" { // If all are available
			e, _ := strconv.ParseFloat(energyStr, 64) // Convert to float
			c, _ := strconv.ParseFloat(powerStr, 64) // Convert to float
			v, _ := strconv.ParseFloat(voltageStr, 64) // Convert to float
			
			energy := e * (v / 1000000.0) // Calculate true energy
			power := c * (v / 1000000.0) // Calculate true power
			
			if power > 0 { // If discharging
				hours := energy / power // Calculate hours remaining
				h := int(hours) // Extract full hours
				m := int((hours - float64(h)) * 60) // Extract remaining minutes
				return fmt.Sprintf("%dh %02dm", h, m) // Return formatted string
			}
		}
		return "Calculating..." // Fallback
	}
	
	energy, _ := strconv.ParseFloat(energyStr, 64) // Convert energy to float
	power, _ := strconv.ParseFloat(powerStr, 64) // Convert power to float
	
	if power > 0 { // If discharging
		hours := energy / power // Calculate hours remaining
		h := int(hours) // Extract hours
		m := int((hours - float64(h)) * 60) // Extract minutes
		return fmt.Sprintf("%dh %02dm", h, m) // Format result
	}
	
	return "Calculating..." // Fallback
}

// GetPowerConsumptionWatts returns current discharge rate in Watts
func (b *LinuxBackend) GetPowerConsumptionWatts() float64 {
	c, _ := strconv.Atoi(readSys("/sys/class/power_supply/BAT0/current_now")) // Read current
	v, _ := strconv.Atoi(readSys("/sys/class/power_supply/BAT0/voltage_now")) // Read voltage
	return float64(c) * float64(v) / 1000000000000.0 // Calculate and convert to Watts
}


// GetRAPLBounds reads hardware limits for Intel RAPL package power
func (b *LinuxBackend) GetRAPLBounds() (int, int) {
	minW, maxW := 5, 115 // Fallbacks
	base := "/sys/class/powercap/intel-rapl:0" // RAPL package 0 path
	if minStr := readSys(base + "/min_power_range_uw"); minStr != "" { // Check minimum
		if v, err := strconv.Atoi(minStr); err == nil { minW = v / 1000000 } // Convert to Watts
	}
	if maxStr := readSys(base + "/max_power_range_uw"); maxStr != "" { // Check maximum
		if v, err := strconv.Atoi(maxStr); err == nil { maxW = v / 1000000 } // Convert to Watts
	}
	return minW, maxW // Return safe bounds
}

// getRAPLPath finds the correct RAPL constraint path by its string name
func (b *LinuxBackend) getRAPLPath(name string) string {
	for i := 0; i < 5; i++ { // Iterate through possible constraints
		p := fmt.Sprintf("/sys/class/powercap/intel-rapl:0/constraint_%d_name", i) // Path to name
		if readSys(p) == name { // If matches what we want
			return fmt.Sprintf("/sys/class/powercap/intel-rapl:0/constraint_%d_power_limit_uw", i) // Return limit path
		}
	}
	return "" // Not found
}

// GetRAPLPL1 gets the long-term (PL1) power limit
func (b *LinuxBackend) GetRAPLPL1() int {
	path := b.getRAPLPath("long_term") // Look for PL1
	if path != "" {
		v, _ := strconv.Atoi(readSys(path)) // Read value
		return v / 1000000 // Return in Watts
	}
	return 0 // Failed
}

// SetRAPLPL1 sets the long-term (PL1) power limit
func (b *LinuxBackend) SetRAPLPL1(w int) {
	minW, maxW := b.GetRAPLBounds() // Check boundaries
	if w < minW { w = minW } // Enforce min
	if w > maxW { w = maxW } // Enforce max
	path := b.getRAPLPath("long_term") // Find path
	if path != "" {
		writeSys(path, strconv.Itoa(w*1000000)) // Apply in micro-watts
	}
}

// GetRAPLPL2 gets the short-term (PL2) boost limit
func (b *LinuxBackend) GetRAPLPL2() int {
	path := b.getRAPLPath("short_term") // Look for PL2
	if path != "" {
		v, _ := strconv.Atoi(readSys(path)) // Read value
		return v / 1000000 // Return in Watts
	}
	return 0 // Failed
}

// SetRAPLPL2 sets the short-term (PL2) boost limit
func (b *LinuxBackend) SetRAPLPL2(w int) {
	minW, maxW := b.GetRAPLBounds() // Check boundaries
	if w < minW { w = minW } // Enforce min
	if w > maxW { w = maxW } // Enforce max
	path := b.getRAPLPath("short_term") // Find path
	if path != "" {
		writeSys(path, strconv.Itoa(w*1000000)) // Apply in micro-watts
	}
}

// GetTurbo checks if CPU boost is enabled
func (b *LinuxBackend) GetTurbo() bool {
	if _, err := os.Stat("/sys/devices/system/cpu/intel_pstate/no_turbo"); err == nil { // Intel systems
		return readSys("/sys/devices/system/cpu/intel_pstate/no_turbo") == "0" // 0 means turbo is ON
	}
	if _, err := os.Stat("/sys/devices/system/cpu/cpufreq/boost"); err == nil { // AMD systems
		return readSys("/sys/devices/system/cpu/cpufreq/boost") == "1" // 1 means turbo is ON
	}
	return true // Assume on if unknown
}

// SetTurbo enables or disables CPU boost
func (b *LinuxBackend) SetTurbo(e bool) {
	if _, err := os.Stat("/sys/devices/system/cpu/intel_pstate/no_turbo"); err == nil { // Intel systems
		val := "1" // 1 disables turbo
		if e { val = "0" } // 0 enables turbo
		writeSys("/sys/devices/system/cpu/intel_pstate/no_turbo", val) // Write policy
	} else if _, err := os.Stat("/sys/devices/system/cpu/cpufreq/boost"); err == nil { // AMD systems
		val := "0" // 0 disables turbo
		if e { val = "1" } // 1 enables turbo
		writeSys("/sys/devices/system/cpu/cpufreq/boost", val) // Write policy
	}
}

// GetEPP returns Energy Performance Preference
func (b *LinuxBackend) GetEPP() string {
	return readSys("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference") // Read EPP
}

// SetEPP sets the Energy Performance Preference
func (b *LinuxBackend) SetEPP(p string) {
	fs, _ := filepath.Glob("/sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference") // Find EPP for all cores
	for _, f := range fs { // Apply to all
		writeSys(f, p) // Write preference
	}
	
	gov := "powersave" // Default fallback governor
	if p == "performance" { gov = "performance" } // If requesting high perf, use performance gov
	
	fsGov, _ := filepath.Glob("/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor") // Find governor for all cores
	for _, f := range fsGov { // Apply to all
		writeSys(f, gov) // Write governor
	}
}

// getGPUPath finds the correct sysfs path for Intel/AMD GPU
func (b *LinuxBackend) getGPUPath() string {
	if _, err := os.Stat("/sys/class/drm/card1/gt_max_freq_mhz"); err == nil { return "/sys/class/drm/card1" } // Usually dedicated or hybrid
	if _, err := os.Stat("/sys/class/drm/card0/gt_max_freq_mhz"); err == nil { return "/sys/class/drm/card0" } // Usually integrated
	return "" // Not found
}

// GetGPUBounds returns hardware min and max GPU frequency in MHz
func (b *LinuxBackend) GetGPUBounds() (int, int) {
	path := b.getGPUPath() // Get path
	if path == "" { return 300, 1100 } // Fallback
	minMhz, maxMhz := 300, 1100 // Fallbacks
	
	minStr := readSys(path + "/gt_RPn_freq_mhz") // Hardware min limit
	if minStr == "" { minStr = readSys(path + "/gt_min_freq_mhz") } // Fallback software min
	if v, err := strconv.Atoi(minStr); err == nil { minMhz = v } // Convert to int
	
	maxStr := readSys(path + "/gt_RP0_freq_mhz") // Hardware max limit
	if maxStr == "" { maxStr = readSys(path + "/gt_max_freq_mhz") } // Fallback software max
	if v, err := strconv.Atoi(maxStr); err == nil { maxMhz = v } // Convert to int
	
	return minMhz, maxMhz // Return boundaries
}

// GetGPUFreq returns the current max frequency for GPU
func (b *LinuxBackend) GetGPUFreq() int {
	path := b.getGPUPath() // Get path
	if path == "" { return 0 } // Fallback
	v, _ := strconv.Atoi(readSys(path + "/gt_max_freq_mhz")) // Read limit
	return v // Return MHz
}

// SetGPUFreq sets a maximum GPU frequency limit
func (b *LinuxBackend) SetGPUFreq(mhz int) {
	path := b.getGPUPath() // Get path
	if path == "" { return } // Stop if missing
	minMhz, maxMhz := b.GetGPUBounds() // Get bounds
	if mhz < minMhz { mhz = minMhz } // Clamp minimum
	if mhz > maxMhz { mhz = maxMhz } // Clamp maximum
	writeSys(path+"/gt_min_freq_mhz", strconv.Itoa(minMhz)) // Ensure software min isn't breached
	writeSys(path+"/gt_max_freq_mhz", strconv.Itoa(mhz)) // Apply limit
}

// GetASPM gets the Active State Power Management policy for PCIe
func (b *LinuxBackend) GetASPM() string {
	raw := readSys("/sys/module/pcie_aspm/parameters/policy") // Read all options (active one is in brackets)
	for _, word := range strings.Fields(raw) { // Split string
		if strings.HasPrefix(word, "[") && strings.HasSuffix(word, "]") { // Check for brackets
			return word[1 : len(word)-1] // Extract active policy
		}
	}
	return raw // Return raw if unparsed
}

// SetASPM sets the Active State Power Management policy for PCIe
func (b *LinuxBackend) SetASPM(p string) {
	writeSys("/sys/module/pcie_aspm/parameters/policy", p) // Write policy (e.g. powersave)
}


// GetWifiPowerSave checks if Wi-Fi power saving is on
func (b *LinuxBackend) GetWifiPowerSave() bool {
	ifaces := strings.Split(runCmd("iw dev | awk '$1==\"Interface\"{print $2}'"), "\n") // List all interfaces
	for _, iface := range ifaces {
		iface = strings.TrimSpace(iface) // Clean string
		if iface != "" && strings.Contains(runCmd(fmt.Sprintf("iw dev %s get power_save", iface)), "on") { // Check state
			return true // Found at least one with power save on
		}
	}
	return readSys("/sys/module/iwlwifi/parameters/power_save") == "Y" // Fallback to Intel driver parameter
}

// SetWifiPowerSave toggles Wi-Fi power saving on all interfaces
func (b *LinuxBackend) SetWifiPowerSave(e bool) {
	val := "N" // N for off
	if e { val = "Y" } // Y for on
	writeSys("/sys/module/iwlwifi/parameters/power_save", val) // Try Intel driver
	
	iwState := "off" // Default command parameter
	if e { iwState = "on" } // Command parameter for on
	ifaces := strings.Split(runCmd("iw dev | awk '$1==\"Interface\"{print $2}'"), "\n") // List all interfaces
	for _, iface := range ifaces {
		iface = strings.TrimSpace(iface) // Clean string
		if iface != "" {
			runCmd(fmt.Sprintf("iw dev %s set power_save %s", iface, iwState)) // Set state
		}
	}
}

// GetKbdBacklight returns true if keyboard backlight is on
func (b *LinuxBackend) GetKbdBacklight() bool {
	fs, _ := filepath.Glob("/sys/class/leds/*kbd_backlight/brightness") // Find keyboard backlight devices
	for _, f := range fs {
		return readSys(f) != "0" // Any non-zero brightness means it is on
	}
	return false // Assume off if no device found
}

// SetKbdBacklight turns keyboard backlight completely off or restores to full brightness
func (b *LinuxBackend) SetKbdBacklight(e bool) {
	fs, _ := filepath.Glob("/sys/class/leds/*kbd_backlight") // Find devices
	for _, f := range fs {
		if e {
			max := readSys(f + "/max_brightness") // Read highest allowed value
			writeSys(f+"/brightness", max) // Turn up to max
		} else {
			writeSys(f+"/brightness", "0") // Turn off
		}
	}
}

// GetAudioPowerSave checks if HDA Intel audio power saving is active
func (b *LinuxBackend) GetAudioPowerSave() bool {
	return readSys("/sys/module/snd_hda_intel/parameters/power_save") != "0" // 0 means disabled
}

// SetAudioPowerSave toggles audio power saving and link power down
func (b *LinuxBackend) SetAudioPowerSave(e bool) {
	val := "0" // Off
	if e { val = "1" } // On (1 second timeout usually)
	writeSys("/sys/module/snd_hda_intel/parameters/power_save", val)
	val = "N" // Off
	if e { val = "Y" } // On
	writeSys("/sys/module/snd_hda_intel/parameters/power_save_controller", val)
}

// GetLCDBrightness returns current screen brightness percentage
func (b *LinuxBackend) GetLCDBrightness() int {
	fs, _ := filepath.Glob("/sys/class/backlight/*") // Find backlights
	for _, f := range fs {
		cur, _ := strconv.Atoi(readSys(f + "/brightness")) // Read current brightness
		max, _ := strconv.Atoi(readSys(f + "/max_brightness")) // Read max brightness
		if max > 0 {
			return (cur * 100) / max // Return percentage
		}
	}
	return 100 // Fallback
}

// SetLCDBrightness sets screen brightness to a specific percentage
func (b *LinuxBackend) SetLCDBrightness(percent int) {
	if percent < 5 { percent = 5 } // Ensure screen doesn't completely turn off and become unusable
	if percent > 100 { percent = 100 } // Clamp to max 100%
	fs, _ := filepath.Glob("/sys/class/backlight/*") // Find backlights
	for _, f := range fs {
		max, _ := strconv.Atoi(readSys(f + "/max_brightness")) // Read max
		if max > 0 {
			target := (percent * max) / 100 // Calculate absolute value based on percentage
			writeSys(f+"/brightness", strconv.Itoa(target)) // Apply
		}
	}
}

// GetBluetooth checks if Bluetooth is enabled via rfkill
func (b *LinuxBackend) GetBluetooth() bool {
	return !strings.Contains(runCmd("rfkill list bluetooth"), "Soft blocked: yes") // If soft blocked, it's off
}

// SetBluetooth enables or disables Bluetooth using rfkill
func (b *LinuxBackend) SetBluetooth(enabled bool) {
	if enabled {
		runCmd("rfkill unblock bluetooth") // Turn on
	} else {
		runCmd("rfkill block bluetooth") // Turn off
	}
}

// GetWifiEnable checks if Wi-Fi is enabled via rfkill
func (b *LinuxBackend) GetWifiEnable() bool {
	return !strings.Contains(runCmd("rfkill list wifi"), "Soft blocked: yes") // If soft blocked, it's off
}

// SetWifiEnable enables or disables Wi-Fi using rfkill
func (b *LinuxBackend) SetWifiEnable(enabled bool) {
	if enabled {
		runCmd("rfkill unblock wifi") // Turn on
	} else {
		runCmd("rfkill block wifi") // Turn off
	}
}

// GetAutosuspend checks if USB/PCI autosuspend rules are active
func (b *LinuxBackend) GetAutosuspend() bool {
	// A basic check to see if at least one device is auto-suspended
	fs, _ := filepath.Glob("/sys/bus/usb/devices/*/power/control")
	for _, f := range fs {
		if readSys(f) == "auto" { // "auto" means autosuspend is active
			return true
		}
	}
	return false // If none found or all "on"
}

// SetAutosuspend forces autosuspend on or off for all USB and PCI devices
func (b *LinuxBackend) SetAutosuspend(enabled bool) {
	val := "on" // "on" means DEVICE is ON, meaning autosuspend is OFF
	if enabled { val = "auto" } // "auto" means the kernel can suspend the device

	fs, _ := filepath.Glob("/sys/bus/usb/devices/*/power/control") // Find all USB devices
	for _, f := range fs {
		writeSys(f, val) // Apply
	}
	
	fsPCI, _ := filepath.Glob("/sys/bus/pci/devices/*/power/control") // Find all PCI devices
	for _, f := range fsPCI {
		writeSys(f, val) // Apply
	}
}

// GetWatchdog checks if NMI Watchdog (system panic handler) is enabled
func (b *LinuxBackend) GetWatchdog() bool {
	return readSys("/proc/sys/kernel/nmi_watchdog") == "1" // 1 means on
}

// SetWatchdog enables or disables NMI Watchdog (disabling saves some power)
func (b *LinuxBackend) SetWatchdog(enabled bool) {
	val := "0" // Off
	if enabled { val = "1" } // On
	writeSys("/proc/sys/kernel/nmi_watchdog", val)
}

// GetVMWriteback gets the current virtual memory dirty writeback time in centiseconds
func (b *LinuxBackend) GetVMWriteback() int {
	v, _ := strconv.Atoi(readSys("/proc/sys/vm/dirty_writeback_centisecs")) // Read centiseconds
	return v
}

// SetVMWriteback sets how long before dirty memory is flushed to disk (higher saves power)
func (b *LinuxBackend) SetVMWriteback(centisecs int) {
	if centisecs < 100 { centisecs = 100 } // Minimum 1 second
	if centisecs > 6000 { centisecs = 6000 } // Maximum 60 seconds
	writeSys("/proc/sys/vm/dirty_writeback_centisecs", strconv.Itoa(centisecs))
}

// ProcessPurge drops filesystem caches to free up RAM
func (b *LinuxBackend) ProcessPurge() {
	writeSys("/proc/sys/vm/drop_caches", "3") // "3" clears pagecache, dentries, and inodes
}

// ApplyModePerformance sets the system for maximum processing power at the cost of battery
func (b *LinuxBackend) ApplyModePerformance() {
	b.SetCores(b.GetNumCPUs()) // Turn on all cores
	b.SetFreqLimit(99999) // Remove frequency limit
	b.SetRAPLPL1(115) // Maximize package power
	b.SetRAPLPL2(115) // Maximize boost power
	b.SetTurbo(true) // Enable boost
	b.SetEPP("performance") // Tell kernel to prioritize speed
	b.SetGPUFreq(99999) // Remove GPU limits
	b.SetASPM("performance") // Disable PCIe power saving
	b.SetWifiPowerSave(false) // Disable Wi-Fi power save
	b.SetAudioPowerSave(false) // Disable audio power save
	b.SetAutosuspend(false) // Keep devices awake
	b.SetWatchdog(true) // Enable watchdog
	b.SetVMWriteback(500) // Default flush time (5 seconds)
}

// ApplyModeExtreme aggressively saves battery by turning almost everything off or to lowest settings
func (b *LinuxBackend) ApplyModeExtreme() {
	b.SetCores(2) // Run on only 2 cores
	minMhz, _ := b.GetCPUFreqBounds()
	b.SetFreqLimit(minMhz) // Lock to absolute lowest CPU frequency
	minW, _ := b.GetRAPLBounds()
	b.SetRAPLPL1(minW) // Absolute lowest CPU power limit
	b.SetRAPLPL2(minW) // Absolute lowest CPU boost limit
	b.SetTurbo(false) // Disable turbo completely
	b.SetEPP("power") // Tell kernel to aggressively save power
	minGPU, _ := b.GetGPUBounds()
	b.SetGPUFreq(minGPU) // Absolute lowest GPU frequency
	b.SetASPM("powersave") // Enable maximum PCIe power saving
	b.SetWifiPowerSave(true) // Enable Wi-Fi sleep
	b.SetKbdBacklight(false) // Turn off keyboard light
	b.SetAudioPowerSave(true) // Turn off audio amp when silent
	b.SetLCDBrightness(10) // Dim screen significantly
	b.SetAutosuspend(true) // Put unused USB/PCI devices to sleep
	b.SetWatchdog(false) // Disable watchdog to prevent periodic wakeups
	b.SetVMWriteback(6000) // Delay disk writes (60 seconds) to keep SSD asleep
}

// ApplyModeRestore resets the system back to typical factory OS defaults
func (b *LinuxBackend) ApplyModeRestore() {
	b.SetCores(b.GetNumCPUs()) // Ensure all cores are available
	_, maxMhz := b.GetCPUFreqBounds()
	b.SetFreqLimit(maxMhz) // Allow full frequency range
	_, maxW := b.GetRAPLBounds()
	b.SetRAPLPL1(maxW) // Restore full package power
	b.SetRAPLPL2(maxW) // Restore full boost power
	b.SetTurbo(true) // Ensure boost is available
	b.SetEPP("default") // Let the OS decide Energy/Performance dynamically
	_, maxGPU := b.GetGPUBounds()
	b.SetGPUFreq(maxGPU) // Allow full GPU frequency
	b.SetASPM("default") // Let OS manage PCIe ASPM
	b.SetWifiPowerSave(false) // Default Wi-Fi to always ready
	b.SetAudioPowerSave(false) // Default Audio to ready
	b.SetAutosuspend(false) // Default Devices to ready
	b.SetWatchdog(true) // Standard kernel watchdog
	b.SetVMWriteback(500) // Standard 5s disk flush
}

var daemonRunning bool // Global state to track if daemon is active
var daemonQuit chan struct{} // Channel to signal the daemon to stop
var daemonMutex sync.Mutex // Mutex to prevent race conditions on daemon state

// StopDaemon stops the auto-extreme adjustment loop
func (b *LinuxBackend) StopDaemon() {
	daemonMutex.Lock()
	defer daemonMutex.Unlock()
	if daemonRunning && daemonQuit != nil {
		close(daemonQuit) // Send kill signal
		daemonRunning = false
		daemonQuit = nil // Prevent double close
	}
}

// StartAutoExtremeDaemon starts a background process that watches battery state
func (b *LinuxBackend) StartAutoExtremeDaemon() {
	b.StopDaemon() // Ensure only one runs at a time
	
	daemonMutex.Lock()
	daemonQuit = make(chan struct{})
	daemonRunning = true
	daemonMutex.Unlock()

	go func() { // Run in background goroutine
		for {
			select { // Wait for either 10 seconds or a quit signal
			case <-time.After(10 * time.Second):
				if b.IsCharging() { // If plugged in
					b.ApplyModePerformance() // Ramp up performance
				} else {
					if b.GetBatteryPercentage() < 20 { // If battery critical
						b.ApplyModeExtreme() // Go into extreme saving
					} else { // If battery normal
						b.ApplyModeRestore() // Run normally
					}
				}
			case <-daemonQuit:
				return // Exit goroutine
			}
		}
	}()
}

func (b *LinuxBackend) SetBrightnessTarget(target string) {}
func (b *LinuxBackend) SetRefreshRate(target string) {}
func (b *LinuxBackend) SetHyprEffects(enabled bool) {}
func (b *LinuxBackend) SetNMIWatchdog(enabled bool) {}
func (b *LinuxBackend) SetVMDirty(writeback int, expire int) {}
