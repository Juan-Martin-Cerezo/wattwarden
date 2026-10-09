# The Doctrine of Silicon Polymorphism 🏛️
> **WattWarden Project Philosophy & Architectural Constitution**

WattWarden is engineered under a singular architectural mandate: **Universal Silicon Polymorphism**. The software must dynamically and autonomously adapt to any computer, silicon architecture, operating system, and kernel version without pre-supposing hardware presence, hardcoding driver layouts, or failing abruptly.

---

## 📜 The 6 Cardinal Axioms

Every contributor—whether a human software engineer or an autonomous AI coding agent—must preserve these six non-negotiable axioms:

### 1. Probe First, Bind Later (Dynamic Discovery)
The system must never assume the existence of any sysfs node, driver attribute, device file, or IPC socket. Hardware components are discovered through non-destructive, prioritized discovery probes at runtime. If a vendor path does not exist, the probe returns `None` and allows lower-priority fallback probes to evaluate.

### 2. The Graceful Degradation Invariant (Zero Hard Panics)
**WattWarden must never abort execution because a hardware feature is missing.**
- On a desktop workstation, server, or container with no battery, the power subsystem must seamlessly degrade into **Stationary AC Mains Mode** rather than terminating with an `InterfaceNotFound` error.
- If display backlight nodes are absent (e.g. external monitors or headless servers), display brightness controls are hidden cleanly without breaking UI layout or daemon loops.
- If RAPL, EPP, or GPU scaling interfaces are unexposed by the kernel, the remaining CPU and OS tweaks remain 100% operational.

### 3. Zero Subprocess Invocations in Telemetry Loops
Telemetry loops and background event monitors must **never** fork external shells (`sh`, `bash`) or spawn child processes (`cat`, `echo`, `hyprctl`, `xrandr`, `rfkill`). All hardware interaction must occur via:
- Direct OS system calls (`sysfs`, `procfs`, `ioctl`).
- Native kernel sockets (`NETLINK_KOBJECT_UEVENT`).
- Native UNIX domain sockets or IPC mechanisms.
Spawning child processes introduces unacceptable CPU overhead, memory fragmentation, and latency that destroys the utility's zero-overhead guarantee.

### 4. Vendor-Agnostic Probe Chains
Every hardware controller interface (`CpuGovernor`, `GpuController`, `ChargeThreshold`, `CompositorFocus`) is backed by an ordered chain of vendor-specific probes:
- **CPU Energy:** Intel RAPL $\to$ AMD Energy $\to$ ACPI Thermal Zones $\to$ Null Governor.
- **GPU Scaling:** Intel DRM $\to$ AMD Radeon $\to$ NVIDIA sysfs $\to$ Null GPU.
- **BMS Threshold:** Linux generic $\to$ ASUS WMI $\to$ Lenovo ThinkPad $\to$ Huawei $\to$ Apple SMC $\to$ Null Threshold.
- **Window Focus:** Hyprland socket $\to$ Sway/i3 socket $\to$ GNOME Shell DBus $\to$ KDE KWin $\to$ X11 Active Window $\to$ Null Focus.

### 5. Hotplug & Environment Agility
Hardware topology is mutable at runtime. Display cables are detached, external power bricks are swapped for USB-PD chargers, eGPUs are connected, and background compositors may restart. WattWarden must handle runtime hardware transitions and socket disconnections gracefully through automatic reconnection and reactive event listeners.

### 6. Zero-Friction Single-Command Deployment (Universal Installation Invariant)
**WattWarden must be installable, configurable, and deployable with exactly ONE single command on any machine.**
- No convoluted multi-step compilation pipelines, dependency hunts, or manual configuration ceremonies.
- A user or autonomous agent must be able to invoke a single self-contained command (via `curl -fsSL https://.../install.sh | sudo bash`, `sudo ./install.sh`, or `make install`) that automatically probes the system, acquires or builds the optimized release binary, establishes proper path bindings, configures native background services (`systemd` / `launchd`), and immediately converges to a running, verified state.
- The installation workflow itself must honour Silicon Polymorphism: probe for toolchains, fall back to pre-compiled multi-arch binaries, and adapt to available privilege levels gracefully.

---

## 🛡️ Maintainer & AI Agent Directives

When modifying or expanding the WattWarden codebase:
1. **Never use `?` on primary component discovery in startup routines.** Missing hardware is a normal operating condition, not an error.
2. **Enforce compile-time typestate guarantees.** Illegal hardware transitions (e.g. entering Extreme mode from an invalid state) must be prevented by the type system.
3. **Preserve the single-command deployment invariant.** Any feature addition must not introduce manual post-install configuration steps.
4. **Maintain 100% test pass rates and zero Clippy warnings.** Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before every commit.
5. **Always provide unit tests with simulated mock interfaces** so that tests remain deterministic on headless CI build nodes without physical hardware.

