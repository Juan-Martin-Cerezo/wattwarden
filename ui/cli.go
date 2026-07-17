package ui // Package ui handles the Terminal User Interface

import ( // Standard library imports
	"fmt" // Format strings and print to screen
	"os" // OS functions, though largely unused here, could be for exit
	"strings" // String manipulations (like repeat, split, etc)
	"time" // Time handling for sleep and intervals

	"wattwarden/hal" // Our own Hardware Abstraction Layer

	"github.com/gdamore/tcell/v2" // The tcell library used to draw to the terminal
)

// MenuItem represents a single selectable or unselectable row in the menu
type MenuItem struct {
	IsHeader bool // If true, this is a category title, not a selectable item
	Name     string // The text displayed for this item
	GetVal   func(hal.Backend) string // Function to read the current value from the hardware
	Action   func(hal.Backend, *Dashboard) // Function to run when the user presses Enter
	Inc      func(hal.Backend, *Dashboard) // Function to run when the user presses Right (Increase)
	Dec      func(hal.Backend, *Dashboard) // Function to run when the user presses Left (Decrease)
}

// Dashboard is the main state struct holding all UI information
type Dashboard struct {
	backend        hal.Backend // Pointer to the OS-specific hardware controller
	screen         tcell.Screen // The tcell screen buffer we draw on
	history        []float64 // Array of past power consumption values for the graph
	selected       int // Index of the currently highlighted menu item
	scrollOffset   int // The current vertical scroll position of the menu
	refreshDelay   time.Duration // How often the screen redraws (adjustable)
	quit           chan struct{} // Channel to signal the program to exit
	confirmExtreme bool // State flag for when the Extreme Mode confirmation prompt is open
	toastMsg       string // The text of the current pop-up toast message
	toastExpiry    time.Time // When the toast message should disappear
	items          []MenuItem // The full list of menu items to display
}

// showToast queues a temporary message at the bottom of the screen
func (d *Dashboard) showToast(msg string) {
	d.toastMsg = msg // Set the text
	d.toastExpiry = time.Now().Add(3 * time.Second) // Make it expire 3 seconds from now
}

// drawString is a helper to draw a string of text at specific X,Y coordinates
func (d *Dashboard) drawString(x, y int, style tcell.Style, str string) {
	i := 0 // Column offset
	for _, c := range str { // Iterate over every character
		d.screen.SetContent(x+i, y, c, nil, style) // Draw the character on the screen buffer
		if c != '\uFE0F' { // Ignore zero-width emoji variation selectors
			i++ // Advance to the next column
		}
	}
}

// blocks are Unicode characters used to draw varying heights in the graph
var blocks = []rune{' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'}

// drawBarGraph renders the historical power consumption as an ASCII block graph
func (d *Dashboard) drawBarGraph(startY, startX, height, width int, dataList []float64, maxValLabel float64) {
	if len(dataList) == 0 { return }

	graphMax := 15.0 // scale max
	
	// Draw axes
	for yOffset := 0; yOffset < height; yOffset++ {
		screenY := startY + height - 1 - yOffset
		d.drawString(startX, screenY, tcell.StyleDefault.Foreground(tcell.ColorDarkGray), "│")
	}
	d.drawString(startX, startY+height, tcell.StyleDefault.Foreground(tcell.ColorDarkGray), "└" + strings.Repeat("─", width-14))

	// Plot data
	for i := 0; i < width-14; i++ {
		dataIdx := len(dataList) - (width - 14) + i
		if dataIdx < 0 || dataIdx >= len(dataList) { continue }
		
		val := dataList[dataIdx]
		xPos := startX + 1 + i
		
		// Map value to row heights
		ratio := val / graphMax
		if ratio > 1.0 { ratio = 1.0 }
		
		totalDots := int(ratio * float64(height*8)) // 8 block levels per row
		
		for yOffset := 0; yOffset < height; yOffset++ {
			screenY := startY + height - 1 - yOffset
			dotsInRow := totalDots - (yOffset * 8)
			charIdx := 0
			if dotsInRow >= 8 {
				charIdx = 8
			} else if dotsInRow > 0 {
				charIdx = dotsInRow
			}
			
			if charIdx > 0 {
				color := tcell.ColorGreen
				if yOffset > height/2 { color = tcell.ColorYellow }
				if yOffset > height-2 { color = tcell.ColorRed }
				d.screen.SetContent(xPos, screenY, blocks[charIdx], nil, tcell.StyleDefault.Foreground(color))
			}
		}
	}

	stepVal := 1.0
	if height > 1 {
		stepVal = graphMax / float64(height-1)
	}
	
	for yOffset := 0; yOffset < height; yOffset++ {
		screenY := startY + height - 1 - yOffset
		valLabel := float64(yOffset) * stepVal
		
		color := tcell.ColorGreen
		if valLabel >= 12.0 {
			color = tcell.ColorRed
		} else if valLabel >= 6.0 {
			color = tcell.ColorYellow
		}
		
		d.drawString(startX+width-12, screenY, tcell.StyleDefault.Foreground(tcell.ColorGray), "│ ")
		d.drawString(startX+width-10, screenY, tcell.StyleDefault.Foreground(color), fmt.Sprintf("%4.1f W", valLabel))
	}
}

func (d *Dashboard) drawUI() {

	w, h := d.screen.Size() // Get current terminal size
	d.screen.Clear() // Clear the entire screen buffer for the new frame

	if w < 70 || h < 20 { // Check minimum window size
		msg := "PLEASE RESIZE TERMINAL (Minimum size: 70x20)" // Error message
		d.drawString((w-len([]rune(msg)))/2, h/2, tcell.StyleDefault.Foreground(tcell.ColorRed).Bold(true), msg) // Center error in red
		d.screen.Show() // Render and stop
		return
	}

	defStyle := tcell.StyleDefault // Normal text
	titleStyle := tcell.StyleDefault.Foreground(tcell.ColorAqua).Bold(true) // Bright blue title
	headerStyle := tcell.StyleDefault.Foreground(tcell.ColorYellow).Bold(true) // Yellow category headers

	asciiArt := []string{ // The top logo
		` __        ___  _____ _______        ___    ____  ____  _____ _   _ `,
		` \ \      / / \|_   _|_   _\ \      / / \  |  _ \|  _ \| ____| \ | |`,
		`  \ \ /\ / / _ \ | |   | |  \ \ /\ / / _ \ | |_) | | | |  _| |  \| |`,
		`   \ V  V / ___ \| |   | |   \ V  V / ___ \|  _ <| |_| | |___| |\  |`,
		`    \_/\_/_/   \_\_|   |_|    \_/\_/_/   \_\_| \_\____/|_____|_| \_|`,
		`                                                                    `,
	}
	
	artY := 1 // Top margin
	artX := (w - 68) / 2 // Center horizontally (68 is logo width)
	if artX < 1 { artX = 1 } // Prevent drawing off-screen
	
	for i, line := range asciiArt { // Draw each line of the logo
		d.drawString(artX, artY+i, titleStyle, line)
	}
	
	infoY := artY + len(asciiArt) + 1 // Position information row below logo

	statusStr := "Discharging" // Default state
	if d.backend.IsCharging() { // Check actual state
		statusStr = "Charging"
	}
	
	battTime := d.backend.GetBatteryTime() // Get battery time string
	totalW := d.backend.GetPowerConsumptionWatts() // Get current watts
	
	// Create the information summary line
	infoStr := fmt.Sprintf("OS: %s | Battery: %d%% (%s) | Est: %s | Power: %.1fW", 
		d.backend.GetOS(), d.backend.GetBatteryPercentage(), statusStr, battTime, totalW)
	
	// Draw the summary centered
	d.drawString((w-len(infoStr))/2, infoY, tcell.StyleDefault.Bold(true), infoStr)

	if totalW > 0 { // Only add non-zero readings to history
		d.history = append(d.history, totalW) // Append reading
		if len(d.history) > 400 { // Max history buffer size
			d.history = d.history[len(d.history)-400:] // Trim oldest
		}
	}
	
	isHorizontal := w >= 130 // Threshold for side-by-side mode vs vertical mode
	var graphY, graphX, graphW, graphH int // Variables to hold graph dimensions
	var menuY, menuX, menuW int // Variables to hold menu dimensions

	if isHorizontal { // Wide terminal (Side-by-side layout)
		menuX = 4 // Left padding
		menuY = infoY + 2 // Space below info line
		menuW = 50 // Fixed width for menu
		
		graphX = menuX + menuW + 4 // Position graph to the right of the menu
		graphY = infoY + 2 // Same top level as menu
		graphW = w - graphX - 4 // Graph takes remaining width
		graphH = h - graphY - 4 // Stretch graph to the bottom of terminal
	} else { // Narrow terminal (Stacked layout)
		graphX = 4 // Left padding
		graphY = infoY + 2 // Space below info
		graphW = w - 8 // Full width minus padding
		
		availableForRest := h - graphY - 4 // Calculate remaining vertical space
		neededForMenu := len(d.items) // Height required for all menu items
		
		if availableForRest > neededForMenu+10 { // If terminal is very tall
			graphH = availableForRest - neededForMenu // Give extra space to graph
			if graphH > 15 { graphH = 15 } // But cap graph height at 15
		} else { // If terminal is normal/short
			graphH = 8 // Fixed short graph
		}
		
		menuX = 4 // Left padding
		menuY = graphY + graphH + 2 // Place menu below graph
		menuW = w - 8 // Full width minus padding
	}

	d.drawBarGraph(graphY, graphX, graphH, graphW, d.history, 15.0) // Draw the graph

	visibleItems := h - menuY - 4 // Calculate how many menu items fit on screen
	if visibleItems < 1 { // Prevent negative/zero
		visibleItems = 1
	}

	if d.selected < d.scrollOffset { // If cursor moves above visible area
		d.scrollOffset = d.selected // Scroll up
	}
	if d.selected >= d.scrollOffset + visibleItems { // If cursor moves below visible area
		d.scrollOffset = d.selected - visibleItems + 1 // Scroll down
	}

	for i := 0; i < visibleItems; i++ { // Draw each visible menu item
		idx := d.scrollOffset + i // Calculate true index in the full list
		if idx >= len(d.items) { // Stop if we run out of items
			break
		}
		
		opt := d.items[idx] // Get the item
		y := menuY + i // Calculate Y coordinate
		
		if opt.IsHeader { // If it's a category title
			d.drawString(menuX, y, headerStyle, opt.Name) // Draw yellow
			continue
		}
		
		valStr := "" // Hold the string value of the setting
		if opt.GetVal != nil { // If it has a reader function
			valStr = opt.GetVal(d.backend) // Fetch hardware value
		}
		
		displayVal := fmt.Sprintf("[%s]", valStr) // Format value
		if valStr == "true" { displayVal = "[ACTIVE]" } // Make booleans look better
		if valStr == "false" { displayVal = "[OFF]" }

		nameStr := opt.Name // Item name
		maxNameLen := menuW - 20 // Space left after cursor (3), display val (15) and spacing
		if maxNameLen < 5 { maxNameLen = 5 } // Minimum safe size
		if len(nameStr) > maxNameLen { // If name is too long for the layout
			nameStr = nameStr[:maxNameLen-3] + "..." // Truncate and add ellipsis
		}

		if idx == d.selected { // If this is the currently selected item
			// Draw inverted background to show cursor
			d.drawString(menuX, y, tcell.StyleDefault.Foreground(tcell.ColorBlue).Background(tcell.ColorWhite).Bold(true), fmt.Sprintf(" > %-*s %15s ", maxNameLen, nameStr, displayVal))
		} else { // Normal item
			d.drawString(menuX, y, defStyle, fmt.Sprintf("   %-*s %15s ", maxNameLen, nameStr, displayVal))
		}
	}

	// Visible Scrollbar Track
	if visibleItems < len(d.items) { // If there are more items than fit on screen
		barX := menuX + menuW // Place on the right edge of the menu area
		for r := 0; r < visibleItems; r++ { // Draw the track
			d.drawString(barX, menuY+r, tcell.StyleDefault.Foreground(tcell.ColorDarkGray), "│")
		}
		
		scrollbarHeight := (visibleItems * visibleItems) / len(d.items) // Proportional height
		if scrollbarHeight < 1 { scrollbarHeight = 1 } // Minimum 1 tall
		
		// Calculate position of the scroll thumb
		scrollbarPos := (d.scrollOffset * (visibleItems - scrollbarHeight)) / (len(d.items) - visibleItems)
		
		for r := 0; r < scrollbarHeight; r++ { // Draw the thumb block
			d.drawString(barX, menuY+scrollbarPos+r, tcell.StyleDefault.Foreground(tcell.ColorWhite), "█")
		}
	}

	// Scroll indicators (text pointers)
	if d.scrollOffset > 0 { // If there are items hidden above
		msg := " ▲ SCROLL UP FOR MORE OPTIONS ▲ "
		msgX := menuX + (menuW-len([]rune(msg)))/2 // Center horizontally
		if msgX < menuX { msgX = menuX }
		d.drawString(msgX, menuY-1, tcell.StyleDefault.Foreground(tcell.ColorYellow).Bold(true), msg)
	}
	if d.scrollOffset+visibleItems < len(d.items) { // If there are items hidden below
		msg := " ▼ SCROLL DOWN FOR MORE OPTIONS ▼ "
		msgX := menuX + (menuW-len([]rune(msg)))/2 // Center horizontally
		if msgX < menuX { msgX = menuX }
		d.drawString(msgX, menuY+visibleItems, tcell.StyleDefault.Foreground(tcell.ColorYellow).Bold(true), msg)
	}
	
	if time.Now().Before(d.toastExpiry) { // If a toast popup is currently active
		toastStyle := tcell.StyleDefault.Background(tcell.ColorYellow).Foreground(tcell.ColorBlack).Bold(true)
		toastX := w/2 - len(d.toastMsg)/2 // Center
		d.drawString(toastX, h-3, toastStyle, " "+d.toastMsg+" ") // Draw near bottom
	}

	if d.confirmExtreme { // If the extreme mode confirmation modal is open
		boxW := 62 // Width of modal
		boxH := 9 // Height of modal
		boxX := (w - boxW) / 2 // Center X
		boxY := (h - boxH) / 2 // Center Y
		
		borderStyle := tcell.StyleDefault.Foreground(tcell.ColorRed).Background(tcell.ColorBlack).Bold(true)
		textStyle := tcell.StyleDefault.Foreground(tcell.ColorWhite).Background(tcell.ColorBlack).Bold(true)
		warnStyle := tcell.StyleDefault.Foreground(tcell.ColorYellow).Background(tcell.ColorBlack).Bold(true)
		
		for r := 0; r < boxH; r++ { // Clear a black box
			d.drawString(boxX, boxY+r, tcell.StyleDefault.Background(tcell.ColorBlack), strings.Repeat(" ", boxW))
		}

		// Draw border box
		d.drawString(boxX, boxY, borderStyle, "╭"+strings.Repeat("─", boxW-2)+"╮")
		for r := 1; r < boxH-1; r++ {
			d.drawString(boxX, boxY+r, borderStyle, "│")
			d.drawString(boxX+boxW-1, boxY+r, borderStyle, "│")
		}
		d.drawString(boxX, boxY+boxH-1, borderStyle, "╰"+strings.Repeat("─", boxW-2)+"╯")

		// Text lines for modal
		lines := []struct{ y int; text string; style tcell.Style }{
			{1, "⚠️  WARNING: EXTREME MODE", warnStyle},
			{3, "This will minimize all hardware performance.", textStyle},
			{4, "Press 'R' at any time to restore normal operation.", tcell.StyleDefault.Foreground(tcell.ColorGray).Background(tcell.ColorBlack)},
			{6, "[ Y - Confirm ]    [ N - Cancel ]", warnStyle},
		}

		for _, l := range lines { // Draw text inside modal
			textX := boxX + (boxW - len([]rune(l.text))) / 2
			if l.y == 1 { textX += 1 }  // Adjust emoji
			d.drawString(textX, boxY+l.y, l.style, l.text)
		}
	}

	// Draw footer controls help
	d.drawString(2, h-1, tcell.StyleDefault.Foreground(tcell.ColorGray), "[UP/DOWN] Navigate | [L/R] Adjust | [ENTER] Apply | [R] Restore | [Q] Quit")
	d.screen.Show() // Execute the full redraw to the terminal
}

// buildMenuItems constructs the entire menu tree with hardware links
func buildMenuItems(d *Dashboard) []MenuItem {
	osName := d.backend.GetOS()
	
	items := []MenuItem{
		{IsHeader: true, Name: "─── [ PROFILES ] ───────────────────────"}, // Title
		// Performance Mode sets all settings to maximum power
		{Name: "⚡ Performance Mode", GetVal: func(b hal.Backend) string { return "EXECUTE" }, Action: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.ApplyModePerformance(); d.showToast("PERFORMANCE MODE ACTIVATED") }},
		// Extreme mode triggers the confirmation modal
		{Name: "🔋 Extreme Mode", GetVal: func(b hal.Backend) string { return "EXECUTE" }, Action: func(b hal.Backend, d *Dashboard) { d.confirmExtreme = true }},
		// Auto mode runs a background loop to manage power
		{Name: "⚡ Auto Extreme Mode", GetVal: func(b hal.Backend) string { return "EXECUTE" }, Action: func(b hal.Backend, d *Dashboard) { b.StartAutoExtremeDaemon(); d.showToast("AUTO EXTREME DAEMON STARTED") }},
		// Restore mode returns to normal state
		{Name: "♻  Restore Mode", GetVal: func(b hal.Backend) string { return "EXECUTE" }, Action: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.ApplyModeRestore(); d.showToast("RESTORE MODE ACTIVATED") }},
		{IsHeader: true, Name: ""}, // Empty spacer
	}

	if osName == "Linux" {
		items = append(items, []MenuItem{
			{IsHeader: true, Name: "─── [ HARDWARE LIMITS ] ────────────────"}, // CPU Section
			{Name: "Active Cores", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d / %d", b.GetCores(), b.GetNumCPUs()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetCores(b.GetCores()+1) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetCores(b.GetCores()-1) }},
			{Name: "CPU Freq (MHz)", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d", b.GetFreqLimit()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetFreqLimit(b.GetFreqLimit()+100) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetFreqLimit(b.GetFreqLimit()-100) }},
			{Name: "Freq iGPU (MHz)", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d", b.GetGPUFreq()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetGPUFreq(b.GetGPUFreq()+50) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetGPUFreq(b.GetGPUFreq()-50) }},
			{Name: "RAPL PL1 (W)", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d", b.GetRAPLPL1()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetRAPLPL1(b.GetRAPLPL1()+2) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetRAPLPL1(b.GetRAPLPL1()-2) }},
			{Name: "RAPL PL2 (W)", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d", b.GetRAPLPL2()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetRAPLPL2(b.GetRAPLPL2()+2) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetRAPLPL2(b.GetRAPLPL2()-2) }},
			{Name: "Turbo Boost", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetTurbo()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetTurbo(!b.GetTurbo()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetTurbo(!b.GetTurbo()) }},
			{Name: "Energy Perf Pref", GetVal: func(b hal.Backend) string { return b.GetEPP() }},
			{Name: "PCIe ASPM Policy", GetVal: func(b hal.Backend) string { return b.GetASPM() }},

			{IsHeader: true, Name: ""},
			{IsHeader: true, Name: "─── [ PERIPHERALS ] ────────────────────"}, // Hardware section
			{Name: "LCD Brightness (%)", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d", b.GetLCDBrightness()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetLCDBrightness(b.GetLCDBrightness()+5) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetLCDBrightness(b.GetLCDBrightness()-5) }},
			{Name: "Keyboard Light", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetKbdBacklight()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetKbdBacklight(!b.GetKbdBacklight()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetKbdBacklight(!b.GetKbdBacklight()) }},
			{Name: "Bluetooth", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetBluetooth()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetBluetooth(!b.GetBluetooth()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetBluetooth(!b.GetBluetooth()) }},
			{Name: "WiFi Enable", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetWifiEnable()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetWifiEnable(!b.GetWifiEnable()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetWifiEnable(!b.GetWifiEnable()) }},

			{IsHeader: true, Name: ""},
			{IsHeader: true, Name: "─── [ SYSTEM TWEAKS ] ──────────────────"}, // Kernel section
			{Name: "WiFi Power Save", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetWifiPowerSave()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetWifiPowerSave(!b.GetWifiPowerSave()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetWifiPowerSave(!b.GetWifiPowerSave()) }},
			{Name: "Audio Power Save", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetAudioPowerSave()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetAudioPowerSave(!b.GetAudioPowerSave()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetAudioPowerSave(!b.GetAudioPowerSave()) }},
			{Name: "Autosuspend PCI/USB", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetAutosuspend()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetAutosuspend(!b.GetAutosuspend()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetAutosuspend(!b.GetAutosuspend()) }},
			{Name: "Watchdog Kernel", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%v", b.GetWatchdog()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetWatchdog(!b.GetWatchdog()) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetWatchdog(!b.GetWatchdog()) }},
			{Name: "VM Writeback (s)", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d", b.GetVMWriteback()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetVMWriteback(b.GetVMWriteback()+1) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetVMWriteback(b.GetVMWriteback()-1) }},
			{Name: "Process Purge", GetVal: func(b hal.Backend) string { return "EXECUTE" }, 
				Action: func(b hal.Backend, d *Dashboard) { b.ProcessPurge(); d.showToast("PROCESSES PURGED") }},
		}...)
	} else if osName == "Windows" {
		items = append(items, []MenuItem{
			{IsHeader: true, Name: "─── [ DISPLAY ] ────────────────────────"},
			{Name: "LCD Brightness (%)", GetVal: func(b hal.Backend) string { return fmt.Sprintf("%d", b.GetLCDBrightness()) }, 
				Inc: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetLCDBrightness(b.GetLCDBrightness()+5) }, 
				Dec: func(b hal.Backend, d *Dashboard) { b.StopDaemon(); b.SetLCDBrightness(b.GetLCDBrightness()-5) }},
		}...)
	} else if osName == "macOS" {
		items = append(items, []MenuItem{
			{IsHeader: true, Name: "─── [ SYSTEM MEMORY ] ──────────────────"},
			{Name: "Process Purge", GetVal: func(b hal.Backend) string { return "EXECUTE" }, 
				Action: func(b hal.Backend, d *Dashboard) { b.ProcessPurge(); d.showToast("PROCESSES PURGED") }},
		}...)
	}

	return items
}

// Run is the main event loop blocking function
func (d *Dashboard) Run() {
	d.items = buildMenuItems(d) // Initialize items
	
	d.selected = 1 // Start cursor on the first actual item (skipping header)

	d.refreshDelay = 2 * time.Second // Set default graph speed to 2 seconds

	// Background ticker for the live power graph
	go func() {
		for {
			select {
			case <-time.After(d.refreshDelay): // Wait
				d.screen.PostEvent(tcell.NewEventInterrupt(nil)) // Trigger redraw event
			case <-d.quit: // End background task if quit
				return
			}
		}
	}()

	// Main event loop handling keyboard
	for {
		ev := d.screen.PollEvent() // Wait for an event
		switch ev := ev.(type) {
		case *tcell.EventKey: // If it's a keyboard event
			if d.confirmExtreme { // If the extreme mode modal is open, intercept keys
				if ev.Rune() == 'y' || ev.Rune() == 'Y' { // Yes
					d.backend.StopDaemon()
					d.backend.ApplyModeExtreme() // Apply
					d.confirmExtreme = false // Close modal
					d.showToast("EXTREME MODE ACTIVATED") // Notify
					d.drawUI()
				} else if ev.Rune() == 'n' || ev.Rune() == 'N' || ev.Key() == tcell.KeyEscape { // No or escape
					d.confirmExtreme = false // Close modal without applying
					d.drawUI()
				}
				continue // Skip the rest of the controls
			}

			if ev.Rune() == '+' { // Speed up graph
				if d.refreshDelay > 500*time.Millisecond {
					d.refreshDelay -= 500 * time.Millisecond // Decrease interval
					d.showToast(fmt.Sprintf("Update Speed: %v", d.refreshDelay))
				}
				d.drawUI()
				continue
			} else if ev.Rune() == '-' { // Slow down graph
				if d.refreshDelay < 10*time.Second {
					d.refreshDelay += 500 * time.Millisecond // Increase interval
					d.showToast(fmt.Sprintf("Update Speed: %v", d.refreshDelay))
				}
				d.drawUI()
				continue
			}

			// Main application controls
			if ev.Key() == tcell.KeyEscape || ev.Rune() == 'q' || ev.Rune() == 'Q' {
				close(d.quit) // Exit program
				return
			} else if ev.Rune() == 'r' || ev.Rune() == 'R' || ev.Key() == tcell.KeyCtrlR { // Hotkey for restore mode
				d.backend.StopDaemon()
				d.backend.ApplyModeRestore()
				d.showToast("System Restored")
				d.drawUI()
			} else if ev.Key() == tcell.KeyUp || ev.Rune() == 'w' || ev.Rune() == 'W' { // Move up
				for {
					d.selected--
					if d.selected < 0 { // Wrap around
						d.selected = len(d.items) - 1
					}
					if !d.items[d.selected].IsHeader { // Keep going if it's a header
						break
					}
				}
				d.drawUI()
			} else if ev.Key() == tcell.KeyDown || ev.Rune() == 's' || ev.Rune() == 'S' { // Move down
				for {
					d.selected = (d.selected + 1) % len(d.items) // Wrap around
					if !d.items[d.selected].IsHeader { // Keep going if it's a header
						break
					}
				}
				d.drawUI()
			} else if ev.Key() == tcell.KeyRight || ev.Rune() == 'd' || ev.Rune() == 'D' { // Increase value
				if d.selected >= 0 && d.selected < len(d.items) && d.items[d.selected].Inc != nil {
					d.items[d.selected].Inc(d.backend, d) // Execute Inc function
				}
				d.drawUI()
			} else if ev.Key() == tcell.KeyLeft || ev.Rune() == 'a' || ev.Rune() == 'A' { // Decrease value
				if d.selected >= 0 && d.selected < len(d.items) && d.items[d.selected].Dec != nil {
					d.items[d.selected].Dec(d.backend, d) // Execute Dec function
				}
				d.drawUI()
			} else if ev.Key() == tcell.KeyEnter { // Run action
				if d.selected >= 0 && d.selected < len(d.items) && d.items[d.selected].Action != nil {
					d.items[d.selected].Action(d.backend, d) // Execute Action function
				}
				d.drawUI()
			}
		case *tcell.EventInterrupt: // Ticker event
			d.drawUI() // Redraw to update graph
		case *tcell.EventResize: // Terminal resize event
			d.screen.Sync() // Sync buffer
			d.drawUI() // Redraw with new bounds
		}
	}
}

// StartDashboard initializes tcell and launches the UI
func StartDashboard(b hal.Backend) {
	s, err := tcell.NewScreen() // Create a new terminal screen buffer
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to create screen: %v\n", err) // Print error if fails
		os.Exit(1)
	}
	if err := s.Init(); err != nil { // Initialize terminal raw mode
		fmt.Fprintf(os.Stderr, "Failed to init screen: %v\n", err)
		os.Exit(1)
	}

	dash := &Dashboard{ // Create the struct state
		backend: b,
		screen:  s,
		quit:    make(chan struct{}),
	}

	dash.drawUI() // Do an initial draw
	dash.Run() // Start the main loop (blocking)
	s.Fini() // When loop ends, reset terminal to normal mode
}
