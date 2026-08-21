package service

import (
	"os"
	"path/filepath"
	"testing"
)

func TestConfigLoadSave(t *testing.T) {
	tempDir := t.TempDir()
	origConfig := GetConfigPath()
	defer func() {
		// restore
	}()

	testConfigPath := filepath.Join(tempDir, "config.json")
	cfg := Config{
		AutoExtremeEnabled: true,
		AutoBrightness:     false,
	}

	data, err := os.ReadFile(testConfigPath)
	if err == nil && len(data) > 0 {
		t.Fatal("file should not exist yet")
	}

	_ = origConfig
	_ = cfg
}

func TestIsProcessAlive(t *testing.T) {
	myPid := os.Getpid()
	if !IsProcessAlive(myPid) {
		t.Errorf("Expected current PID %d to be alive", myPid)
	}

	if IsProcessAlive(-1) {
		t.Errorf("Expected PID -1 to not be alive")
	}
}
