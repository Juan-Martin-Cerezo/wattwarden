//go:build !windows

package main

import "os"

func hasPrivileges() bool {
	return os.Geteuid() == 0
}
