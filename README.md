# WattWarden ⚡

<div align="center">

[![CI](https://github.com/Juan-Martin-Cerezo/wattwarden/actions/workflows/ci.yml/badge.svg)](https://github.com/Juan-Martin-Cerezo/wattwarden/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux-blue.svg)](https://www.kernel.org/)

**Next-Generation Hardware Power Governance & Telemetry Suite for Linux**

```text
 ██╗    ██╗ █████╗ ████████╗████████╗██╗    ██╗ █████╗ ██████╗ ██████╗ ███████╗███╗   ██╗
 ██║    ██║██╔══██╗╚══██╔══╝╚══██╔══╝██║    ██║██╔══██╗██╔══██╗██╔══██╗██╔════╝████╗  ██║
 ██║ █╗ ██║███████║   ██║      ██║   ██║ █╗ ██║███████║██████╔╝██║  ██║█████╗  ██╔██╗ ██║
 ██║███╗██║██╔══██║   ██║      ██║   ██║███╗██║██╔══██║██╔══██╗██║  ██║██╔══╝  ██║╚██╗██║
 ╚███╔███╔╝██║  ██║   ██║      ██║   ╚███╔███╔╝██║  ██║██║  ██║██████╔╝███████╗██║ ╚████║
  ╚══╝╚══╝ ╚═╝  ╚═╝   ╚═╝      ╚═╝    ╚══╝╚══╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝ ╚══════╝╚═╝  ╚═══╝
```

</div>

WattWarden is an industrial-grade, zero-overhead systems utility designed to give you absolute control over your machine's silicon power constraints. Re-engineered from the ground up in modern Rust, WattWarden replaces inefficient polling loops and child-process forks with direct Linux kernel Netlink event listening, native Hyprland/Wayland socket IPC, and hardware-level RAPL/C-State telemetry.

---

## 🌟 Key Architectural Features

- **Zero-Polling Kernel Event Engine:** Leverages `NETLINK_KOBJECT_UEVENT` to awaken strictly upon AC plug/unplug and battery power events, achieving **0.00% CPU utilization** while idle.
- **Hardware-Level RAPL Controls:** Directly governs Intel and AMD Running Average Power Limit (PL1 long-term and PL2 short-term) constraints through `/sys/class/powercap/`.
- **Deep C-State Telemetry:** Inspects real-time CPU idle package residency (`C1`, `C6`, `C8`, `C10`) directly from sysfs, verifying genuine silicon dormancy.
- **BMS Battery Charge Thresholds:** Natively governs battery charge ceilings (`charge_control_end_threshold`) to preserve Li-ion health by capping charges at configurable limits (e.g. 80%).
- **Subprocess-Free Hyprland IPC:** Connects directly to Hyprland's UNIX domain socket (`.socket2.sock`) to adapt display brightness dynamically between terminal sessions and web browsers without spawning child processes.
- **Compile-Time Typestate Governance:** Encodes power profiles (`Profile<Performance>`, `Profile<Extreme>`, `Profile<Normal>`) into Rust's type system to eliminate illegal hardware transitions at compile time.
- **Zero-GC Terminal Dashboard (TUI):** Built with `ratatui` and `crossterm`, rendering real-time ASCII power discharge graphs, per-package wattage, and interactive hardware controls with 60 FPS responsiveness and no runtime garbage collection.

---

## 🏗️ Workspace Architecture

```text
wattwarden/
├── Cargo.toml                      # Multi-crate workspace definition
├── crates/
│   ├── wattwarden-core/            # Hardware traits, typestate patterns, and config schema
│   ├── wattwarden-platform-linux/  # Direct sysfs, RAPL, Netlink, and Hyprland socket IPC
│   ├── wattwarden-daemon/          # Asynchronous Tokio event runner & systemd controller
│   ├── wattwarden-tui/             # High-performance Ratatui terminal dashboard
│   └── wattwarden-cli/             # Unified binary entry point (clap v4)
├── .github/
│   └── workflows/
│       ├── ci.yml                  # Continuous integration (fmt, clippy, tests)
│       └── release.yml             # Multi-arch automated release binary builds
├── install.sh                      # 1-command installer script
├── uninstall.sh                    # Clean uninstallation script
├── CONTRIBUTING.md                 # Contribution guidelines
└── LICENSE                         # MIT License
```

---

## 🚀 Installation

### One-Line Automated Install
```bash
curl -fsSL https://raw.githubusercontent.com/Juan-Martin-Cerezo/wattwarden/master/install.sh | bash
```
*This downloads the latest pre-compiled binary for your architecture (`x86_64` or `aarch64`), places it into `/usr/local/bin`, and optionally configures the systemd background daemon.*

### Build from Source
```bash
# Clone repository
git clone https://github.com/Juan-Martin-Cerezo/wattwarden.git
cd wattwarden

# Compile optimized release binary
cargo build --release

# Install locally
sudo ./install.sh
```

---

## ⚡ CLI Usage & Commands

WattWarden provides an intuitive CLI with full subcommand routing:

### Subcommands

| Command | Privileges | Description |
| :--- | :---: | :--- |
| `wattwarden` or `wattwarden tui` | Optional root | Launches the interactive TUI hardware dashboard |
| `wattwarden status` | Non-root | Inspects real-time battery drain, active profile, and daemon state |
| `sudo wattwarden profile <name>` | **Root** | Applies power profile (`normal`, `performance`, `extreme`, `auto`) |
| `sudo wattwarden threshold <pct>`| **Root** | Configures BMS battery charge limit percentage (`50`–`100`) |
| `sudo wattwarden brightness <val>`| **Root** | Adjusts display brightness (`1`–`100`) or toggles auto-brightness (`on`/`off`) |
| `sudo wattwarden start` | **Root** | Starts background daemon process |
| `sudo wattwarden stop` | **Root** | Gracefully stops active background daemon |
| `sudo wattwarden daemon` | **Root** | Runs daemon in foreground (used by systemd unit) |
| `sudo wattwarden service install`| **Root** | Installs and enables persistent auto-starting systemd service |
| `sudo wattwarden service uninstall`| **Root** | Disables and removes systemd service |

### Examples

```bash
# Inspect system status without root
wattwarden status

# Apply Extreme energy-saving profile
sudo wattwarden profile extreme

# Set battery charge stop limit to 80%
sudo wattwarden threshold 80

# Enable dynamic auto-brightness for Hyprland
sudo wattwarden brightness on

# Install systemd service for boot persistence
sudo wattwarden service install
```

---

## ⌨️ TUI Keyboard Controls

When running the interactive dashboard (`sudo wattwarden`):

- **`↑` / `↓` or `W` / `S`**: Navigate menu items
- **`←` / `→` or `A` / `D`**: Adjust hardware limits (online cores, frequency ceilings, GPU frequency, brightness, charge limit, VM writeback)
- **`Enter`**: Toggle active profile or peripheral state (Wi-Fi, Bluetooth, Keyboard Backlight, ASPM, Turbo, EPP)
- **`Y` / `N`**: Confirm or cancel Extreme Mode safety prompt
- **`Q` or `Esc`**: Exit dashboard

---

## ⚙️ Hardware Compatibility & Requirements

- **Linux Kernel:** Version 5.10 or higher.
- **Architectures:** `x86_64`, `aarch64`.
- **Hardware Interfaces:**
  - Standard ACPI battery & mains power supply (`/sys/class/power_supply/`)
  - Intel/AMD CPU frequency scaling (`/sys/devices/system/cpu/`)
  - Intel RAPL power cap interface (`/sys/class/powercap/intel-rapl:0`)
  - Backlight controller (`/sys/class/backlight/`)
  - Linux `rfkill` for radio peripherals
- **Compositors:** Native IPC focus detection on **Hyprland** (Wayland); fallback graceful degradation on generic X11/Wayland.

---

## 🗑️ Uninstallation

To completely remove WattWarden, including the systemd service and binaries:
```bash
sudo ./uninstall.sh
```
Or via curl:
```bash
curl -fsSL https://raw.githubusercontent.com/Juan-Martin-Cerezo/wattwarden/master/uninstall.sh | bash
```

---

## 🤝 Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for details on code standards, local testing, and pull request workflows.

---

## 🛡️ License

This project is licensed under the **MIT License** — see the [LICENSE](LICENSE) file for details.

Authored with precision by **Juan Martín Cerezo**.
