# WattWarden ⚡

<div align="center">

[![CI](https://github.com/Juan-Martin-Cerezo/wattwarden/actions/workflows/ci.yml/badge.svg)](https://github.com/Juan-Martin-Cerezo/wattwarden/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS-blue.svg)](https://github.com/Juan-Martin-Cerezo/wattwarden)

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
- **Cross-Platform Support**: Native power-management and hardware control backends for Linux (`sysfs`, RAPL, Netlink sockets, Wayland/Hyprland IPC) and macOS (`pmset`, `ioreg`, `sysctl`).
- **Persistent Background Daemon**: Auto Extreme Mode and Auto-Brightness run continuously in the background as an OS service (`systemd`, `launchd`, or background runner), keeping your power optimized even after closing the terminal or rebooting.
- **Intelligent Auto-Brightness**: Dynamically adjusts display brightness based on active window context (Terminals vs Browsers/IDEs) and AC charging state, with instant live toggle and manual override.
- **Live Telemetry & ASCII Power Graph**: Track battery drain in Watts, charge percentage, BMS thresholds, and estimated battery time remaining in real time via an interactive, zero-overhead TUI power graph.
- **Zero-Polling Kernel Architecture**: Built in modern Rust without subprocess forks or wasteful sleep loops, achieving virtually 0.00% CPU utilization while idling in the background.

---

## 🎚️ Auto Extreme Levels

**Auto Extreme** is an adaptive controller: on every tick it reads `/proc/loadavg`, normalises it by core count, and scales CPU frequency, active cores, GPU frequency and RAPL power limits to match. The level does **not** hardcode those values — it *parametrises the loop* (idle threshold, minimum idle cores, EPP, turbo gate and window brightness). The 100% hardware ceiling under load is never lowered at any level.

| Level | Idle threshold | Idle cores | EPP (idle / load) | Turbo gate | Brightness (terminal / default / heavy UI) |
|-------|----------------|------------|-------------------|------------|--------------------------------------------|
| `high` *(default)* | 0.30 | 2 | power / power | ≥ 0.80 | 12 / 20 / 30 |
| `medium` | 0.50 | max(2, ncpu/2) | power / power | ≥ 0.50 | 20 / 30 / 40 |
| `low` | 0.70 | all | balance_power / balance_performance | always | 30 / 45 / 55 |

`high` is the most aggressive (maximum battery life, lowest performance) and is the default; `medium` and `low` react sooner to load and leave more resources available. The daemon re-reads the level on every tick, so a change applies without restarting the service.

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

## ⚙️ CLI Commands

WattWarden supports both intuitive subcommands and classic flags for rapid terminal workflows:

```bash
sudo wattwarden                      # Launch interactive TUI Dashboard (Default)
wattwarden status                   # Check daemon running status & battery health summary
wattwarden health                   # Full battery health report (capacity, cycles, charge limit)
sudo wattwarden charge-limit <55-100|off> # Set or clear the battery charge ceiling (e.g. 80%)
sudo wattwarden calibrate <start|status|done|abort> # Guided battery gauge recalibration
sudo wattwarden start               # Start background daemon/service
sudo wattwarden stop                # Stop background daemon/service
sudo wattwarden profile <name>      # Apply profile (normal, performance, extreme, auto)
sudo wattwarden level <low|medium|high> # Set Auto Extreme adaptive level
sudo wattwarden brightness <val>    # Set display brightness (1-100) or toggle auto (on/off)
sudo wattwarden service install     # Install & enable auto-start system service
sudo wattwarden service uninstall   # Remove system background service
sudo wattwarden daemon              # Run daemon in foreground (for systemd/launchd)
```

*Classic flags (`wattwarden --status`, `sudo wattwarden --start`, `sudo wattwarden --stop`) are fully supported as drop-in aliases.*

---

## 🔋 Battery Health & Calibration

`wattwarden health` reports what the battery's fuel gauge knows: full vs design capacity
(the health percentage), cycle count, pack temperature, and the charge ceiling in effect.

- **What the percentage means**: it is the *reported* health of the pack. With a charge
  ceiling (e.g. 50–80%) the gauge never sees a full charge, so its capacity estimate can
  drift downwards — reported health degrades faster than the chemistry actually does.
- **Calibration** relearns the gauge: `sudo wattwarden calibrate start` lifts the ceiling
  to 100%, then follow the guided phases (`status` walks charge → discharge to 10% →
  recharge) and finish with `sudo wattwarden calibrate done`, which restores the previous
  threshold. Run it every couple of months. The dashboard's **BATTERY** section drives the
  same phases: `Calibrate` starts (or advances) the cycle and `Calib Restore` finishes it.
- **Charge ceiling**: `sudo wattwarden charge-limit 80` persists the limit and applies it;
  `charge-limit off` clears WattWarden's setting and *never* touches the hardware threshold
  (it may be managed by the BIOS or another tool). The dashboard's **Charge Limit** row
  cycles OFF → 55…100 the same way.
- **macOS**: health metrics come from `ioreg` (`AppleSmartBattery`: full vs design
  capacity, cycles, temperature). The charge ceiling and calibration are **Linux-only**:
  macOS exposes no charge threshold and manages charging natively via Optimized Battery
  Charging, so `charge-limit`/`calibrate` report unsupported instead of pretending.
- **History & temperature** (opt-in, off by default): set `"battery_health_log": true` in
  `/etc/wattwarden/config.json` to append an hourly CSV sample to
  `/var/log/wattwarden-health.log`, and `"battery_temp_warn_c": 45` to get a log warning
  when the pack runs hot while charging.

WattWarden obeys the user, never the cable: it suggests a charge limit in `wattwarden
health`, but only applies one when you set it.

---

## ⌨️ TUI Controls

- **Up / Down** or **W / S**: Navigate menu options.
- **Left / Right** or **A / D**: Adjust hardware limits or values (brightness, active cores, frequency bounds, charge limit).
- **Enter**: Trigger highlighted action or toggle profile (**Performance**, **Extreme**, **Auto Extreme**, **Restore**, battery **Calibrate** / **Calib Restore**).
- **+ / -**: Speed up or slow down the live power graph refresh rate.
- **R**: Hotkey to instantly restore default system power settings.
- **Q / Esc**: Exit the TUI (if Auto Extreme is active, the daemon remains running seamlessly in the background).

---

## 🏗️ Architecture & Philosophy

- [`PHILOSOPHY.md`](PHILOSOPHY.md): **The Doctrine of Silicon Polymorphism** (6 Cardinal Axioms of hardware adaptation).
- [`ARCHITECTURE.md`](ARCHITECTURE.md): Multi-crate modular architecture, dynamic probe chains, and capability discovery.
- [`CONTRIBUTING.md`](CONTRIBUTING.md): Guidelines for code standards, testing, and contribution.

---

## 📄 License

WattWarden is distributed under the terms of the [MIT License](LICENSE).
