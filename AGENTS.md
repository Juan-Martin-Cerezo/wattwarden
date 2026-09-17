# AI Agent Operational Directives 🤖
> **Strict Guidelines for AI Assistants & Autonomous Coding Agents Modifying WattWarden**

If you are an AI agent modifying or reasoning about this codebase, you **must strictly adhere** to the directives below. Any PR, patch, or edit violating these rules will be rejected immediately.

---

## 🚫 Cardinal Prohibitions

1. **NO Hardcoded Sysfs or Kernel Paths Without Dynamic Probing:**
   - ❌ **Forbidden:** Writing code that directly assumes `/sys/class/power_supply/BAT0` or `/sys/class/drm/card1/gt_max_freq_mhz` exists.
   - ✅ **Mandated:** Use the `glob_dirs` or probe chain pattern to discover nodes dynamically across vendor conventions (Intel, AMD, NVIDIA, Apple Silicon, ARM).

2. **NO Panicking or Fatal Exits on Missing Hardware:**
   - ❌ **Forbidden:** `let battery = LinuxBattery::new()?;` where failure prevents application startup.
   - ✅ **Mandated:** Return a graceful fallback (e.g., `StationaryPowerSource` for desktop machines without batteries). The application must boot flawlessly on a desktop PC, rack server, Raspberry Pi, or container.

3. **NO Child Process Forks (`Command::new("sh")`, `Command::new("cat")`, etc.) in Hot Loops:**
   - ❌ **Forbidden:** Invoking `hyprctl activewindow`, `xrandr`, `cat /sys/...`, or `rfkill` periodically.
   - ✅ **Mandated:** Read/write directly via `fs::read_to_string`, `fs::write`, UNIX domain sockets, or Netlink uevent listeners.

4. **NO Hardcoded Desktop Environments:**
   - ❌ **Forbidden:** Assuming Hyprland is the only compositor in existence.
   - ✅ **Mandated:** Provide fallback probes (Sway, GNOME, KDE, X11, or Null compositor fallback).

---

## 📋 Required Code Patterns

### 1. The Dynamic Capability Pattern
Every hardware controller must report its capabilities cleanly via bitflags or query methods (`supports_*()`). The UI and CLI must inspect capabilities before displaying controls or adjusting hardware limits.

### 2. Mock-Friendly Unit Testing
When adding or altering hardware modules:
- Create unit tests utilizing temporary directories (`std::env::temp_dir()`) populated with simulated sysfs structures.
- Tests must pass on headless Linux CI machines without requiring root or physical hardware access.

### 3. Clippy and Formatting Invariants
- Code must pass `cargo clippy --workspace --all-targets -- -D warnings` with zero warnings.
- Code must be formatted with standard `cargo fmt`.

---

## 🏛️ Reference Architecture
Read [`PHILOSOPHY.md`](PHILOSOPHY.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md) before proposing modifications to hardware abstraction layers.
