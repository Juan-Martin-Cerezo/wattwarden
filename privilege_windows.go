//go:build windows

package main

import "golang.org/x/sys/windows"

func hasPrivileges() bool {
	return windows.GetCurrentProcessToken().IsElevated()
}
