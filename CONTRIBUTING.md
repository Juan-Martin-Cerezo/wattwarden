# Contributing to WattWarden ⚡

Thank you for your interest in contributing to WattWarden! We welcome contributions from developers of all backgrounds. This guide provides instructions on setting up your development environment, running tests, and submitting high-quality pull requests.

---

## 🏗️ Architecture Overview

WattWarden is organized as a multi-crate Cargo workspace:

- **`crates/wattwarden-core`**: Core domain logic, hardware abstraction traits (`PowerSource`, `CpuGovernor`, `RaplController`, `DisplayManager`, `ChargeThreshold`, `CompositorFocus`), compile-time typestate profile definitions, and configuration schema.
- **`crates/wattwarden-platform-linux`**: Linux platform implementations communicating directly with kernel interfaces: sysfs, Netlink kobject uevents (`NETLINK_KOBJECT_UEVENT`), RAPL power caps, rfkill, and Hyprland UNIX domain sockets.
- **`crates/wattwarden-daemon`**: Asynchronous Tokio-based background daemon handling reactive kernel event loops, Hyprland window focus changes, and systemd service management.
- **`crates/wattwarden-tui`**: High-performance terminal dashboard built using `ratatui` and `crossterm`, providing real-time telemetry graphs, wattage discharge rates, and interactive hardware controls.
- **`crates/wattwarden-cli`**: Unified command-line entry point driven by `clap` v4 with subcommand routing and backward-compatible flag normalization.

---

## 🛠️ Development Prerequisites

- **Rust toolchain:** Latest stable Rust (1.80+ recommended, edition 2021). Install via [rustup](https://rustup.rs/):
  ```bash
  rustup update stable
  rustup component add clippy rustfmt
  ```
- **Operating System:** Linux (kernel 5.10+ recommended for modern sysfs, RAPL, and Netlink interfaces).
- **Optional desktop environment:** Hyprland compositor (for auto-brightness window focus testing).

---

## 🧪 Development Workflow

### 1. Fork & Clone
```bash
git clone https://github.com/<your-username>/wattwarden.git
cd wattwarden
```

### 2. Build
```bash
# Debug build
cargo build

# Optimized release build
cargo build --release
```

### 3. Run Tests
Every pull request must pass all workspace unit tests:
```bash
cargo test --all-targets
```

### 4. Code Formatting & Linting
WattWarden maintains strict zero-warning standards. Ensure your code is formatted and passes Clippy:
```bash
# Format check
cargo fmt --check

# Automatic format fix
cargo fmt

# Clippy linter
cargo clippy --workspace --all-targets -- -D warnings
```

---

## 💡 Guidelines & Best Practices

1. **Zero Child Processes for Telemetry:** Do not invoke external shells or child process commands (e.g. `cat`, `echo`, `hyprctl`, `cat /sys/...`) when direct sysfs file I/O or UNIX domain sockets can be used.
2. **Deterministic Fallbacks:** Hardware interfaces vary widely across vendors (Intel, AMD, ASUS, Dell, Lenovo). Always provide robust error handling and fallbacks without panicking.
3. **Mockable Design:** When introducing new hardware controllers, write unit tests with simulated temporary sysfs structures to ensure deterministic CI runs on headless runners.
4. **Conventional Commits:** Please format commit messages following the Conventional Commits specification:
   - `feat: add support for AMD Ryzen RAPL energy counters`
   - `fix(tui): prevent graph overflow on high wattage spikes`
   - `docs: update systemd configuration details`
   - `test: add unit tests for charge threshold limits`

---

## 🚀 Submitting a Pull Request

1. Create a feature branch: `git checkout -b feat/my-new-feature`
2. Commit your changes: `git commit -m "feat: implement feature"`
3. Push to your branch: `git push origin feat/my-new-feature`
4. Open a Pull Request on GitHub against `master`. CI will automatically verify formatting, clippy lints, and tests.
