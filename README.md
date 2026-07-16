# Power Center Extreme ⚡

Welcome to **Power Center Extreme**! This project provides a powerful, adaptive, and lightning-fast Terminal User Interface (TUI) to take absolute control of your computer's hardware power limits and performance.

## 🌟 Why Power Center Extreme?
- **Unleash or Constrain**: Push your CPU/GPU to absolute maximum performance, or cap it heavily to save incredible amounts of battery using our dedicated **Extreme Mode**.
- **Universal Adaptability**: Dynamically detects your system hardware limits (CPU cores, turbo boost, Intel RAPL package limits, GPU boundaries) and adapts the interface to precisely what your hardware supports.
- **Cross-Platform**: Designed for seamless hardware abstraction. Supports Linux out of the box with `sysfs` access. Future modules target seamless macOS and Windows capability.
- **Live Monitoring**: See your active battery drain (in Watts), charge state, and battery time left directly inside the TUI via a responsive ASCII bar graph.
  
## 🚀 Installation & Usage

We provide pre-compiled binaries for all major operating systems. 

### 🐧 Linux Installation

To install Power Center Extreme globally as a system application, open your terminal and run this single command:

**For Intel/AMD (64-bit):**
```bash
sudo curl -L https://github.com/Juan-Martin-Cerezo/power-center-extreme/releases/download/v1.0.0/power-center-linux-amd64 -o /usr/local/bin/power-center && sudo chmod +x /usr/local/bin/power-center
```

**For ARM (Raspberry Pi, etc):**
```bash
sudo curl -L https://github.com/Juan-Martin-Cerezo/power-center-extreme/releases/download/v1.0.0/power-center-linux-arm64 -o /usr/local/bin/power-center && sudo chmod +x /usr/local/bin/power-center
```

**How to run (Linux):**
Because the program directly controls hardware boundaries, simply run it anywhere with `sudo`:
```bash
sudo power-center
```

### 🪟 Windows Installation

Windows users do not need to use the terminal. Just follow these steps:

1. Go to the [Releases page](https://github.com/Juan-Martin-Cerezo/power-center-extreme/releases/latest).
2. Download the `power-center-windows-amd64.exe` file.
3. Save it to your Desktop or a folder of your choice.
4. **Right-click** on the `.exe` file and select **"Run as administrator"**. 
*(Administrator privileges are required to change system power profiles and frequencies).*

### 🍎 macOS Installation

macOS installation works similarly to Linux. Open the `Terminal` app and run:

**For Apple Silicon (M1/M2/M3):**
```bash
sudo curl -L https://github.com/Juan-Martin-Cerezo/power-center-extreme/releases/download/v1.0.0/power-center-macos-arm64 -o /usr/local/bin/power-center && sudo chmod +x /usr/local/bin/power-center
```

**For Intel Macs:**
```bash
sudo curl -L https://github.com/Juan-Martin-Cerezo/power-center-extreme/releases/download/v1.0.0/power-center-macos-amd64 -o /usr/local/bin/power-center && sudo chmod +x /usr/local/bin/power-center
```

**How to run (macOS):**
```bash
sudo power-center
```

---

## ⌨️ TUI Controls
- **Up/Down or W/S**: Navigate the menu options.
- **Left/Right or A/D**: Adjust the specific hardware limit/value (increase or decrease).
- **Enter**: Apply the highlighted mode (like Performance, Restore, Extreme).
- **+ / -**: Speed up or slow down the live power graph refresh rate.
- **R**: Hotkey to instantly restore system defaults.
- **Q / Esc**: Quit the application.
