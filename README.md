# ⚡ VoltTamer

VoltTamer is an ultra-minimalist, dependency-free, pure-Python Terminal User Interface (TUI) designed to give you absolute hardware-level control over your Linux machine's power and performance. It allows you to tune CPU cores, frequencies, Intel RAPL limits, PCIe ASPM policies, and much more, natively reading and writing to the Linux Kernel's `/sys/` and `/proc/` filesystems.

Built around a "Ponytail ULTRA" minimalist philosophy: No bloated libraries, no unnecessary abstractions, just direct hardware manipulation wrapped in a gorgeous Curses-based UI.

## ✨ Features

- **Dynamic Hardware Control**: Enable/disable CPU cores, limit CPU frequency, tweak Intel RAPL PL1/PL2 wattage, and toggle Intel P-State/AMD Boost natively.
- **Advanced Networking & Radios**: Aggressive WiFi power saving, Bluetooth RFKill, and Audio power saving states.
- **Universal Agnostic Design**: Designed to work on ANY modern Linux distribution and ANY desktop environment (Hyprland, Wayland, X11, GNOME, KDE, or raw TTY).
- **Auto-Extreme Daemon**: A built-in background daemon that intelligently scales your CPU cores, frequencies, and screen brightness based on active load and window focus.
- **Live Power Monitor**: A 600-frame historical power draw and battery graph built purely in ASCII, visualizing real-time wattage spikes.

## 🚀 Installation

VoltTamer is designed as a single monolithic Python script. No `pip install`, no `virtualenv`, no build tools required. 

1. **Clone the repository**:
   ```bash
   git clone https://github.com/juan/power-center.git
   cd power-center
   ```

2. **Install globally (Recommended)**:
   Because VoltTamer manipulates `/sys/class` and other kernel interfaces, it requires `root` privileges. We recommend placing it in your system binaries path:
   ```bash
   sudo cp power-center.py /usr/local/bin/power-center
   sudo chmod +x /usr/local/bin/power-center
   ```

3. **Dependencies**:
   VoltTamer is written using only the Python 3 Standard Library (`curses`, `os`, `sys`, `glob`, `json`, `subprocess`). 
   *Optional Fallback Dependency*: `brightnessctl` (Used as a fallback for manipulating display brightness if kernel DRM modules block direct raw sysfs writes).

## 🎮 Usage

Simply run VoltTamer with sudo privileges:

```bash
sudo power-center
```

- **[↑↓] Navigate**: Scroll through hardware categories (CPU, GPU, Radios, Actions).
- **[←→] Adjust**: Change configurable values (e.g., Wattage limits, Frequencies).
- **[ENTER] Toggle / Execute**: Turn features ON/OFF or apply global profiles.
- **[TAB] Switch View**: Toggle between the **General Control Panel** and the **Live Power Monitor**.

### Power Profiles

- ⚡ **PERFORMANCE PROFILE**: Unlocks all CPU cores, unlocks max frequency limits, bumps RAPL limits to maximum hardware bounds, enables Turbo Boost, and maximizes brightness.
- ♻ **RESTORE PROFILE**: Restores your computer to the original baseline settings captured the first time the app was run.
- 🔋 **EXTREME PROFILE**: Extreme battery saving. Drops your computer to absolute minimum hardware bounds (minimum cores, minimum frequencies, minimum wattage, minimum brightness) and disables radios.
- ⚡ **AUTO EXTREME PROFILE**: A smart, un-intrusive background daemon. It scales your CPU cores, frequencies, and brightness proportionally to your hardware bounds based on a 1-minute load average. Quantized 20% steps ensure zero micro-fluctuations, giving you power exactly when you need it while saving maximum energy when you don't.

## 🤝 How to Contribute

We welcome contributions to make VoltTamer the ultimate Linux power management utility. 

1. **Fork the Project** on GitHub.
2. **Create a Feature Branch** (`git checkout -b feature/AmazingFeature`).
3. **Commit your Changes** (`git commit -m 'Add some AmazingFeature'`).
4. **Push to the Branch** (`git push origin feature/AmazingFeature`).
5. **Open a Pull Request**.

### Contribution Guidelines
- **Zero Dependencies**: Do not introduce any third-party dependencies from PyPI (e.g., `psutil`). If it's not in the Python Standard Library, we don't want it.
- **Native over Wrappers**: Prefer direct reads/writes to `/sys/` or `/proc/` over executing Bash commands via `subprocess`.
- **Security First**: If you must use `subprocess`, use `shell=False` or pass arguments as a list. Do not pipe unfiltered user input.
- **Fallback Gracefully**: Ensure that if a specific hardware node doesn't exist (e.g., an AMD GPU missing Intel RAPL nodes), the script catches the `FileNotFoundError` and continues without crashing the `curses` interface.

## 🛡️ Cybersecurity Note

VoltTamer uses `SUDO_UID` and `SUDO_USER` to bridge the gap between running as `root` (for hardware manipulation) and running as your unprivileged user (for interacting with things like Wayland/Hyprland socket APIs). It avoids unchecked `shell=True` executions whenever possible to prevent command injection vulnerabilities.

---
*VoltTamer - Tame your voltage, own your hardware.*
