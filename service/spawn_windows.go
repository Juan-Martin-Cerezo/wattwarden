//go:build windows

package service

import (
	"os"
	"os/exec"
	"syscall"
)

// SpawnDetachedDaemon launches a daemon process detached from the controlling console on Windows
func SpawnDetachedDaemon() error {
	exePath, err := os.Executable()
	if err != nil {
		exePath = "wattwarden.exe"
	}

	cmd := exec.Command(exePath, "--daemon")
	cmd.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP | 0x08000000, // CREATE_NO_WINDOW
	}

	return cmd.Start()
}
