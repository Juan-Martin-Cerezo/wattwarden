package service

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"wattwarden/hal"
)

// Config represents persistent settings for WattWarden
type Config struct {
	AutoExtremeEnabled bool `json:"auto_extreme_enabled"`
	AutoBrightness     bool `json:"auto_brightness"`
}

// GetConfigPath returns the cross-platform path for the configuration file
func GetConfigPath() string {
	if runtime.GOOS == "windows" {
		programData := os.Getenv("ProgramData")
		if programData == "" {
			programData = "C:\\ProgramData"
		}
		return filepath.Join(programData, "wattwarden", "config.json")
	}
	return "/etc/wattwarden/config.json"
}

// GetPIDPath returns the path to the daemon PID file
func GetPIDPath() string {
	if runtime.GOOS == "windows" {
		programData := os.Getenv("ProgramData")
		if programData == "" {
			programData = "C:\\ProgramData"
		}
		return filepath.Join(programData, "wattwarden", "wattwarden.pid")
	}
	return "/var/run/wattwarden.pid"
}

// LoadConfig reads the configuration from disk, with default fallbacks
func LoadConfig() Config {
	cfg := Config{
		AutoExtremeEnabled: true,
		AutoBrightness:     true,
	}
	data, err := os.ReadFile(GetConfigPath())
	if err == nil {
		_ = json.Unmarshal(data, &cfg)
	}
	return cfg
}

// SaveConfig writes the configuration to disk atomically
func SaveConfig(cfg Config) error {
	p := GetConfigPath()
	if err := os.MkdirAll(filepath.Dir(p), 0755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(p, data, 0644)
}

// IsProcessAlive checks if a process with the given PID is currently running
func IsProcessAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	process, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	if runtime.GOOS == "windows" {
		return true // FindProcess on Windows verifies process existence
	}
	// On Unix, sending signal 0 checks for process existence without killing it
	return process.Signal(syscall.Signal(0)) == nil
}

// IsDaemonActive checks if the WattWarden daemon is running in the background
func IsDaemonActive() bool {
	pidPath := GetPIDPath()
	data, err := os.ReadFile(pidPath)
	if err == nil {
		pidStr := strings.TrimSpace(string(data))
		if pid, err := strconv.Atoi(pidStr); err == nil {
			if IsProcessAlive(pid) {
				return true
			}
		}
	}

	if runtime.GOOS == "linux" {
		out, err := exec.Command("systemctl", "is-active", "wattwarden.service").Output()
		if err == nil && strings.TrimSpace(string(out)) == "active" {
			return true
		}
	}

	return false
}

// RunDaemon runs the daemon blocking loop for systemd, launchd, or background runner
func RunDaemon(b hal.Backend) {
	// Write current PID to PID file
	pidPath := GetPIDPath()
	_ = os.MkdirAll(filepath.Dir(pidPath), 0755)
	_ = os.WriteFile(pidPath, []byte(strconv.Itoa(os.Getpid())), 0644)
	defer os.Remove(pidPath)

	cfg := LoadConfig()
	b.SetAutoBrightness(cfg.AutoBrightness)
	b.StartAutoExtremeDaemon()

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM, syscall.SIGHUP)
	<-sigChan

	b.StopDaemon()
}

// StartBackgroundDaemon ensures the daemon is started in background
func StartBackgroundDaemon(b hal.Backend) error {
	cfg := LoadConfig()
	cfg.AutoExtremeEnabled = true
	_ = SaveConfig(cfg)

	// If systemd is available and service is installed, use systemctl
	if runtime.GOOS == "linux" {
		if _, err := os.Stat("/etc/systemd/system/wattwarden.service"); err == nil {
			_ = exec.Command("systemctl", "restart", "wattwarden.service").Run()
			if IsDaemonActive() {
				return nil
			}
		}
	} else if runtime.GOOS == "darwin" {
		if _, err := os.Stat("/Library/LaunchDaemons/com.wattwarden.daemon.plist"); err == nil {
			_ = exec.Command("launchctl", "start", "com.wattwarden.daemon").Run()
			if IsDaemonActive() {
				return nil
			}
		}
	}

	// Also start in-process daemon
	if !b.IsDaemonRunning() {
		b.StartAutoExtremeDaemon()
	}

	// Otherwise spawn detached background daemon process
	if !IsDaemonActive() {
		_ = SpawnDetachedDaemon()
	}
	return nil
}

// StopBackgroundDaemon stops the daemon locally and any background service/process
func StopBackgroundDaemon(b hal.Backend) {
	cfg := LoadConfig()
	cfg.AutoExtremeEnabled = false
	_ = SaveConfig(cfg)

	b.StopDaemon()

	if runtime.GOOS == "linux" {
		_ = exec.Command("systemctl", "stop", "wattwarden.service").Run()
	} else if runtime.GOOS == "darwin" {
		_ = exec.Command("launchctl", "stop", "com.wattwarden.daemon").Run()
	}

	pidPath := GetPIDPath()
	data, err := os.ReadFile(pidPath)
	if err == nil {
		pidStr := strings.TrimSpace(string(data))
		if pid, err := strconv.Atoi(pidStr); err == nil {
			if proc, err := os.FindProcess(pid); err == nil {
				_ = proc.Signal(syscall.SIGTERM)
			}
		}
		_ = os.Remove(pidPath)
	}
}

// InstallService installs the auto-start background service for the current OS
func InstallService() error {
	exePath, err := os.Executable()
	if err != nil {
		exePath = "/usr/local/bin/wattwarden"
	}

	cfg := LoadConfig()
	_ = SaveConfig(cfg)

	switch runtime.GOOS {
	case "linux":
		serviceContent := fmt.Sprintf(`[Unit]
Description=WattWarden Auto Power and Hardware Management Daemon
After=multi-user.target

[Service]
Type=simple
ExecStart=%s --daemon
Restart=always
RestartSec=3
KillMode=process

[Install]
WantedBy=multi-user.target
`, exePath)

		if err := os.WriteFile("/etc/systemd/system/wattwarden.service", []byte(serviceContent), 0644); err != nil {
			return err
		}
		_ = exec.Command("systemctl", "daemon-reload").Run()
		_ = exec.Command("systemctl", "enable", "--now", "wattwarden.service").Run()
		return nil

	case "darwin":
		plistContent := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.wattwarden.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>%s</string>
        <string>--daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardErrorPath</key>
    <string>/var/log/wattwarden.log</string>
    <key>StandardOutPath</key>
    <string>/var/log/wattwarden.log</string>
</dict>
</plist>
`, exePath)

		plistPath := "/Library/LaunchDaemons/com.wattwarden.daemon.plist"
		if err := os.WriteFile(plistPath, []byte(plistContent), 0644); err != nil {
			return err
		}
		_ = exec.Command("launchctl", "load", "-w", plistPath).Run()
		return nil

	case "windows":
		_ = exec.Command("schtasks", "/Create", "/TN", "WattWardenDaemon", "/TR", fmt.Sprintf("\"%s\" --daemon", exePath), "/SC", "ONSTART", "/RU", "SYSTEM", "/RL", "HIGHEST", "/F").Run()
		_ = exec.Command("schtasks", "/Run", "/TN", "WattWardenDaemon").Run()
		return nil
	}

	return fmt.Errorf("unsupported operating system: %s", runtime.GOOS)
}

// UninstallService removes the background service for the current OS
func UninstallService() error {
	switch runtime.GOOS {
	case "linux":
		_ = exec.Command("systemctl", "disable", "--now", "wattwarden.service").Run()
		_ = os.Remove("/etc/systemd/system/wattwarden.service")
		_ = exec.Command("systemctl", "daemon-reload").Run()
		return nil
	case "darwin":
		plistPath := "/Library/LaunchDaemons/com.wattwarden.daemon.plist"
		_ = exec.Command("launchctl", "unload", "-w", plistPath).Run()
		_ = os.Remove(plistPath)
		return nil
	case "windows":
		_ = exec.Command("schtasks", "/Delete", "/TN", "WattWardenDaemon", "/F").Run()
		return nil
	}
	return nil
}
