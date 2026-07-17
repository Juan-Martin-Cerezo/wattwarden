package main // Defines this as the main executable package

import ( // Import standard library and project packages
	"fmt" // Used for formatted I/O like printing errors to the console
	"os"  // Used for OS-level operations like getting UID and exiting
	"volttamer/hal" // Imports the Hardware Abstraction Layer for OS-agnostic power management
	"volttamer/ui"  // Imports the user interface package to render the TUI
)

func main() { // The entry point of the VoltTamer application
	if hal.CurrentBackend == nil { // Checks if the OS was recognized and a backend was loaded
		fmt.Println("Error: No backend implementation available for this OS.") // Prints an error if the OS is unsupported
		return // Exits the program safely without panicking
	}
	
	if os.Geteuid() != 0 { // Checks the Effective User ID to verify if the user is root (Administrator)
		fmt.Println("Error: You must run this program as administrator (root/sudo) to be able to change system frequencies and parameters!") // Explains why root is needed in English
		os.Exit(1) // Exits the program with a non-zero status indicating an error
	}
	
	if os.Getenv("VOLTTAMER_DAEMON") == "1" || (len(os.Args) > 1 && os.Args[1] == "--daemon") { // Checks if the program was launched in daemon mode via Env Var or CLI argument
		return // Exits the main function since the daemon blocks or runs until killed
	}
	
	ui.StartDashboard(hal.CurrentBackend) // If no daemon mode was requested, starts the interactive TUI Dashboard with the loaded hardware backend
}
