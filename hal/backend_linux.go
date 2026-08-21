//go:build linux
// +build linux

package hal // Hardware Abstraction Layer for Linux

import (
	"encoding/json" // JSON library for parsing hyprctl output
	"fmt" // Formatting library for string manipulation
	"math" // Math library for calculations
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

var (
	batteryPathOnce sync.Once
	batteryPath     string
)

// getBatteryPath dynamically locates the system's primary battery directory
func getBatteryPath() string {
	batteryPathOnce.Do(func() {
		// First try standard names
		for _, name := range []string{"BAT0", "BAT1", "BAT2", "BATT"} {
			p := "/sys/class/power_supply/" + name
			if _, err := os.Stat(p); err == nil {
				batteryPath = p
				return
			}
		}
		// Fallback: scan for any supply with type "Battery"
		matches, _ := filepath.Glob("/sys/class/power_supply/*")
		for _, p := range matches {
			typeContent, _ := os.ReadFile(filepath.Join(p, "type"))
			if strings.TrimSpace(string(typeContent)) == "Battery" {
				batteryPath = p
				return
			}
		}
		// Final fallback
		batteryPath = "/sys/class/power_supply/BAT0"
	})
	return batteryPath
}

// parseUevent parses the battery's uevent properties file directly
func parseUevent(batPath string) map[string]string {
	m := make(map[string]string)
	content, err := os.ReadFile(filepath.Join(batPath, "uevent"))
	if err != nil {
		return m
	}
	lines := strings.Split(string(content), "\n")
	for _, line := range lines {
		parts := strings.SplitN(line, "=", 2)
		if len(parts) == 2 {
			m[strings.TrimSpace(parts[0])] = strings.TrimSpace(parts[1])
		}
	}
	return m
}

// GetBatteryPercentage returns the current battery level
func (b *LinuxBackend) GetBatteryPercentage() int {
	v, _ := strconv.Atoi(readSys(filepath.Join(getBatteryPath(), "capacity"))) // Read 0-100 percentage
	return v // Return it
}

// IsCharging checks if the device is plugged in
func (b *LinuxBackend) IsCharging() bool {
	// First check AC power supply online status if available
	matches, _ := filepath.Glob("/sys/class/power_supply/*")
	for _, p := range matches {
		typeContent := strings.TrimSpace(readSys(filepath.Join(p, "type")))
		if typeContent == "Mains" || typeContent == "USB_C" || typeContent == "USB" {
			if readSys(filepath.Join(p, "online")) == "1" {
				return true
			}
		}
	}
	status := readSys(filepath.Join(getBatteryPath(), "status")) // Read charging state
	return status == "Charging" || status == "Full"
}

// GetBatteryTime estimates remaining time or time to full
func (b *LinuxBackend) GetBatteryTime() string {
	if b.IsCharging() { // If on AC power
		return "Charging" // Simple string
	}
	
	bat := getBatteryPath()
	energyStr := readSys(filepath.Join(bat, "energy_now")) // Attempt to read energy in micro-watt-hours
	powerStr := readSys(filepath.Join(bat, "power_now")) // Attempt to read power in micro-watts
	
	if energyStr == "" || powerStr == "" { // Try parsing uevent
		uevent := parseUevent(bat)
		energyStr = uevent["POWER_SUPPLY_ENERGY_NOW"]
		powerStr = uevent["POWER_SUPPLY_POWER_NOW"]
		
		if energyStr == "" || powerStr == "" { // If hardware reports in amps/charge instead
			energyStr = uevent["POWER_SUPPLY_CHARGE_NOW"]
			powerStr = uevent["POWER_SUPPLY_CURRENT_NOW"]
			voltageStr := uevent["POWER_SUPPLY_VOLTAGE_NOW"]
			
			if energyStr == "" || powerStr == "" || voltageStr == "" {
				energyStr = readSys(filepath.Join(bat, "charge_now"))
				powerStr = readSys(filepath.Join(bat, "current_now"))
				voltageStr = readSys(filepath.Join(bat, "voltage_now"))
			}
			
			if energyStr != "" && powerStr != "" && voltageStr != "" { // If all are available
				e, _ := strconv.ParseFloat(energyStr, 64) // Convert to float
				c, _ := strconv.ParseFloat(powerStr, 64) // Convert to float
				v, _ := strconv.ParseFloat(voltageStr, 64) // Convert to float
				
				energy := e * (v / 1000000.0) // Calculate true energy
				power := math.Abs(c) * (v / 1000000.0) // Calculate true power
				
				if power > 0 { // If discharging
					hours := energy / power // Calculate hours remaining
					h := int(hours) // Extract full hours
					m := int((hours - float64(h)) * 60) // Extract remaining minutes
					return fmt.Sprintf("%dh %02dm", h, m) // Return formatted string
				}
			}
			return "Calculating..." // Fallback
		}
	}
	
	energy, _ := strconv.ParseFloat(energyStr, 64) // Convert energy to float
	power, _ := strconv.ParseFloat(powerStr, 64) // Convert power to float
	power = math.Abs(power)
	
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
	bat := getBatteryPath()
	
	// 1. Some systems expose power_now (microwatts) directly
	if pStr := readSys(filepath.Join(bat, "power_now")); pStr != "" {
		if pVal, err := strconv.ParseFloat(pStr, 64); err == nil {
			return math.Abs(pVal) / 1000000.0 // Convert microwatts to Watts
		}
	}
	
	// 2. Fallback: current_now (microamperes) * voltage_now (microvolts)
	cStr := readSys(filepath.Join(bat, "current_now"))
	vStr := readSys(filepath.Join(bat, "voltage_now"))
	if cStr != "" && vStr != "" {
		c, _ := strconv.ParseFloat(cStr, 64)
		v, _ := strconv.ParseFloat(vStr, 64)
		return math.Abs(c * v) / 1000000000000.0 // Calculate and convert to Watts
	}
	
	// 3. Last fallback: Parse uevent file directly (covers cases where sysfs exposes it in uevent but not separate files)
	uevent := parseUevent(bat)
	if pStr, ok := uevent["POWER_SUPPLY_POWER_NOW"]; ok && pStr != "" {
		if pVal, err := strconv.ParseFloat(pStr, 64); err == nil {
			return math.Abs(pVal) / 1000000.0
		}
	}
	if cStr, ok := uevent["POWER_SUPPLY_CURRENT_NOW"]; ok && cStr != "" {
		if vStr, ok := uevent["POWER_SUPPLY_VOLTAGE_NOW"]; ok && vStr != "" {
			c, _ := strconv.ParseFloat(cStr, 64)
			v, _ := strconv.ParseFloat(vStr, 64)
			return math.Abs(c * v) / 1000000000000.0
		}
	}
	
	return 0.0
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
	out := runCmd("brightnessctl -m")
	if out != "" {
		parts := strings.Split(out, ",")
		if len(parts) >= 4 {
			percStr := strings.TrimSuffix(parts[3], "%")
			if v, err := strconv.Atoi(percStr); err == nil {
				return v
			}
		}
	}
	return 100 // Fallback
}

// SetLCDBrightness sets screen brightness to a specific percentage
func (b *LinuxBackend) SetLCDBrightness(percent int) {
	if percent < 1 { percent = 1 } // Ensure screen doesn't completely turn off and become unusable
	if percent > 100 { percent = 100 } // Clamp to max 100%
	fs, _ := filepath.Glob("/sys/class/backlight/*") // Find backlights
	for _, f := range fs {
		max, _ := strconv.Atoi(readSys(f + "/max_brightness")) // Read max
		if max > 0 {
			target := (percent * max) / 100 // Calculate absolute value based on percentage
			writeSys(f+"/brightness", strconv.Itoa(target)) // Apply
		}
	}
	_ = exec.Command("brightnessctl", "set", fmt.Sprintf("%d%%", percent)).Run()
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
	b.SetLCDBrightness(100) // Restore full brightness
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
	b.SetLCDBrightness(100) // Restore full brightness
	b.SetAutosuspend(false) // Default Devices to ready
	b.SetWatchdog(true) // Standard kernel watchdog
	b.SetVMWriteback(500) // Standard 5s disk flush
}

var daemonRunning bool // Global state to track if daemon is active
var daemonQuit chan struct{} // Channel to signal the daemon to stop
var daemonMutex sync.Mutex // Mutex to prevent race conditions on daemon state
var autoBrightnessEnabled = true // Auto-brightness toggle for daemon mode
var autoBrightnessMutex sync.RWMutex // Mutex for auto-brightness toggle
// GetAutoBrightness returns whether auto brightness is enabled in daemon mode
func (b *LinuxBackend) GetAutoBrightness() bool {
	if os.Geteuid() == 0 {
		data, err := os.ReadFile("/etc/wattwarden/config.json")
		if err == nil {
			var cfg struct {
				AutoBrightness bool `json:"auto_brightness"`
			}
			if err := json.Unmarshal(data, &cfg); err == nil {
				return cfg.AutoBrightness
			}
		}
	}

	autoBrightnessMutex.RLock()
	defer autoBrightnessMutex.RUnlock()
	return autoBrightnessEnabled
}

// SetAutoBrightness configures auto brightness behavior in daemon mode
func (b *LinuxBackend) SetAutoBrightness(enabled bool) {
	autoBrightnessMutex.Lock()
	autoBrightnessEnabled = enabled
	autoBrightnessMutex.Unlock()

	_ = os.MkdirAll("/etc/wattwarden", 0755)
	var cfg struct {
		AutoExtremeEnabled bool `json:"auto_extreme_enabled"`
		AutoBrightness     bool `json:"auto_brightness"`
	}
	cfg.AutoExtremeEnabled = true
	data, err := os.ReadFile("/etc/wattwarden/config.json")
	if err == nil {
		_ = json.Unmarshal(data, &cfg)
	}
	cfg.AutoBrightness = enabled
	newData, err := json.MarshalIndent(cfg, "", "  ")
	if err == nil {
		_ = os.WriteFile("/etc/wattwarden/config.json", newData, 0644)
	}
}

// IsDaemonRunning returns true if the Auto Extreme daemon is running
func (b *LinuxBackend) IsDaemonRunning() bool {
	daemonMutex.Lock()
	defer daemonMutex.Unlock()
	return daemonRunning
}

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

// getLinuxActiveWindow retrieves the active window class on Hyprland, Wayland or X11 even when running under sudo
func getLinuxActiveWindow() string {
	sig := os.Getenv("HYPRLAND_INSTANCE_SIGNATURE")
	runtimeDir := os.Getenv("XDG_RUNTIME_DIR")

	if sig == "" || runtimeDir == "" {
		matches, _ := filepath.Glob("/run/user/*/hypr/*/.socket.sock")
		if len(matches) > 0 {
			parts := strings.Split(matches[0], "/")
			if len(parts) >= 6 {
				runtimeDir = "/" + filepath.Join(parts[1], parts[2], parts[3])
				sig = parts[5]
			}
		}
	}

	if sig != "" && runtimeDir != "" {
		cmd := exec.Command("hyprctl", "activewindow", "-j")
		cmd.Env = append(os.Environ(), "HYPRLAND_INSTANCE_SIGNATURE="+sig, "XDG_RUNTIME_DIR="+runtimeDir)
		out, err := cmd.Output()
		if err == nil {
			var win struct {
				Class string `json:"class"`
			}
			if err := json.Unmarshal(out, &win); err == nil {
				return strings.ToLower(win.Class)
			}
		}
	}

	if out := runCmd("xdotool getactivewindow getwindowclassname 2>/dev/null"); out != "" {
		return strings.ToLower(out)
	}

	return ""
}

type cpuStat struct {
	idle  uint64
	total uint64
}

func readCPUStat() cpuStat {
	data, err := os.ReadFile("/proc/stat")
	if err != nil {
		return cpuStat{}
	}
	lines := strings.Split(string(data), "\n")
	if len(lines) == 0 {
		return cpuStat{}
	}
	fields := strings.Fields(lines[0])
	if len(fields) < 5 || fields[0] != "cpu" {
		return cpuStat{}
	}
	var total, idle uint64
	for i := 1; i < len(fields); i++ {
		val, _ := strconv.ParseUint(fields[i], 10, 64)
		total += val
		if i == 4 || i == 5 {
			idle += val
		}
	}
	return cpuStat{idle: idle, total: total}
}

var (
	lastStatMutex sync.Mutex
	lastCPUStat   cpuStat
)

func getInstantCPULoad() float64 {
	lastStatMutex.Lock()
	defer lastStatMutex.Unlock()
	cur := readCPUStat()
	if lastCPUStat.total == 0 || cur.total <= lastCPUStat.total {
		lastCPUStat = cur
		return 0.0
	}
	deltaTotal := float64(cur.total - lastCPUStat.total)
	deltaIdle := float64(cur.idle - lastCPUStat.idle)
	lastCPUStat = cur
	if deltaTotal == 0 {
		return 0.0
	}
	usage := (deltaTotal - deltaIdle) / deltaTotal
	if usage < 0 { usage = 0 }
	if usage > 1.0 { usage = 1.0 }
	return usage
}

// StartAutoExtremeDaemon starts a background process that watches battery state
func (b *LinuxBackend) StartAutoExtremeDaemon() {
	b.StopDaemon() // Ensure only one runs at a time
	
	daemonMutex.Lock()
	daemonQuit = make(chan struct{})
	daemonRunning = true
	daemonMutex.Unlock()

	go func() { // Run in background goroutine
		ticker := time.NewTicker(5 * time.Second)
		defer ticker.Stop()

		var lastAppliedBrightness int

		applyBrightness := func() {
			if !b.GetAutoBrightness() {
				return
			}
			if b.IsCharging() {
				if b.GetLCDBrightness() < 80 {
					b.SetLCDBrightness(100)
					lastAppliedBrightness = 100
				}
				return
			}

			activeClass := getLinuxActiveWindow()

			isTerminal := activeClass == "" ||
				strings.Contains(activeClass, "kitty") ||
				strings.Contains(activeClass, "foot") ||
				strings.Contains(activeClass, "alacritty") ||
				strings.Contains(activeClass, "wezterm") ||
				strings.Contains(activeClass, "ghostty") ||
				strings.Contains(activeClass, "xterm")

			isHeavyUI := strings.Contains(activeClass, "firefox") ||
				strings.Contains(activeClass, "chrome") ||
				strings.Contains(activeClass, "chromium") ||
				strings.Contains(activeClass, "brave") ||
				strings.Contains(activeClass, "zen") ||
				strings.Contains(activeClass, "code") ||
				strings.Contains(activeClass, "cursor") ||
				strings.Contains(activeClass, "idea") ||
				strings.Contains(activeClass, "studio")

			targetBrightness := 20
			if isTerminal {
				targetBrightness = 12
			} else if isHeavyUI {
				targetBrightness = 30
			} else {
				targetBrightness = 20
			}

			if targetBrightness != lastAppliedBrightness || b.GetLCDBrightness() != targetBrightness {
				b.SetLCDBrightness(targetBrightness)
				lastAppliedBrightness = targetBrightness
			}
		}

		// Helper to apply logic based on battery state
		applyLogic := func() {
			if b.IsCharging() { // If plugged in
				b.SetCores(b.GetNumCPUs())
				b.SetFreqLimit(99999)
				b.SetRAPLPL1(115)
				b.SetRAPLPL2(115)
				b.SetTurbo(true)
				b.SetEPP("performance")
				b.SetGPUFreq(99999)
				b.SetASPM("performance")
				b.SetWifiPowerSave(false)
				b.SetAudioPowerSave(false)
				b.SetAutosuspend(false)
				b.SetWatchdog(true)
				b.SetVMWriteback(500)
				if b.GetAutoBrightness() {
					b.SetLCDBrightness(100)
				}
			} else {
				// Read /proc/loadavg
				loadAvgStr := readSys("/proc/loadavg")
				parts := strings.Fields(loadAvgStr)
				load := 0.0
				if len(parts) > 0 {
					load, _ = strconv.ParseFloat(parts[0], 64)
				}
				
				// Map load to 0.0 - 1.0 scale
				powerLevel := load / float64(b.GetNumCPUs())
				if powerLevel > 1.0 { powerLevel = 1.0 }
				
				// Quantize into 4 steps: 0, 0.33, 0.66, 1.0
				discretePower := math.Round(powerLevel * 3) / 3.0
				
				// Calculate bounds (40% of hardware max)
				minCPU, hwMaxCPU := b.GetCPUFreqBounds()
				maxCPU := int(float64(minCPU) + float64(hwMaxCPU-minCPU)*0.4)
				
				minGPU, hwMaxGPU := b.GetGPUBounds()
				maxGPU := int(float64(minGPU) + float64(hwMaxGPU-minGPU)*0.4)
				
				minW, hwMaxW := b.GetRAPLBounds()
				maxW := int(float64(minW) + float64(hwMaxW-minW)*0.4)
				
				maxCores := b.GetNumCPUs() / 2
				if maxCores < 1 { maxCores = 1 }
				
				targetCores := int(1.0 + discretePower * float64(maxCores - 1))
				if targetCores < 1 { targetCores = 1 }
				
				targetCPU := int(float64(minCPU) + discretePower * float64(maxCPU - minCPU))
				targetGPU := int(float64(minGPU) + discretePower * float64(maxGPU - minGPU))
				targetRAPL := int(float64(minW) + discretePower * float64(maxW - minW))
				
				targetEPP := "power"
				targetTurbo := discretePower >= 0.8
				
				// Apply settings dynamically
				b.SetCores(targetCores)
				b.SetFreqLimit(targetCPU)
				b.SetGPUFreq(targetGPU)
				b.SetRAPLPL1(targetRAPL)
				b.SetRAPLPL2(targetRAPL)
				b.SetEPP(targetEPP)
				b.SetTurbo(targetTurbo)
				
				b.SetASPM("powersave") 
				b.SetWifiPowerSave(true)
				b.SetKbdBacklight(false)
				b.SetAudioPowerSave(true)
				b.SetAutosuspend(true)
				b.SetWatchdog(false)
				b.SetVMWriteback(6000)
			}
		}

		brightnessTicker := time.NewTicker(300 * time.Millisecond)
		defer brightnessTicker.Stop()

		// Run immediately the first time
		applyLogic()
		applyBrightness()

		for {
			select {
			case <-ticker.C:
				applyLogic()
			case <-brightnessTicker.C:
				applyBrightness()
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
