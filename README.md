# WattWarden ⚡ (Rust Edition)

> **Next-Generation Hardware Power Governance & Telemetry Suite for Linux**

WattWarden is an industrial-grade, zero-overhead systems utility designed to give you absolute control over your device's power constraints. Re-engineered in Rust, WattWarden replaces inefficient polling loops and subprocess forks with direct Linux kernel Netlink event listening, native Hyprland/Wayland socket IPC, and hardware-level RAPL/C-State telemetry.

---

## 🌟 Key Architectural Features

- **Zero-Polling Kernel Event Engine:** Leverages `NETLINK_KOBJECT_UEVENT` to awaken strictly upon AC plug/unplug and battery state changes, achieving **0.00% CPU utilization** while idle.
- **Deep C-State Telemetry:** Inspects real-time CPU idle package residency (`C1`, `C6`, `C8`, `C10`) directly from sysfs, ensuring true silicon dormancy.
- **BMS Battery Charge Thresholds:** Natively governs battery charge ceilings (`charge_control_end_threshold`) to preserve Li-ion health by capping charges at 80%.
- **Subprocess-Free Hyprland IPC:** Connects directly to Hyprland's UNIX domain socket (`.socket2.sock`) to adapt display brightness dynamically between terminal sessions and web browsers without spawning child processes.
- **Compile-Time Typestate Governance:** Encodes power profiles (`Profile<Performance>`, `Profile<Extreme>`, `Profile<Normal>`) into Rust's type system to eliminate illegal hardware transitions.
- **Terminal User Interface (TUI):** Built with `ratatui` and `crossterm`, providing real-time ASCII power graphs, package wattage, and interactive profile switching with zero GC pauses.

---

## 🏗️ Workspace Architecture

```
wattwarden/
├── Cargo.toml                      # Multi-crate workspace definition
├── crates/
│   ├── wattwarden-core/            # Hardware traits, typestate patterns, and config schema
│   ├── wattwarden-platform-linux/  # Direct sysfs, RAPL, Netlink, and Hyprland socket IPC
│   ├── wattwarden-daemon/          # Asynchronous Tokio event runner & systemd controller
│   ├── wattwarden-tui/             # High-performance Ratatui terminal dashboard
│   └── wattwarden-cli/             # Unified binary entry point (clap v4)
└── README.md
```

---

## 🚀 Quick Start & CLI Usage

### Build from Source
```bash
cargo build --release
```
The optimized binary is emitted to `target/release/wattwarden`.

### Interactive TUI Dashboard
```bash
sudo ./target/release/wattwarden
```

### CLI Subcommands
```bash
# Inspect real-time battery drain, daemon state, and charge ceiling
wattwarden status

# Apply and persist a power profile (normal, performance, extreme, auto)
sudo wattwarden profile extreme

# Limit battery charge ceiling to 80% (BMS health preservation)
sudo wattwarden threshold 80

# Configure display brightness or auto-brightness toggle
sudo wattwarden brightness 45
sudo wattwarden brightness on

# Start/Stop background daemon
sudo wattwarden start
sudo wattwarden stop

# Install and enable auto-starting systemd service
sudo wattwarden service install
```

---

## ⌨️ TUI Keyboard Controls
- **Up/Down or W/S:** Navigate hardware menu items.
- **Left/Right or A/D:** Adjust hardware limits (Cores, Freq limits, Brightness, BMS threshold).
- **Enter:** Toggle active profile or feature state.
- **Q or Esc:** Exit the dashboard.

---

## 🛡️ License
MIT License. Authored by Juan Martín Cerezo.
