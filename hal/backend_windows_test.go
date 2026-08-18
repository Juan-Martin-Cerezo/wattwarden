//go:build windows

package hal

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestWindowsBackendNativeCommands(t *testing.T) {
	dir := t.TempDir()
	logPath := filepath.Join(dir, "commands.log")
	powershell := "@echo off\necho powershell %* >> %WATTWARDEN_TEST_LOG%\necho 50\n"
	powercfg := "@echo off\necho powercfg %* >> %WATTWARDEN_TEST_LOG%\n"
	for name, content := range map[string]string{"powershell.cmd": powershell, "powercfg.cmd": powercfg} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0755); err != nil {
			t.Fatal(err)
		}
	}
	t.Setenv("PATH", dir+";"+os.Getenv("PATH"))
	t.Setenv("WATTWARDEN_TEST_LOG", logPath)

	backend := &WindowsBackend{}
	if backend.GetLCDBrightness() != 50 {
		t.Fatalf("brightness did not come from PowerShell")
	}
	backend.ApplyModeExtreme()
	backend.ApplyModeRestore()
	log, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(log), "powercfg") {
		t.Fatalf("powercfg was not invoked: %s", log)
	}
}
