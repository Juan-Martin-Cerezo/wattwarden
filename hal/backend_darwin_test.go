//go:build darwin

package hal

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDarwinBackendNativeCommands(t *testing.T) {
	dir := t.TempDir()
	logPath := filepath.Join(dir, "commands.log")
	pmset := "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$WATTWARDEN_TEST_LOG\"\nif [ \"$1\" = \"-g\" ]; then printf '%s\\n' \"Now drawing from 'Battery Power'\" ' -InternalBattery-0 (id=1) 82%; discharging; 4:12 remaining; present: true'\nfi\n"

ioreg := "#!/bin/sh\nprintf '%s\\n' '\"Current\" = 1000' '\"Voltage\" = 12000'\n"
	purge := "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$WATTWARDEN_TEST_LOG\"\n"
	for name, content := range map[string]string{"pmset": pmset, "ioreg": ioreg, "purge": purge} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0755); err != nil {
			t.Fatal(err)
		}
	}
	t.Setenv("PATH", dir+":"+os.Getenv("PATH"))
	t.Setenv("WATTWARDEN_TEST_LOG", logPath)

	backend := &DarwinBackend{}
	if backend.GetBatteryPercentage() != 82 {
		t.Fatalf("battery percentage did not come from pmset")
	}
	if backend.IsCharging() {
		t.Fatalf("discharging pmset status reported charging")
	}
	if backend.GetBatteryTime() != "4:12" {
		t.Fatalf("battery time did not come from pmset")
	}
	if backend.GetPowerConsumptionWatts() != 12 {
		t.Fatalf("power consumption did not come from ioreg")
	}
	backend.ApplyModeExtreme()
	backend.ProcessPurge()
	log, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatal(err)
	}
	output := string(log)
	if !strings.Contains(output, "pmset -a lowpowermode 1") || !strings.Contains(output, "purge") {
		t.Fatalf("native commands were not invoked: %s", output)
	}
}
