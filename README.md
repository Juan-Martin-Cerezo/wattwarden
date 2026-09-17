# WattWarden ⚡

<div align="center">

[![CI](https://github.com/Juan-Martin-Cerezo/wattwarden/actions/workflows/ci.yml/badge.svg)](https://github.com/Juan-Martin-Cerezo/wattwarden/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS%20%7C%20Windows-blue.svg)](https://github.com/Juan-Martin-Cerezo/wattwarden)

**Universal Hardware Power Governance & Telemetry Suite**

```text
 ██╗    ██╗ █████╗ ████████╗████████╗██╗    ██╗ █████╗ ██████╗ ██████╗ ███████╗███╗   ██╗
 ██║    ██║██╔══██╗╚══██╔══╝╚══██╔══╝██║    ██║██╔══██╗██╔══██╗██╔══██╗██╔════╝████╗  ██║
 ██║ █╗ ██║███████║   ██║      ██║   ██║ █╗ ██║███████║██████╔╝██║  ██║█████╗  ██╔██╗ ██║
 ██║███╗██║██╔══██║   ██║      ██║   ██║███╗██║██╔══██║██╔══██╗██║  ██║██╔══╝  ██║╚██╗██║
 ╚███╔███╔╝██║  ██║   ██║      ██║   ╚███╔███╔╝██║  ██║██║  ██║██████╔╝███████╗██║ ╚████║
  ╚══╝╚══╝ ╚═╝  ╚═╝   ╚═╝      ╚═╝    ╚══╝╚══╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝ ╚══════╝╚═╝  ╚═══╝
```

</div>

Welcome to **WattWarden**, the ultimate hardware management tool designed to give you absolute ownership over your device's power constraints.

In an era where operating systems and software abstractions often obscure direct hardware control, WattWarden empowers you to reclaim your machine. Whether your goal is to breathe new life into an aging laptop by dramatically extending its battery lifespan, or to unshackle your CPU and GPU for maximum raw performance, this tool provides the definitive solution. By interacting directly with system-level boundaries, it allows you to dynamically enforce extreme power-saving limits or unleash unrestrained computing power—all through a lightning-fast, highly optimized Terminal User Interface (TUI) or an autonomous background daemon.

---

## 🌟 Why WattWarden?

- **Unleash or Constrain**: Push your CPU/GPU to absolute maximum performance, or cap it heavily to save battery using our dedicated **Extreme Mode**.
- **Universal Adaptability**: Dynamically detects your system hardware limits (CPU cores, turbo boost, Intel RAPL limits, GPU bounds, battery metrics) and gracefully adapts the interface to precisely what your hardware supports.
- **Cross-Platform Support**: Native power-management and hardware control backends for Linux (`sysfs`, RAPL, Netlink sockets, Wayland/Hyprland IPC), macOS (`pmset`, `ioreg`, `sysctl`), and Windows (`powercfg`, WMI, Win32 CIM APIs).
- **Persistent Background Daemon**: Auto Extreme Mode and Auto-Brightness run continuously in the background as an OS service (`systemd`, `launchd`, or background runner), keeping your power optimized even after closing the terminal or rebooting.
- **Intelligent Auto-Brightness**: Dynamically adjusts display brightness based on active window context (Terminals vs Browsers/IDEs) and AC charging state, with instant live toggle and manual override.
- **Live Telemetry & ASCII Power Graph**: Track battery drain in Watts, charge percentage, BMS thresholds, and estimated battery time remaining in real time via an interactive, zero-overhead TUI power graph.
- **Zero-Polling Kernel Architecture**: Built in modern Rust without subprocess forks or wasteful sleep loops, achieving virtually 0.00% CPU utilization while idling in the background.

---

## 🚀 Installation & Usage

We provide pre-compiled, static binaries for all major operating systems and architectures.

### 🐧 Linux & 🍏 macOS Installation

For Linux (`x86_64`, `aarch64`) and macOS (Intel or Apple Silicon), install and configure WattWarden automatically:

```bash
curl -fsSL https://raw.githubusercontent.com/Juan-Martin-Cerezo/wattwarden/master/install.sh | sudo bash
```

This installer will:
1. Detect your OS and CPU architecture.
2. Download or compile the matching binary to `/usr/local/bin/wattwarden`.
3. Register and start the background service (`systemd` on Linux or `launchd` on macOS).

**To open the interactive TUI dashboard anytime:**
```bash
sudo wattwarden
```

#### Alternative: Build from Source
```bash
git clone https://github.com/Juan-Martin-Cerezo/wattwarden.git
cd wattwarden
sudo make install
```

#### Alternative: Cargo
```bash
cargo install --git https://github.com/Juan-Martin-Cerezo/wattwarden.git wattwarden-cli
```

---

### 🪟 Windows Installation

1. Open PowerShell as Administrator and run:
   ```powershell
   irm https://raw.githubusercontent.com/Juan-Martin-Cerezo/wattwarden/master/install.ps1 | iex
   ```
2. Or download `wattwarden-windows-x86_64.exe` directly from the [Releases page](https://github.com/Juan-Martin-Cerezo/wattwarden/releases/latest).
3. *(Optional)* To install and run the background service automatically at system startup:
   ```cmd
   wattwarden --install-service
   ```

---

## ⚙️ CLI Commands

WattWarden supports both intuitive subcommands and classic flags for rapid terminal workflows:

```bash
sudo wattwarden                      # Launch interactive TUI Dashboard (Default)
wattwarden status                   # Check daemon running status, battery drain & profile
sudo wattwarden start               # Start background daemon/service
sudo wattwarden stop                # Stop background daemon/service
sudo wattwarden profile <name>      # Apply profile (normal, performance, extreme, auto)
sudo wattwarden threshold <pct>     # Set battery charge limit percentage (e.g. 80%)
sudo wattwarden brightness <val>    # Set display brightness (1-100) or toggle auto (on/off)
sudo wattwarden service install     # Install & enable auto-start system service
sudo wattwarden service uninstall   # Remove system background service
sudo wattwarden daemon              # Run daemon in foreground (for systemd/launchd)
```

*Classic flags (`wattwarden --status`, `sudo wattwarden --start`, `sudo wattwarden --stop`) are fully supported as drop-in aliases.*

---

## ⌨️ TUI Controls

- **Up / Down** or **W / S**: Navigate menu options.
- **Left / Right** or **A / D**: Adjust hardware limits or values (brightness, active cores, frequency bounds).
- **Enter**: Trigger highlighted action or toggle profile (**Performance**, **Extreme**, **Auto Extreme**, **Restore**).
- **+ / -**: Speed up or slow down the live power graph refresh rate.
- **R**: Hotkey to instantly restore default system power settings.
- **Q / Esc**: Exit the TUI (if Auto Extreme is active, the daemon remains running seamlessly in the background).

---

## 🏗️ Architecture & Philosophy

- [`PHILOSOPHY.md`](PHILOSOPHY.md): **The Doctrine of Silicon Polymorphism** (6 Cardinal Axioms of hardware adaptation).
- [`ARCHITECTURE.md`](ARCHITECTURE.md): Multi-crate modular architecture, dynamic probe chains, and capability discovery.
- [`AGENTS.md`](AGENTS.md): Operational guidelines for autonomous coding agents and maintainers.
- [`CONTRIBUTING.md`](CONTRIBUTING.md): Guidelines for code standards, testing, and contribution.

---

## 📄 License

WattWarden is distributed under the terms of the [MIT License](LICENSE).
