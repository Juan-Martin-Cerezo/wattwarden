package main // Defines this as the main executable package

import ( // Import standard library and project packages
	"fmt" // Used for formatted I/O like printing errors to the console
	"os"  // Used for OS-level operations like getting UID and exiting
	"wattwarden/hal" // Imports the Hardware Abstraction Layer for OS-agnostic power management
	"wattwarden/ui"  // Imports the user interface package to render the TUI
)

func main() { // The entry point of the WattWarden application
	if hal.CurrentBackend == nil { // Checks if the OS was recognized and a backend was loaded
		fmt.Println("Error: No backend implementation available for this OS.") // Prints an error if the OS is unsupported
		return // Exits the program safely without panicking
	}
	
	if !hasPrivileges() { // Checks whether the process has the required system privileges
		fmt.Println("Error: You must run this program with administrator/root privileges to change system power settings.") // Explains why elevated privileges are required
		os.Exit(1) // Exits the program with a non-zero status indicating an error
	}
	
	if os.Getenv("WATTWARDEN_DAEMON") == "1" || (len(os.Args) > 1 && os.Args[1] == "--daemon") { // Checks if the program was launched in daemon mode via Env Var or CLI argument
		return // Exits the main function since the daemon blocks or runs until killed
	}
	
	ui.StartDashboard(hal.CurrentBackend) // If no daemon mode was requested, starts the interactive TUI Dashboard with the loaded hardware backend
}
