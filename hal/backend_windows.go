//go:build windows
// +build windows

package hal

import (
	"fmt"
	"math"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"
	"unsafe"
)

type WindowsBackend struct{}

func init() { CurrentBackend = &WindowsBackend{} }

func runWinCmd(name string, arg ...string) string {
	out, err := exec.Command(name, arg...).CombinedOutput()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// Win32 API Definitions for Battery Status
type systemPowerStatus struct {
	ACLineStatus        byte
	BatteryFlag         byte
	BatteryLifePercent  byte
	SystemStatusFlag    byte
	BatteryLifeTime     uint32
	BatteryFullLifeTime uint32
}

var (
	kernel32           = syscall.NewLazyDLL("kernel32.dll")
	getSystemPowerStat = kernel32.NewProc("GetSystemPowerStatus")
)

func getPowerStatus() *systemPowerStatus {
	var sps systemPowerStatus
	ret, _, _ := getSystemPowerStat.Call(uintptr(unsafe.Pointer(&sps)))
	if ret == 0 {
		return nil
	}
	return &sps
}

func (b *WindowsBackend) GetOS() string { return "Windows" }

func (b *WindowsBackend) GetNumCPUs() int { return runtime.NumCPU() }
func (b *WindowsBackend) GetCores() int { return runtime.NumCPU() } // Stubbed, affinity limits effectively do this
func (b *WindowsBackend) SetCores(n int) {} // Stubbed

func (b *WindowsBackend) GetFreqLimit() int { return 0 }
func (b *WindowsBackend) SetFreqLimit(m int) {} // Frequency is managed via powercfg percentages

func (b *WindowsBackend) GetBatteryPercentage() int {
	sps := getPowerStatus()
	if sps != nil && sps.BatteryLifePercent <= 100 {
		return int(sps.BatteryLifePercent)
	}
	return 100
}

func (b *WindowsBackend) IsCharging() bool {
	sps := getPowerStatus()
	if sps != nil {
		return sps.ACLineStatus == 1
	}
	return true
}

func (b *WindowsBackend) GetBatteryTime() string { return "N/A" }
func (b *WindowsBackend) GetPowerConsumptionWatts() float64 { return 0.0 }
func (b *WindowsBackend) GetRAPLPL1() int { return 0 }
func (b *WindowsBackend) SetRAPLPL1(w int) {}
func (b *WindowsBackend) GetRAPLPL2() int { return 0 }
func (b *WindowsBackend) SetRAPLPL2(w int) {}
func (b *WindowsBackend) GetTurbo() bool { return true }
func (b *WindowsBackend) SetTurbo(e bool) {}
func (b *WindowsBackend) GetEPP() string { return "default" }
func (b *WindowsBackend) SetEPP(p string) {}
func (b *WindowsBackend) GetGPUFreq() int { return 0 }
func (b *WindowsBackend) SetGPUFreq(m int) {}
func (b *WindowsBackend) GetASPM() string { return "default" }
func (b *WindowsBackend) SetASPM(p string) {}

func (b *WindowsBackend) GetWifiPowerSave() bool { return false }
func (b *WindowsBackend) SetWifiPowerSave(e bool) {}

func (b *WindowsBackend) GetKbdBacklight() bool { return false }
func (b *WindowsBackend) SetKbdBacklight(e bool) {}
func (b *WindowsBackend) GetAudioPowerSave() bool { return false }
func (b *WindowsBackend) SetAudioPowerSave(e bool) {}
func (b *WindowsBackend) SetBrightnessTarget(t string) {}
func (b *WindowsBackend) SetRefreshRate(t string) {}
func (b *WindowsBackend) SetHyprEffects(e bool) {}
func (b *WindowsBackend) SetNMIWatchdog(e bool) {}
func (b *WindowsBackend) SetVMDirty(w int, e int) {}

// LCDBrightness via WMI
func (b *WindowsBackend) GetLCDBrightness() int {
	out := runWinCmd("powershell", "-NoProfile", "-Command", "(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightness).CurrentBrightness")
	if v, err := strconv.Atoi(out); err == nil {
		return v
	}
	return 100
}

func (b *WindowsBackend) SetLCDBrightness(percent int) {
	if percent < 0 { percent = 0 }
	if percent > 100 { percent = 100 }
	runWinCmd("powershell", "-NoProfile", "-Command", fmt.Sprintf("(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1, %d)", percent))
}

func (b *WindowsBackend) GetBluetooth() bool { return true }
func (b *WindowsBackend) SetBluetooth(enabled bool) {}
func (b *WindowsBackend) GetWifiEnable() bool { return true }
func (b *WindowsBackend) SetWifiEnable(enabled bool) {}
func (b *WindowsBackend) GetAutosuspend() bool { return false }
func (b *WindowsBackend) SetAutosuspend(enabled bool) {}
func (b *WindowsBackend) GetWatchdog() bool { return true }
func (b *WindowsBackend) SetWatchdog(enabled bool) {}
func (b *WindowsBackend) GetVMWriteback() int { return 500 }
func (b *WindowsBackend) SetVMWriteback(centisecs int) {}
func (b *WindowsBackend) ProcessPurge() {}

// Powercfg helper
func setWinProcThrottle(maxPercent int) {
	// scheme_current, sub_processor, PROCTHROTTLEMAX
	runWinCmd("powercfg", "-setacvalueindex", "SCHEME_CURRENT", "SUB_PROCESSOR", "PROCTHROTTLEMAX", strconv.Itoa(maxPercent))
	runWinCmd("powercfg", "-setdcvalueindex", "SCHEME_CURRENT", "SUB_PROCESSOR", "PROCTHROTTLEMAX", strconv.Itoa(maxPercent))
	runWinCmd("powercfg", "-setactive", "SCHEME_CURRENT") // Apply changes
}

func (b *WindowsBackend) ApplyModePerformance() {
	setWinProcThrottle(100)
}

func (b *WindowsBackend) ApplyModeExtreme() {
	setWinProcThrottle(1) // 1% limits aggressively on Windows
	b.SetLCDBrightness(10)
}

func (b *WindowsBackend) ApplyModeRestore() {
	setWinProcThrottle(100)
}

// Daemon State
var daemonWinRunning bool
var daemonWinQuit chan struct{}
var daemonWinMutex sync.Mutex

func (b *WindowsBackend) StopDaemon() {
	daemonWinMutex.Lock()
	defer daemonWinMutex.Unlock()
	if daemonWinRunning && daemonWinQuit != nil {
		close(daemonWinQuit)
		daemonWinRunning = false
		daemonWinQuit = nil
	}
}

// getWinLoad returns an approximation of CPU load 0.0 to 1.0
func getWinLoad() float64 {
	out := runWinCmd("typeperf", `\Processor Information(_Total)\% Processor Time`, "-sc", "1")
	lines := strings.Split(out, "\n")
	if len(lines) >= 3 {
		parts := strings.Split(lines[2], ",")
		if len(parts) >= 2 {
			valStr := strings.Trim(parts[1], `" `)
			if v, err := strconv.ParseFloat(valStr, 64); err == nil {
				return v / 100.0
			}
		}
	}
	return 0.0
}

func (b *WindowsBackend) StartAutoExtremeDaemon() {
	b.StopDaemon()
	
	daemonWinMutex.Lock()
	daemonWinQuit = make(chan struct{})
	daemonWinRunning = true
	daemonWinMutex.Unlock()

	go func() {
		ticker := time.NewTicker(10 * time.Second)
		defer ticker.Stop()

		applyLogic := func() {
			if b.IsCharging() {
				b.ApplyModePerformance()
			} else {
				powerLevel := getWinLoad()
				if powerLevel > 1.0 { powerLevel = 1.0 }
				
				discretePower := math.Round(powerLevel * 3) / 3.0
				
				// CPU scales 1% to 100% based on load
				minPercent := 1.0
				maxPercent := 100.0
				targetPercent := int(minPercent + discretePower*(maxPercent-minPercent))
				
				setWinProcThrottle(targetPercent)
				
				minB := 10
				maxB := 30
				targetBrightness := int(float64(minB) + discretePower*float64(maxB-minB))
				b.SetLCDBrightness(targetBrightness)
			}
		}

		applyLogic()

		for {
			select {
			case <-ticker.C:
				applyLogic()
			case <-daemonWinQuit:
				return
			}
		}
	}()
}
