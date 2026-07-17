# System Architecture ⚙️

WattWarden uses a highly modular, decoupled architecture focused on performance, maintainability, and cross-platform compatibility.

## Core Design Principles

1. **Strict Decoupling**: The User Interface (`ui/`) has absolutely zero knowledge of how hardware limits are actually enforced. It merely calls interface methods.
2. **Hardware Abstraction Layer (HAL)**: All hardware interactions are routed through a unified interface (`hal/backend.go`). This allows the system to easily adapt to Windows, macOS, or Linux.
3. **No External Dependencies for Hardware**: We avoid third-party libraries for hardware access. On Linux, this is achieved by reading and writing directly to the kernel's `/sys/` pseudo-filesystem.

## Directory Structure

```
wattwarden/
├── main.go               # Entry point, verifies permissions and injects backend
├── install.sh            # Universal installation script (detects OS and 32/64-bit architectures)
├── ARCHITECTURE.md       # Architecture specification
├── README.md             # Project user guide
├── hal/                  # Hardware Abstraction Layer
│   ├── backend.go        # Defines the `Backend` interface that all OS-specific files must implement
│   ├── backend_linux.go  # Linux implementation using sysfs and uevent fallbacks
│   ├── backend_linux_test.go # Unit tests verifying Linux hardware getters
│   ├── backend_darwin.go # macOS native implementation (pmset)
│   └── backend_windows.go# Windows native implementation (powercfg, WMI)
└── ui/                   # Terminal User Interface
    └── cli.go            # Draws the TUI, manages state, handles user input using `tcell`
```

## How It Works

### The Hardware Abstraction Layer (HAL)
The `Backend` interface dictates what actions a platform *must* support, such as:
- `GetNumCPUs() int`
- `SetFreqLimit(mhz int)`
- `GetBatteryPercentage() int`
- `GetPowerConsumptionWatts() float64`
- `ApplyModeExtreme()`

Cuando la aplicación arranca, `hal.CurrentBackend` es inyectado gracias a los build tags de Go (`//go:build linux`, `//go:build windows`, `//go:build darwin`). Esto significa que `main.go` y la Interfaz Gráfica (`ui/cli.go`) nunca necesitan saber en qué sistema operativo están corriendo.

#### Robust Battery & Power Resolution (Linux)
In standard Linux systems, hardware directories can change across vendors. To handle this dynamically without hardcoded assumptions, the Linux backend implements:
* **Dynamic Battery Directory Lookup**: Scans for standard directories (`BAT0`, `BAT1`, `BAT2`, `BATT`) using file statistics, and falls back to listing all `/sys/class/power_supply/*` interfaces and checking if their `type` file contains `"Battery"`.
* **Multi-Format Power Metric Resolution**:
  1. Tries reading the standard `power_now` (microwatts) file.
  2. If missing, falls back to `current_now` (microamperes) * `voltage_now` (microvolts) computation.
  3. **uevent Parsing Fallback**: If separate files are missing or restricted, parses the unified `/sys/class/power_supply/BAT*/uevent` file for properties (`POWER_SUPPLY_POWER_NOW`, `POWER_SUPPLY_CURRENT_NOW`, `POWER_SUPPLY_VOLTAGE_NOW`).
  4. **Absolute Value Normalization**: Applies absolute value conversions (`math.Abs`) to resolve issues where ACPI drivers return negative numbers while discharging.

### Multi-Architecture & 32-bit Support
WattWarden compiles to native machine binaries with no external runtime library dependencies. In addition to 64-bit platforms (amd64, arm64), the build system generates static binaries for **32-bit x86 Linux (i386/i686)**, allowing lightweight execution on legacy hardware (e.g. running MX Linux 32-bit).

The automated `install.sh` script queries `uname -m` and dynamically maps old architectures like `i386`/`i686` to the Go 32-bit target (`386`).

### The User Interface (UI)
The `ui.Dashboard` struct handles the presentation layer using `tcell`.
Contruye una lista de `MenuItem` que conecta la interfaz gráfica a los métodos de la interfaz `hal.Backend`. Dependiendo del OS devuelto por `b.GetOS()`, los menús ocultan opciones no soportadas nativamente por el SO anfitrión, garantizando que todos los botones funcionales hagan lo prometido sin generar errores silenciosos.

**Dynamic Layout Engine:**
The UI dynamically detects terminal sizes on every redraw event:
- If the terminal is wider than 130 columns, it splits the menu and the graph side-by-side.
- If narrower, it stacks the graph on top of the menu and calculates visible list items, adding a visual scrollbar.

### Auto Daemon
La implementación en `hal` posee un proceso en segundo plano (Goroutine) llamado "Auto Extreme Daemon". Cuando está habilitado, monitorea la carga en el CPU (loadavg en Unix/Mac o typeperf en Windows) cada 10 segundos.
- Si está conectada a la corriente, aplica Máximo Rendimiento.
- Si está con batería, calcula un factor normalizado matemático (`discretePower`), y ajusta los límites de hardware (brillo de pantalla, frecuencias, limits de acelerador) proporcionalmente a la carga de procesamiento, logrando un ahorro de batería ultra fino sin congelar la PC durante picos de trabajo.
