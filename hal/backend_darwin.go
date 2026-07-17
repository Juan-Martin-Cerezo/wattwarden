//go:build darwin
// +build darwin

package hal

import (
	"math"
	"os/exec"
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
	return strings.Contains(out, "AC Power") || strings.Contains(out, "charging")
}

func (b *DarwinBackend) GetBatteryTime() string { return "N/A" }
func (b *DarwinBackend) GetPowerConsumptionWatts() float64 { return 0.0 }
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

func (b *DarwinBackend) GetLCDBrightness() int { return 100 }
func (b *DarwinBackend) SetLCDBrightness(percent int) {}

func (b *DarwinBackend) GetBluetooth() bool { return true }
func (b *DarwinBackend) SetBluetooth(enabled bool) {}
func (b *DarwinBackend) GetWifiEnable() bool { return true }
func (b *DarwinBackend) SetWifiEnable(enabled bool) {}
func (b *DarwinBackend) GetAutosuspend() bool { return false }
func (b *DarwinBackend) SetAutosuspend(enabled bool) {}
func (b *DarwinBackend) GetWatchdog() bool { return true }
func (b *DarwinBackend) SetWatchdog(enabled bool) {}
func (b *DarwinBackend) GetVMWriteback() int { return 500 }
func (b *DarwinBackend) SetVMWriteback(centisecs int) {}
func (b *DarwinBackend) ProcessPurge() {
	runMacCmd("sudo", "purge")
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
