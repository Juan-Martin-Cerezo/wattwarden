# System Architecture ⚙️
> **WattWarden Architecture Specification (Rust Edition)**

WattWarden is designed around the principles of **Universal Silicon Polymorphism**, zero-overhead asynchronous event handling, and strict decoupling between hardware drivers and presentation layers.

---

## 🏛️ Foundational Principles
Read [`PHILOSOPHY.md`](PHILOSOPHY.md) for the complete 5 Cardinal Axioms governing hardware abstraction and graceful degradation. All AI-driven refactoring must follow [`AGENTS.md`](AGENTS.md).

---

## 📦 Workspace Crate Layout

```text
wattwarden/
├── Cargo.toml                      # Workspace root & shared dependency definitions
├── crates/
│   ├── wattwarden-core/            # Hardware abstraction traits, typestates, capabilities, config
│   ├── wattwarden-platform-linux/  # Kernel Netlink, sysfs, RAPL, DRM, and Wayland IPC
│   ├── wattwarden-daemon/          # Asynchronous Tokio event loop & systemd service controller
│   ├── wattwarden-tui/             # High-performance Ratatui terminal dashboard
│   └── wattwarden-cli/             # Unified binary CLI dispatcher (clap v4)
├── PHILOSOPHY.md                   # Core philosophy & graceful degradation doctrine
├── AGENTS.md                       # AI agent guidelines & coding constraints
└── CONTRIBUTING.md                 # Development & contribution standards
```

---

## 🧩 Subsystem Architecture

### 1. Hardware Abstraction Layer (`wattwarden-core`)
Defines the unified hardware interfaces and capabilities:
- **`PowerSource`**: Battery capacity, discharge wattage, AC connection status, and stationary mains fallback.
- **`CpuGovernor`**: Number of CPUs, online core scaling, frequency limits, turbo boost, and Energy Performance Preference (EPP).
- **`RaplController`**: Intel/AMD Running Average Power Limit (PL1 long-term and PL2 short-term package wattage).
- **`GpuController`**: Integrated and discrete GPU frequency ceilings across Intel, AMD, and NVIDIA.
- **`DisplayManager`**: Backlight brightness governance for internal panels and external monitors.
- **`ChargeThreshold`**: Battery charge ceiling control across ASUS, Lenovo ThinkPad, Huawei, and standard ACPI interfaces.
- **`CompositorFocus`**: Active window class resolution across Wayland (Hyprland, Sway) and X11 compositors.
- **`PeripheralsController`**: Keyboard backlight, Bluetooth radio, and Wi-Fi state via `rfkill`.
- **`SystemTweaksController`**: Kernel energy optimizations (Wi-Fi power save, audio power save, USB autosuspend, NMI watchdog, VM writeback).

### 2. Polymorphic Linux Implementation (`wattwarden-platform-linux`)
- **Probing Chains**: All hardware interfaces are resolved through non-destructive probe sequences. Missing hardware returns graceful fallbacks instead of causing application failure.
- **Zero-Polling Netlink Engine**: Awakens instantly on kernel power uevents (`NETLINK_KOBJECT_UEVENT`), dropping background CPU consumption to 0.00% while idle.
- **Direct IPC**: Communicates directly with Hyprland and window manager UNIX domain sockets without invoking child processes.

### 3. Asynchronous Daemon Engine (`wattwarden-daemon`)
- Governed by Tokio async tasks:
  1. Netlink kernel event listener.
  2. Wayland compositor window focus socket reader.
  3. Maintenance timer tick (10s interval) for dynamic profile persistence and dynamic auto-brightness adjustments.

### 4. Zero-GC Terminal Interface (`wattwarden-tui`)
- Implemented with `ratatui` and `crossterm`.
- Adapts menu items dynamically according to detected `HardwareCapabilities`. On a desktop with no battery, battery widgets seamlessly transition to "AC Stationary Mode".
