//go:build darwin
// +build darwin

package hal

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"os/exec"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
)

type DarwinBackend struct{}

func init() { CurrentBackend = &DarwinBackend{} }

func runMacCmd(name string, arg ...string) string {
	out, err := exec.Command(name, arg...).CombinedOutput()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

func parseMacFloat(output, label string) float64 {
	re := regexp.MustCompile(label + `[: ]+(-?[0-9]+(?:\.[0-9]+)?)`)
	match := re.FindStringSubmatch(output)
	if len(match) != 2 {
		return 0
	}
	value, _ := strconv.ParseFloat(match[1], 64)
	return value
}

func (b *DarwinBackend) GetOS() string { return "macOS" }
func (b *DarwinBackend) GetNumCPUs() int { return runtime.NumCPU() }
func (b *DarwinBackend) GetCores() int { return runtime.NumCPU() }
func (b *DarwinBackend) SetCores(n int) {}

func (b *DarwinBackend) GetFreqLimit() int { return 0 }
func (b *DarwinBackend) SetFreqLimit(m int) {} 

func (b *DarwinBackend) GetBatteryPercentage() int {
	out := runMacCmd("pmset", "-g", "batt")
	if idx := strings.Index(out, "%"); idx != -1 {
		start := idx - 3
		if start < 0 { start = 0 }
		percStr := strings.TrimSpace(out[start:idx])
		// Sometimes there's a character or bracket, so we just extract the numbers
		numStr := ""
		for _, char := range percStr {
			if char >= '0' && char <= '9' {
				numStr += string(char)
			}
		}
		if v, err := strconv.Atoi(numStr); err == nil {
			return v
		}
	}
	return 100
}

func (b *DarwinBackend) IsCharging() bool {
	out := runMacCmd("pmset", "-g", "batt")
	return strings.Contains(out, "AC Power") && !strings.Contains(out, "discharging")
}

func (b *DarwinBackend) GetBatteryTime() string {
	out := runMacCmd("pmset", "-g", "batt")
	match := regexp.MustCompile(`([0-9]+:[0-9]+) remaining`).FindStringSubmatch(out)
	if len(match) == 2 {
		return match[1]
	}
	if b.IsCharging() {
		return "Charging"
	}
	return "Calculating..."
}

func (b *DarwinBackend) GetPowerConsumptionWatts() float64 {
	out := runMacCmd("ioreg", "-rn", "AppleSmartBattery")
	current := math.Abs(parseMacFloat(out, `"Current" =`))
	voltage := parseMacFloat(out, `"Voltage" =`)
	if current == 0 || voltage == 0 {
		return 0
	}
	return current * voltage / 1000000.0
}
func (b *DarwinBackend) GetRAPLPL1() int { return 0 }
func (b *DarwinBackend) SetRAPLPL1(w int) {}
func (b *DarwinBackend) GetRAPLPL2() int { return 0 }
func (b *DarwinBackend) SetRAPLPL2(w int) {}
func (b *DarwinBackend) GetTurbo() bool { return true }
func (b *DarwinBackend) SetTurbo(e bool) {}
func (b *DarwinBackend) GetEPP() string { return "default" }
func (b *DarwinBackend) SetEPP(p string) {}
func (b *DarwinBackend) GetGPUFreq() int { return 0 }
func (b *DarwinBackend) SetGPUFreq(m int) {}
func (b *DarwinBackend) GetASPM() string { return "default" }
func (b *DarwinBackend) SetASPM(p string) {}

func (b *DarwinBackend) GetWifiPowerSave() bool { return false }
func (b *DarwinBackend) SetWifiPowerSave(e bool) {}
func (b *DarwinBackend) GetKbdBacklight() bool { return false }
func (b *DarwinBackend) SetKbdBacklight(e bool) {}
func (b *DarwinBackend) GetAudioPowerSave() bool { return false }
func (b *DarwinBackend) SetAudioPowerSave(e bool) {}
func (b *DarwinBackend) SetBrightnessTarget(t string) {}
func (b *DarwinBackend) SetRefreshRate(t string) {}
func (b *DarwinBackend) SetHyprEffects(e bool) {}
func (b *DarwinBackend) SetNMIWatchdog(e bool) {}
func (b *DarwinBackend) SetVMDirty(w int, e int) {}

func (b *DarwinBackend) GetLCDBrightness() int {
	out := runMacCmd("brightness", "-l")
	if strings.Contains(out, "brightness") {
		match := regexp.MustCompile(`brightness[ =]+([0-9.]+)`).FindStringSubmatch(out)
		if len(match) == 2 {
			if v, err := strconv.ParseFloat(match[1], 64); err == nil {
				return int(v * 100)
			}
		}
	}
	return 100
}

func (b *DarwinBackend) SetLCDBrightness(percent int) {
	if percent < 1 { percent = 1 }
	if percent > 100 { percent = 100 }
	val := float64(percent) / 100.0
	runMacCmd("brightness", fmt.Sprintf("%.2f", val))
}

func (b *DarwinBackend) GetBluetooth() bool {
	out := runMacCmd("defaults", "read", "/Library/Preferences/com.apple.Bluetooth", "ControllerPowerState")
	return strings.TrimSpace(out) != "0"
}

func (b *DarwinBackend) SetBluetooth(enabled bool) {
	val := "1"
	if !enabled { val = "0" }
	runMacCmd("defaults", "write", "/Library/Preferences/com.apple.Bluetooth", "ControllerPowerState", "-int", val)
	runMacCmd("blueutil", "--power", val)
}

func getMacWifiDevice() string {
	out := runMacCmd("networksetup", "-listallhardwareports")
	lines := strings.Split(out, "\n")
	for i, line := range lines {
		if strings.Contains(line, "Wi-Fi") || strings.Contains(line, "AirPort") {
			if i+1 < len(lines) && strings.Contains(lines[i+1], "Device:") {
				parts := strings.Fields(lines[i+1])
				if len(parts) >= 2 {
					return parts[1]
				}
			}
		}
	}
	return "en0"
}

func (b *DarwinBackend) GetWifiEnable() bool {
	dev := getMacWifiDevice()
	out := runMacCmd("networksetup", "-getairportpower", dev)
	return strings.Contains(strings.ToLower(out), "on")
}

func (b *DarwinBackend) SetWifiEnable(enabled bool) {
	dev := getMacWifiDevice()
	val := "on"
	if !enabled { val = "off" }
	runMacCmd("networksetup", "-setairportpower", dev, val)
}
func (b *DarwinBackend) GetAutosuspend() bool { return false }
func (b *DarwinBackend) SetAutosuspend(enabled bool) {}
func (b *DarwinBackend) GetWatchdog() bool { return true }
func (b *DarwinBackend) SetWatchdog(enabled bool) {}
func (b *DarwinBackend) GetVMWriteback() int { return 500 }
func (b *DarwinBackend) SetVMWriteback(centisecs int) {}
func (b *DarwinBackend) ProcessPurge() {
	runMacCmd("purge")
}

func (b *DarwinBackend) ApplyModePerformance() {
	runMacCmd("pmset", "-a", "lowpowermode", "0")
	runMacCmd("pmset", "-a", "tcpkeepalive", "1")
	runMacCmd("pmset", "-a", "displaysleep", "10")
}

func (b *DarwinBackend) ApplyModeExtreme() {
	runMacCmd("pmset", "-a", "lowpowermode", "1")
	runMacCmd("pmset", "-a", "tcpkeepalive", "0")
	runMacCmd("pmset", "-a", "displaysleep", "3")
}

func (b *DarwinBackend) ApplyModeRestore() {
	runMacCmd("pmset", "-a", "lowpowermode", "0")
	runMacCmd("pmset", "-a", "tcpkeepalive", "1")
	runMacCmd("pmset", "-a", "displaysleep", "10")
}

var daemonMacRunning bool
var daemonMacQuit chan struct{}
var daemonMacMutex sync.Mutex
var autoBrightnessMacEnabled = true
var autoBrightnessMacMutex sync.RWMutex
var lastAutoBrightnessMacSet time.Time

func (b *DarwinBackend) GetAutoBrightness() bool {
	data, err := os.ReadFile("/etc/wattwarden/config.json")
	if err == nil {
		var cfg struct {
			AutoBrightness bool `json:"auto_brightness"`
		}
		if err := json.Unmarshal(data, &cfg); err == nil {
			return cfg.AutoBrightness
		}
	}

	autoBrightnessMacMutex.RLock()
	defer autoBrightnessMacMutex.RUnlock()
	return autoBrightnessMacEnabled
}

func (b *DarwinBackend) SetAutoBrightness(enabled bool) {
	autoBrightnessMacMutex.Lock()
	autoBrightnessMacEnabled = enabled
	lastAutoBrightnessMacSet = time.Now()
	autoBrightnessMacMutex.Unlock()

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

func (b *DarwinBackend) IsDaemonRunning() bool {
	daemonMacMutex.Lock()
	defer daemonMacMutex.Unlock()
	return daemonMacRunning
}

func (b *DarwinBackend) StopDaemon() {
	daemonMacMutex.Lock()
	defer daemonMacMutex.Unlock()
	if daemonMacRunning && daemonMacQuit != nil {
		close(daemonMacQuit)
		daemonMacRunning = false
		daemonMacQuit = nil
	}
}

func getMacLoad() float64 {
	out := runMacCmd("sysctl", "-n", "vm.loadavg")
	out = strings.Trim(out, "{} ")
	parts := strings.Fields(out)
	if len(parts) >= 1 {
		if v, err := strconv.ParseFloat(parts[0], 64); err == nil {
			return v / float64(runtime.NumCPU())
		}
	}
	return 0.0
}

func (b *DarwinBackend) StartAutoExtremeDaemon() {
	b.StopDaemon()
	
	daemonMacMutex.Lock()
	daemonMacQuit = make(chan struct{})
	daemonMacRunning = true
	daemonMacMutex.Unlock()

	go func() {
		ticker := time.NewTicker(10 * time.Second)
		defer ticker.Stop()

		applyLogic := func() {
			if b.IsCharging() {
				b.ApplyModePerformance()
			} else {
				powerLevel := getMacLoad()
				if powerLevel > 1.0 { powerLevel = 1.0 }
				
				discretePower := math.Round(powerLevel * 3) / 3.0
				
				if discretePower < 0.6 {
					runMacCmd("pmset", "-a", "lowpowermode", "1")
				} else {
					runMacCmd("pmset", "-a", "lowpowermode", "0")
				}
			}
		}

		applyLogic()

		for {
			select {
			case <-ticker.C:
				applyLogic()
			case <-daemonMacQuit:
				return
			}
		}
	}()
}
