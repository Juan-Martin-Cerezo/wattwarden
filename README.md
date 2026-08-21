# WattWarden ⚡

Welcome to **WattWarden**, the ultimate hardware management tool designed to give you absolute ownership over your device's power constraints.

In an era where operating systems and software abstractions often obscure direct hardware control, WattWarden empowers you to reclaim your machine. Whether your goal is to breathe new life into an aging laptop by dramatically extending its battery lifespan, or to unshackle your CPU and GPU for maximum raw performance, this tool provides the definitive solution. By interacting directly with system-level boundaries, it allows you to dynamically enforce extreme power-saving limits or unleash unrestrained computing power—all through a lightning-fast, highly optimized Terminal User Interface (TUI) or an autonomous background daemon.

## 🌟 Why WattWarden?
- **Unleash or Constrain**: Push your CPU/GPU to absolute maximum performance, or cap it heavily to save battery using our dedicated **Extreme Mode**.
- **Universal Adaptability**: Dynamically detects your system hardware limits (CPU cores, turbo boost, Intel RAPL limits, GPU bounds, battery metrics) and gracefully adapts the interface to precisely what your hardware supports.
- **Cross-Platform Support**: Native power-management and hardware control backends for Linux (sysfs/brightnessctl/rfkill), macOS (pmset/ioreg/defaults), and Windows (powercfg/WMI/Win32 APIs).
- **Persistent Background Daemon**: Auto Extreme Mode and Auto-Brightness run continuously in the background as an OS service (systemd, launchd, or background runner), keeping your power optimized even after closing the terminal or rebooting.
- **Intelligent Auto-Brightness**: Dynamically adjusts display brightness based on active window context (Terminals vs Browsers/IDEs) and AC charging state, with instant live toggle and manual override.
- **Live Monitoring**: Track battery drain (in Watts), charge percentage, and estimated battery time remaining in real time via an interactive ASCII power graph.

---

## 🚀 Installation & Usage

We provide pre-compiled, static binaries for all major operating systems and architectures.

### 🐧 Linux & 🍎 macOS Installation (1-Command Install)

For Linux (x86_64, ARM64, 32-bit i386) and macOS (Intel or Apple Silicon M1/M2/M3/M4), install and start WattWarden automatically with a single command:

```bash
curl -fsSL https://raw.githubusercontent.com/Juan-Martin-Cerezo/wattwarden/main/install.sh | sudo bash
```

This installer will:
1. Detect your OS and CPU architecture.
2. Download and install the matching binary to `/usr/local/bin/wattwarden`.
3. Register and start the background service (Systemd on Linux or LaunchDaemon on macOS).

**To open the interactive TUI dashboard anytime:**
```bash
sudo wattwarden
```

### 🪟 Windows Installation

1. Go to the [Releases page](https://github.com/Juan-Martin-Cerezo/wattwarden/releases/latest).
2. Download `wattwarden-windows-amd64.exe`.
3. **Right-click** on the `.exe` file and select **"Run as administrator"**.
4. *(Optional)* To install and run the background service automatically at system startup:
   ```cmd
   wattwarden-windows-amd64.exe --install-service
   ```

---

## ⚙️ CLI Commands

In addition to the interactive TUI, WattWarden provides dedicated CLI flags for quick control:

```bash
sudo wattwarden                     # Launch interactive TUI Dashboard
sudo wattwarden --start             # Start background daemon/service
sudo wattwarden --stop              # Stop background daemon/service
sudo wattwarden --status            # Check daemon running status
sudo wattwarden --brightness on|off # Configure auto-brightness setting
sudo wattwarden --install-service   # Install & enable auto-start system service
sudo wattwarden --uninstall-service # Remove system background service
sudo wattwarden --daemon            # Run daemon in foreground (for systemd/launchd)
```

---

## ⌨️ TUI Controls
- **Up/Down or W/S**: Navigate menu options.
- **Left/Right or A/D**: Adjust hardware limits or values (e.g. brightness, cores, limits).
- **Enter**: Trigger highlighted action or toggle profile (Performance, Extreme, Auto Extreme, Restore).
- **+ / -**: Speed up or slow down the live power graph refresh rate.
- **R**: Hotkey to instantly restore default system power settings.
- **Q / Esc**: Exit the TUI (if Auto Extreme is active, it remains running in the background).
