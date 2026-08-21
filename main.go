package main // Defines this as the main executable package

import ( // Import standard library and project packages
	"fmt" // Used for formatted I/O like printing errors to the console
	"os"  // Used for OS-level operations like getting UID and exiting
	"wattwarden/hal" // Imports the Hardware Abstraction Layer for OS-agnostic power management
	"wattwarden/service" // Imports background service and daemon management
	"wattwarden/ui"  // Imports the user interface package to render the TUI
)

func main() { // The entry point of the WattWarden application
	if hal.CurrentBackend == nil { // Checks if the OS was recognized and a backend was loaded
		fmt.Println("Error: No backend implementation available for this OS.")
		return
	}

	if len(os.Args) > 1 {
		switch os.Args[1] {
		case "--daemon", "daemon":
			if !hasPrivileges() {
				fmt.Println("Error: Administrator/root privileges are required to run the daemon.")
				os.Exit(1)
			}
			service.RunDaemon(hal.CurrentBackend)
			return

		case "--install-service", "install-service":
			if !hasPrivileges() {
				fmt.Println("Error: Administrator/root privileges are required to install the service.")
				os.Exit(1)
			}
			if err := service.InstallService(); err != nil {
				fmt.Printf("Error installing service: %v\n", err)
				os.Exit(1)
			}
			fmt.Println("✅ WattWarden background service installed and started successfully.")
			return

		case "--uninstall-service", "uninstall-service":
			if !hasPrivileges() {
				fmt.Println("Error: Administrator/root privileges are required to uninstall the service.")
				os.Exit(1)
			}
			if err := service.UninstallService(); err != nil {
				fmt.Printf("Error uninstalling service: %v\n", err)
				os.Exit(1)
			}
			fmt.Println("✅ WattWarden background service uninstalled.")
			return

		case "--start", "start":
			if !hasPrivileges() {
				fmt.Println("Error: Administrator/root privileges are required.")
				os.Exit(1)
			}
			if err := service.StartBackgroundDaemon(hal.CurrentBackend); err != nil {
				fmt.Printf("Error starting daemon: %v\n", err)
				os.Exit(1)
			}
			fmt.Println("⚡ WattWarden background daemon started.")
			return

		case "--stop", "stop":
			if !hasPrivileges() {
				fmt.Println("Error: Administrator/root privileges are required.")
				os.Exit(1)
			}
			service.StopBackgroundDaemon(hal.CurrentBackend)
			fmt.Println("🛑 WattWarden background daemon stopped.")
			return

		case "--status", "status":
			if service.IsDaemonActive() {
				fmt.Println("WattWarden Daemon Status: [ACTIVE] (Running in background)")
			} else {
				fmt.Println("WattWarden Daemon Status: [INACTIVE]")
			}
			return

		case "--brightness":
			if len(os.Args) > 2 {
				enabled := os.Args[2] == "on" || os.Args[2] == "1" || os.Args[2] == "true"
				hal.CurrentBackend.SetAutoBrightness(enabled)
				cfg := service.LoadConfig()
				cfg.AutoBrightness = enabled
				_ = service.SaveConfig(cfg)
				fmt.Printf("Auto-brightness set to: %v\n", enabled)
				return
			}
			fmt.Printf("Auto-brightness is currently: %v\n", hal.CurrentBackend.GetAutoBrightness())
			return

		case "--help", "-h", "help":
			fmt.Println("⚡ WattWarden - Hardware Power & Auto-Brightness Management")
			fmt.Println("\nUsage:")
			fmt.Println("  sudo wattwarden                   Launch interactive TUI Dashboard")
			fmt.Println("  sudo wattwarden --daemon          Run background auto-tuning daemon in foreground")
			fmt.Println("  sudo wattwarden --start           Start background daemon (persists after closing terminal)")
			fmt.Println("  sudo wattwarden --stop            Stop background daemon")
			fmt.Println("  sudo wattwarden --status          Check background daemon status")
			fmt.Println("  sudo wattwarden --brightness <on|off> Configure auto-brightness setting")
			fmt.Println("  sudo wattwarden --install-service Install & enable auto-start system service")
			fmt.Println("  sudo wattwarden --uninstall-service Remove system service")
			return
		}
	}

	if !hasPrivileges() { // Checks whether the process has the required system privileges
		fmt.Println("Error: You must run this program with administrator/root privileges to change system power settings.")
		os.Exit(1)
	}

	if os.Getenv("WATTWARDEN_DAEMON") == "1" {
		service.RunDaemon(hal.CurrentBackend)
		return
	}

	ui.StartDashboard(hal.CurrentBackend) // Starts interactive TUI Dashboard
}
