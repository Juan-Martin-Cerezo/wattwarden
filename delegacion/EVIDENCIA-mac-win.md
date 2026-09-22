# Evidencia — macOS y Windows FUNCIONALES (paridad con el Go) + tests portados

Rama: `feat/paridad-go-rust`. Host de trabajo: Linux `aarch64-unknown-linux-gnu`, rustc/cargo 1.98.1.
No se tocó `/home/juan/wattwarden` (sólo lectura), ni `/etc`, ni se corrió el A/B. Sin `sudo`.

## 0. Objetivo

Portar `hal/backend_darwin.go` + `backend_darwin_test.go` y `hal/backend_windows.go` +
`backend_windows_test.go` a Rust, función por función, con mocks, y dejar los 4 comandos de
aceptación en 0.

## 1. Qué se portó (tabla función Go → función Rust)

Tabla completa y regla por regla en **`PARITY.md` §6.1 y §6.2`**. Resumen:

### Darwin (`hal/backend_darwin.go` → `wattwarden-platform::macos`)

| Go | Rust | Nota |
|---|---|---|
| `GetNumCPUs` / `GetCores` | `CpuGovernor::num_cpus` / `online_cores` | `available_parallelism` |
| `SetCores` | `set_online_cores` | no-op |
| `GetFreqLimit` / `SetFreqLimit` | `freq_limit` / `set_freq_limit` | `Err(Unsupported)` / no-op |
| `GetBatteryPercentage` | `battery_percentage` | `pmset -g batt` |
| `IsCharging` | `is_charging` | fallback de fallo = **false** (como Go) |
| `GetBatteryTime` | `time_remaining` | `4:12` / `Charging` / `Calculating...` |
| `GetPowerConsumptionWatts` | `consumption_watts` | `ioreg` |
| `GetRAPLPL1/PL2` (+setters) | `rapl: None` | ver §2 |
| `GetTurbo` / `SetTurbo` | `turbo_enabled` / `set_turbo_enabled` | `true` / no-op |
| `GetEPP` / `SetEPP` | `energy_performance_preference` / set | `"default"` / no-op |
| `GetGPUFreq`/`SetGPUFreq`, `GetASPM`/`SetASPM` | `gpu: None`, `aspm: None` | ver §2 |
| `GetLCDBrightness` / `SetLCDBrightness` | `brightness_percent` / `set_brightness_percent` | `brightness` |
| `GetBluetooth` / `SetBluetooth` | `bluetooth_enabled` / `set_bluetooth_enabled` | +`blueutil`, +`-int` |
| `getMacWifiDevice`/`GetWifiEnable`/`SetWifiEnable` | `wifi_enabled` / `set_wifi_enabled` | `networksetup` |
| `GetWifiPowerSave`, `GetKbdBacklight`, `GetAudioPowerSave` (+setters) | tweaks/peripherals | `false` / no-op |
| `GetAutosuspend` / `SetAutosuspend` | `autosuspend` / set | **`false`** / no-op |
| `GetWatchdog` / `SetWatchdog` | `nmi_watchdog` / set | **`true`** / no-op |
| `GetVMWriteback` / `SetVMDirty` / `SetNMIWatchdog` | `vm_writeback_seconds`(5) / set | 500 cs = 5 s |
| `SetBrightnessTarget`/`SetRefreshRate`/`SetHyprEffects` | — | no-op en Go; sin trait equivalente |
| `ProcessPurge` | `process_purge` | `purge` |
| `ApplyModePerformance`/`Extreme`/`Restore` | `apply_mode_performance`/`extreme`/`restore` | triple `pmset` |
| `getMacLoad` | `MacOsBackend::load_average` | `sysctl` (el lazo divide por NCpu) |
| `GetAutoBrightness`/`SetAutoBrightness`, `IsDaemonRunning`/`StopDaemon`/`StartAutoExtremeDaemon` | `Config` + `DaemonRunner` + `PidManager` | compartido con Linux |

### Windows (`hal/backend_windows.go` → `wattwarden-platform::windows`)

| Go | Rust | Nota |
|---|---|---|
| `getPowerStatus` | `get_power_status` | `kernel32!GetSystemPowerStatus` (FFI, sin fork) |
| `GetBatteryPercentage` / `IsCharging` / `GetBatteryTime` | battery | FFI, mismos fallbacks que Go |
| `GetPowerConsumptionWatts` | `consumption_watts` | `powershell` BatteryStatus |
| `GetNumCPUs`/`GetCores`/`SetCores` | `num_cpus`/`online_cores`/`set_online_cores` | no-op |
| `GetFreqLimit`/`SetFreqLimit` | `freq_limit`/`set_freq_limit` | `Err(Unsupported)`/no-op |
| `GetRAPLPL1/PL2`, `GetGPUFreq`, `GetASPM` (+setters) | `rapl`/`gpu`/`aspm: None` | ver §2 |
| `GetTurbo` / `SetTurbo` | `turbo_enabled` / `set_turbo_enabled` | `powercfg`, **AC+DC** |
| `GetEPP` / `SetEPP` | `energy_performance_preference` / set | `"default"` / no-op |
| `GetLCDBrightness` / `SetLCDBrightness` | `brightness_percent` / `set_brightness_percent` | clamp **0**..100 |
| `GetBluetooth` / `SetBluetooth` | `bluetooth_enabled` / `set_bluetooth_enabled` | `true` / `Set-Service bthserv` |
| `GetWifiEnable` / `SetWifiEnable` | `wifi_enabled` / `set_wifi_enabled` | `netsh` |
| `GetWifiPowerSave`, `GetKbdBacklight`, `GetAudioPowerSave` (+setters) | tweaks/peripherals | `false` / no-op |
| `GetAutosuspend` / `SetAutosuspend` | `autosuspend` / set | **`false`** / no-op |
| `GetWatchdog` / `SetWatchdog` | `nmi_watchdog` / set | **`true`** / no-op |
| `GetVMWriteback` / `SetVMDirty` / `SetNMIWatchdog` | `vm_writeback_seconds`(5) / set | 500 cs = 5 s |
| `SetBrightnessTarget`/`SetRefreshRate`/`SetHyprEffects` | — | no-op en Go; sin trait equivalente |
| `setWinProcThrottle` | `set_proc_throttle` | `PROCTHROTTLEMAX` AC+DC |
| `ApplyModePerformance`/`Extreme`/`Restore` | `apply_mode_performance`/`extreme`/`restore` | throttle 100 / 1+brillo10 |
| `getWinLoad` | `WindowsBackend::load_average` | `typeperf` × NCpu |
| `GetAutoBrightness`/`SetAutoBrightness`, daemon | `Config` + `DaemonRunner` + `PidManager` | compartido |

## 2. Lo que queda `Unsupported`/ausente — y por qué

Paridad = **mismo comportamiento, incluso el faltante**. El Go tampoco implementa esto en esas
plataformas (los `GetRAPL*`, `GetGPU*`, `GetASPM*` devuelven `0`/`"default"` y los `Set*` son no-ops;
no hay `GetChargeThreshold` ni C-states ni compositor):

| Capacidad | Go | Rust | Motivo |
|---|---|---|---|
| RAPL PL1/PL2 | `0` / no-op | `rapl: None` | macOS/Windows no exponen RAPL |
| GPU freq | `0` / no-op | `gpu: None` | sin interfaz de frecuencia de GPU |
| ASPM | `"default"` / no-op | `aspm: None` | sin policy ASPM |
| Charge threshold | no existe | `supports_threshold()=false` + `Err(Unsupported)` | lo maneja el SO/vendor |
| Frecuencia/GOB CPU (MHz) | `0` / no-op | `Err(Unsupported)` | no hay interfaz de MHz; devolver un rango abs. era el bug |
| C-states | no existe | `Ok(vec![])` | sin telemetría |
| Compositor activo | no existe | `None` | Hyprland/X11 son de Linux |

Regla respetada: **ningún rango absoluto inventado** para escribir. macOS/Windows exponen
`discovered_freq_bounds()/discovered_gpu_bounds()/pl1/pl2_bounds_watts()` = `None` (default del
trait), así que el lazo compartido **no escribe** frecuencia/GPU/RAPL ahí (lo cubre el test del
daemon `non_linux_tests`).

## 3. Tests

### 3.1 Portados del Go

* `macos::tests::native_commands_drive_the_backend` ← `TestDarwinBackendNativeCommands`
  (`backend_darwin_test.go`): mismos `pmset`/`ioreg`/`purge` falsos en `PATH`, y afirma batería 82,
  `is_charging == false`, tiempo `4:12`, consumo `12 W`, y que `ApplyModeExtreme`/`ProcessPurge`
  ejecutan `pmset -a lowpowermode 1` y `purge`.
* `windows::tests::native_commands_drive_the_backend` ← `TestWindowsBackendNativeCommands`
  (`backend_windows_test.go`): `powershell`/`powercfg` falsos en `PATH`, brillo 50 desde PowerShell y
  `powercfg` invocado por los modos Extreme/Restore.

### 3.2 Qué corre en Linux (este host) y qué sólo en CI

| Test | Corre acá (Linux)? | Corre en CI macOS? | Corre en CI Windows? |
|---|---|---|---|
| `parse::tests::*` (13 tests, sin `cfg`) | ✅ | ✅ | ✅ |
| `exec::tests::missing_binary_returns_empty_string` | ✅ | ✅ | ✅ |
| `macos::tests::native_commands_drive_the_backend` (`cfg(unix)`) | ✅ | ✅ | — |
| `windows::tests::native_commands_drive_the_backend` (`cfg(not(windows))`) | ✅ | ✅ (macOS runner) | ❌ |

**Por qué el test de Windows no corre en Windows (honestidad):** los binarios falsos del test del Go
son `.cmd` y Go los resuelve con `exec.LookPath` (que usa `PATHEXT`). El `std::process::Command` de
Rust usa `CreateProcess`, que **no** consulta `PATHEXT`, así que `Command::new("powershell")` nunca
resolvería un `powershell.cmd` falso. Por eso el mock se escribe como script POSIX `sh` y corre en
Linux/macOS; en Windows la cobertura que corre son los parsers puros de `crate::parse`. No se
inventó cobertura: se portó la que existe y se hizo correr donde el mecanismo lo permite.

Los mocks mutan `PATH` global ⇒ se serializan con `crate::exec::env_lock()` (como `t.Setenv`).

## 4. Clippy cruzado (lint, sin SDK ni linker)

```
### cargo clippy --workspace --all-targets --target x86_64-apple-darwin -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 36.85s

### cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.77s

### cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in  0.86s

### cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.10s

### cargo clippy --workspace --all-targets -- -D warnings      (host Linux)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in  0.84s
```

Con `-D warnings`, `Finished` = cero warnings/errores en los 5 crates. La primera corrida de
`windows-gnu` falló con `error: function env_lock is never used` (en Windows los dos mocks están
`cfg`-ados fuera): se gateó `env_lock` a `#[cfg(all(test, any(unix, not(target_os = "windows"))))]`
y quedó en 0. Los dos targets extra (arm64 macOS / MSVC) se agregaron porque son los hosts reales del
CI, además de los dos del enunciado.

## 5. Aceptación local (host Linux) — los 4 en 0

```bash
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && \
  cargo clippy --workspace --all-targets -- -D warnings
```

```
FMT_OK
    Finished `dev` profile (build --all-targets)
test result: ok.   8 passed; 0 failed;   (cli)
test result: ok.  16 passed; 0 failed;   (core)
test result: ok.  26 passed; 0 failed;   (daemon)
test result: ok.  76 passed; 0 failed;   (platform)   ← +24 vs antes (13 parse + 2 mocks mac/win + os_name + …)
test result: ok.   9 passed; 0 failed;   (platform, tests/go_parity.rs)
test result: ok.   6 passed; 0 failed;   (platform, tests/hw_agnostic.rs)
test result: ok.   8 passed; 0 failed;   (tui)        ← +2 (menú por OS + línea de resumen)
CLIPPY_HOST: Finished (`dev` profile) con -D warnings
```

149 tests, 0 fallos. Ningún test de Linux se tocó: se agregó `exec`, `parse`, los dos mocks y los
golden de `GetOS()`.

## 6. Lo que NO se pudo verificar (honestidad)

* **No hay hardware Apple ni Windows.** Los tests de paridad **se ejecutaron** en Linux contra
  binarios falsos (eso es real), pero la ejecución contra el hardware real (`pmset`, `ioreg`,
  `networksetup`, `brightness`, `blueutil`, `GetSystemPowerStatus`, `powercfg`, `powershell`,
  `netsh`, `typeperf`) no se probó. Queda como **"no verificado en hardware"** (también en
  `PARITY.md` §6.4).
* **El link contra `kernel32`** (`GetSystemPowerStatus`) está verificado en *compilación* (clippy
  cruzado a `*-windows-gnu`/`*-msvc`); desde Linux no hay MinGW/SDK para linkear. Ese paso lo prueba
  el CI de Windows. Se siguió el mismo patrón FFI ya presente en `wattwarden-daemon/src/pid.rs`
  (`OpenProcess`), que hoy linkea en el CI de Windows.
* El test de Windows **no corre en el runner de Windows** por la diferencia `PATHEXT`
  (`CreateProcess` vs `exec.LookPath`) descrita en §3.2.
* No se tocó `.github/workflows/` ni `install.sh`/`uninstall.sh`: el cambio es sólo de código y
  documentación.
