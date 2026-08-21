//go:build !windows

package service

import (
	"os"
	"os/exec"
	"syscall"
)

// SpawnDetachedDaemon launches a daemon process detached from the controlling terminal
func SpawnDetachedDaemon() error {
	exePath, err := os.Executable()
	if err != nil {
		exePath = "/usr/local/bin/wattwarden"
	}

	cmd := exec.Command(exePath, "--daemon")
	cmd.SysProcAttr = &syscall.SysProcAttr{
		Setsid: true, // Creates a new session so closing terminal does not kill it
	}
	// Redirect stdout/stderr to log file
	logFile, err := os.OpenFile("/var/log/wattwarden.log", os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err == nil {
		cmd.Stdout = logFile
		cmd.Stderr = logFile
	}

	return cmd.Start()
}
