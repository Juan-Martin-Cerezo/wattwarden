# WattWarden ⚡

Welcome to **WattWarden**, the ultimate hardware management tool designed to give you absolute ownership over your device's power constraints.

In an era where operating systems and software abstractions often obscure direct hardware control, WattWarden empowers you to reclaim your machine. Whether your goal is to breathe new life into an aging laptop by dramatically extending its battery lifespan, or to unshackle your CPU and GPU for maximum raw performance, this tool provides the definitive solution. By interacting directly with system-level boundaries, it allows you to dynamically enforce extreme power-saving limits or unleash unrestrained computing power—all through a lightning-fast, highly optimized Terminal User Interface (TUI).

## 🌟 Why WattWarden?
- **Unleash or Constrain**: Push your CPU/GPU to absolute maximum performance, or cap it heavily to save incredible amounts of battery using our dedicated **Extreme Mode**.
- **Universal Adaptability**: Dynamically detects your system hardware limits (CPU cores, turbo boost, Intel RAPL package limits, GPU boundaries) and adapts the interface to precisely what your hardware supports.
- **Cross-Platform**: Designed for seamless hardware abstraction. Supports Linux out of the box with `sysfs` access. Future modules target seamless macOS and Windows capability.
- **Live Monitoring**: See your active battery drain (in Watts), charge state, and battery time left directly inside the TUI via a responsive ASCII bar graph.
  
## 🚀 Installation & Usage

We provide pre-compiled binaries for all major operating systems. 

### 🐧 Linux & 🍎 macOS Installation

For Linux (Intel, AMD, or ARM) and macOS (Intel or Apple Silicon M1/M2/M3), you can install WattWarden automatically with a single command. Open your terminal and run:

*Note: Ensure you have `curl` or `wget` installed on your system (e.g., `sudo apt install curl` on Ubuntu/Debian).*

```bash
curl -fsSL https://raw.githubusercontent.com/Juan-Martin-Cerezo/wattwarden/master/install.sh | sudo bash
```

This script will automatically detect your operating system and processor architecture, download the correct binary, and install it globally.

**How to run:**
Because the program directly controls hardware boundaries, simply run it in your terminal with `sudo`:
```bash
sudo wattwarden
```

### 🪟 Windows Installation

Windows users do not need to use the terminal. Just follow these steps:

1. Go to the [Releases page](https://github.com/Juan-Martin-Cerezo/wattwarden/releases/latest).
2. Download the `wattwarden-windows-amd64.exe` file.
3. Save it to your Desktop or a folder of your choice.
4. **Right-click** on the `.exe` file and select **"Run as administrator"**. 
*(Administrator privileges are required to change system power profiles and frequencies).*

---

## ⌨️ TUI Controls
- **Up/Down or W/S**: Navigate the menu options.
- **Left/Right or A/D**: Adjust the specific hardware limit/value (increase or decrease).
- **Enter**: Apply the highlighted mode (like Performance, Restore, Extreme).
- **+ / -**: Speed up or slow down the live power graph refresh rate.
- **R**: Hotkey to instantly restore system defaults.
- **Q / Esc**: Quit the application.
