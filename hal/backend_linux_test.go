package hal

import (
	"testing"
)

func TestLinuxBackend_GetOS(t *testing.T) {
	backend := &LinuxBackend{}
	if backend.GetOS() != "Linux" {
		t.Errorf("Expected OS to be 'Linux', got '%s'", backend.GetOS())
	}
}

func TestLinuxBackend_GetNumCPUs(t *testing.T) {
	backend := &LinuxBackend{}
	cpus := backend.GetNumCPUs()
	if cpus <= 0 {
		t.Errorf("Expected CPU count to be greater than 0, got %d", cpus)
	}
}

func TestLinuxBackend_GetCores(t *testing.T) {
	backend := &LinuxBackend{}
	cores := backend.GetCores()
	if cores <= 0 {
		t.Errorf("Expected cores count to be greater than 0, got %d", cores)
	}
}

func TestLinuxBackend_GetTurbo(t *testing.T) {
	backend := &LinuxBackend{}
	// Just verify it doesn't panic
	_ = backend.GetTurbo()
}

func TestLinuxBackend_GetASPM(t *testing.T) {
	backend := &LinuxBackend{}
	// Just verify it doesn't panic
	_ = backend.GetASPM()
}

func TestLinuxBackend_GetAudioPowerSave(t *testing.T) {
	backend := &LinuxBackend{}
	// Just verify it doesn't panic
	_ = backend.GetAudioPowerSave()
}

func TestLinuxBackend_GetGPUBounds(t *testing.T) {
	backend := &LinuxBackend{}
	minMhz, maxMhz := backend.GetGPUBounds()
	if minMhz < 0 || maxMhz < 0 {
		t.Errorf("Expected non-negative GPU bounds, got min=%d, max=%d", minMhz, maxMhz)
	}
	if minMhz > maxMhz {
		t.Errorf("Expected minMhz <= maxMhz, got min=%d, max=%d", minMhz, maxMhz)
	}
}

func TestLinuxBackend_AutoBrightness(t *testing.T) {
	backend := &LinuxBackend{}
	backend.SetAutoBrightness(true)
	if !backend.GetAutoBrightness() {
		t.Errorf("Expected auto brightness to be true")
	}
	backend.SetAutoBrightness(false)
	if backend.GetAutoBrightness() {
		t.Errorf("Expected auto brightness to be false")
	}
	backend.SetAutoBrightness(true)
}

func TestLinuxBackend_IsDaemonRunning(t *testing.T) {
	backend := &LinuxBackend{}
	backend.StopDaemon()
	if backend.IsDaemonRunning() {
		t.Errorf("Expected daemon to not be running")
	}
}

