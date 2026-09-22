# PARITY.md — Go (`master`) es la especificación. Este doc es el contrato.

Origen: `~/wattwarden` rama `origin/master` (Go, estable, EN PRODUCCIÓN).
Destino: este repo (Rust). **No hay ancestro común entre las ramas** (verificado: `merge-base` vacío,
`master` root `3dd20acc`, `feat/rust-rewrite` root `85f0d0bb`) → el Rust es una reescritura hecha de
memoria. Todo lo de abajo se transcribe de `master` leyendo el código, no de la memoria de nadie.

Regla de oro: **si este doc y el código Rust discrepan, gana este doc** (viene de Go, que funciona).
Si el doc y el Go real discrepan, gana el Go real → leer `~/wattwarden/service/service.go` (305 líneas),
`~/wattwarden/hal/backend_linux.go` (1018 líneas), `~/wattwarden/ui/cli.go` (719 líneas),
`~/wattwarden/main.go` (121 líneas). Esas rutas son la fuente de verdad.

---

## 1. Daemon (lo que está roto hoy)

Go: `hal/backend_linux.go:845` `StartAutoExtremeDaemon()`. **Dos tickers, no uno:**

| Ticker | Período | Qué corre |
|---|---|---|
| `ticker` | **5 s** | `applyLogic()` |
| `brightnessTicker` | **300 ms** | `applyBrightness()` |

Ambos corren una vez **inmediatamente** al arrancar (antes del `select`), y luego por tick.
El Rust hoy usa **un solo ticker de 10 s** y el brillo sólo por evento de Hyprland → eso es la mitad
del problema reportado.

### 1.1 `applyLogic()` — Go verbatim

```
si IsCharging():        # enchufado = baseline de rendimiento
    SetCores(GetNumCPUs())
    SetFreqLimit(99999)         # se clampa a hw max
    SetRAPLPL1(115); SetRAPLPL2(115)
    SetTurbo(true)
    SetEPP("performance")
    SetGPUFreq(99999)           # se clampa a hw max
    SetASPM("performance")
    SetWifiPowerSave(false)
    SetAudioPowerSave(false)
    SetAutosuspend(false)
    SetWatchdog(true)
    SetVMWriteback(500)
    si GetAutoBrightness(): SetLCDBrightness(100)
si no:                  # a batería = adaptativo
    load = parse_float(primer campo de /proc/loadavg)     # loadavg 1 min
    powerLevel = min(load / GetNumCPUs(), 1.0)
    discretePower = round(powerLevel * 3) / 3.0           # CUANTIZADO: 0 | 0.333 | 0.667 | 1.0
    minCPU, hwMaxCPU = GetCPUFreqBounds()
    maxCPU = int(minCPU + (hwMaxCPU-minCPU) * 0.4)        # TECHO 40% del rango hw
    minGPU, hwMaxGPU = GetGPUBounds()
    maxGPU = int(minGPU + (hwMaxGPU-minGPU) * 0.4)        # TECHO 40%
    minW, hwMaxW = GetRAPLBounds()
    maxW = int(minW + (hwMaxW-minW) * 0.4)                # TECHO 40%
    maxCores = max(GetNumCPUs() / 2, 1)                   # división entera
    targetCores = max(int(1.0 + discretePower * (maxCores - 1)), 1)
    targetCPU  = int(minCPU + discretePower * (maxCPU - minCPU))
    targetGPU  = int(minGPU + discretePower * (maxGPU - minGPU))
    targetRAPL = int(minW   + discretePower * (maxW   - minW))
    targetTurbo = discretePower >= 0.8                    # o sea: SÓLO en el escalón 1.0
    targetEPP  = "power"                                  # SIEMPRE power, no depende del nivel
    SetCores(targetCores); SetFreqLimit(targetCPU); SetGPUFreq(targetGPU)
    SetRAPLPL1(targetRAPL); SetRAPLPL2(targetRAPL)
    SetEPP(targetEPP); SetTurbo(targetTurbo)
    SetASPM("powersave"); SetWifiPowerSave(true); SetKbdBacklight(false)
    SetAudioPowerSave(true); SetAutosuspend(true)
    SetWatchdog(false); SetVMWriteback(6000)
```

Errores que introdujo el Rust actual (hay que eliminarlos):
1. Techo **100%** del rango en vez del **40%** → con `low`/`medium` el techo crece, nunca el default.
2. Load **continuo** en vez de cuantizado en 4 escalones.
3. **10 s** en vez de 5 s.
4. `SetEPP`/`SetTurbo` condicionados a `manage_power_hints` (High = false) → en High **no** se
   escribe EPP ni turbo. Go **sí** los escribe en cada tick a batería (`EPP=power`, turbo por escalón).
5. Faltan por completo: ASPM, wifi power save, kbd backlight, audio power save, autosuspend,
   watchdog, VM writeback en la rama a batería (Go los aplica cada tick).
6. `handle_power_state_change` sólo por evento netlink: si no llega el evento, nunca cambia de rama.
   Go lo decide **en cada tick de 5 s** con `IsCharging()`.

### 1.2 `applyBrightness()` — Go verbatim

```
relee /etc/wattwarden/config.json → si parsea, actualiza el flag auto_brightness en memoria
si not GetAutoBrightness(): return
si IsCharging():
    si GetLCDBrightness() < 80: SetLCDBrightness(100); last=100
    return
activeClass = getLinuxActiveWindow()      # minúsculas, puede ser ""
isTerminal = class == "" || contains(kitty|foot|alacritty|wezterm|ghostty|xterm)
isHeavyUI  = contains(firefox|chrome|chromium|brave|zen|code|cursor|idea|studio)
target = isTerminal ? 12 : (isHeavyUI ? 30 : 20)
si target != last || GetLCDBrightness() != target:
    SetLCDBrightness(target); last = target
```

Ojo: **clase vacía = terminal = 12%**. Los valores 12/20/30 son fijos en Go; en el Rust actual
salen de `terminal_brightness`/`gui_brightness` del config (25/65) → cambiar eso.

`getLinuxActiveWindow()` (Go): `HYPRLAND_INSTANCE_SIGNATURE` + `XDG_RUNTIME_DIR` del env; si faltan,
glob de `/run/user/*/hypr/*/.socket.sock` para deducir runtimeDir + sig; después
`hyprctl activewindow -j` (con ese env) y parsea el campo `class` a minúsculas; si falla,
`xdotool getactivewindow getwindowclassname`. Devolver `""` si no hay nada.

### 1.3 Estado y config

- Config: `/etc/wattwarden/config.json`, JSON **indentado 2 espacios**, modo 0644, dir 0755.
  Claves de Go (deben seguir existiendo y con el mismo significado):
  `auto_extreme_enabled` (bool), `auto_brightness` (bool). Defaults en memoria: ambos `true`.
  Los campos extra del Rust están OK (Go ignora desconocidos), pero el JSON escrito debe seguir
  siendo legible por Go y `auto_extreme_enabled` debe reflejar `--start`/`--stop`.
- PID: `/var/run/wattwarden.pid`, contenido = PID en texto plano, 0644, se borra al salir.
- `IsDaemonActive()`: lee el PID y verifica con señal 0; **si eso falla**, en Linux consulta
  `systemctl is-active wattwarden.service` y acepta `active`.

## 2. Hardware — mapeo exacto de sysfs (`hal/backend_linux.go`)

| Función | Paths / regla |
|---|---|
| `GetNumCPUs` | glob `/sys/devices/system/cpu/cpu[0-9]*`; fallback `runtime.NumCPU()` |
| `GetCores` | `1 + count(cpuN/online == "1", N=1..)` (cpu0 se asume online) |
| `SetCores(n)` | clamp 1..NumCPUs; escribe `cpuN/online` 1/0 para N=1..; **después re-aplica `SetFreqLimit(GetFreqLimit())` si >0** |
| `GetCPUFreqBounds` | `cpu0/cpufreq/cpuinfo_min_freq` y `_max_freq` (kHz); fallback 400/1600 MHz; /1000 |
| `GetFreqLimit` | `cpu0/cpufreq/scaling_max_freq` /1000 |
| `SetFreqLimit(mhz)` | clamp a bounds; escribe en **todos** los `cpu*/cpufreq`: `scaling_min_freq = min*1000` y `scaling_max_freq = mhz*1000` |
| batería | nombres BAT0, BAT1, BAT2, BATT; fallback: cualquier power_supply con `type == "Battery"`; fallback final `BAT0` (cacheado) |
| `GetBatteryPercentage` | `capacity` |
| `IsCharging` | recorre power_supply: si `type` ∈ {Mains, USB_C, USB} y `online == "1"` → true; si no: `status` ∈ {Charging, Full} |
| `GetBatteryTime` | charging → `"Charging"`; si no `energy_now`/`power_now`, entonces uevent `POWER_SUPPLY_ENERGY_NOW`/`POWER_SUPPLY_POWER_NOW`, si no `CHARGE_NOW`/`CURRENT_NOW`/`VOLTAGE_NOW` (o los archivos homónimos) → `"%dh %02dm"`; fallback `"Calculating..."` |
| `GetPowerConsumptionWatts` | `power_now`/1e6; si no `current_now*voltage_now`/1e12; si no uevent; 0.0 |
| RAPL bounds | `/sys/class/powercap/intel-rapl:0/min_power_range_uw` y `max_power_range_uw` /1e6; fallback 5/115 W |
| RAPL PL1/PL2 | `getRAPLPath`: busca en `constraint_%d_name` (i=0..4) el literal `long_term`/`short_term` y escribe `constraint_%d_power_limit_uw` (W×1e6, clamp a bounds) |
| Turbo | si existe `intel_pstate/no_turbo` → `"0"`=on (`GetTurbo` == `"0"`), escribir `"0"`/`"1"`; si no `cpufreq/boost` → `"1"`=on; default `true` |
| EPP | lee `cpu0/cpufreq/energy_performance_preference`; escribe **todos** `cpu*/cpufreq/energy_performance_preference`; además governor: `performance` si pref == `"performance"`, si no `powersave`, en todos los `cpu*/cpufreq/scaling_governor` |
| GPU path | `card1/gt_max_freq_mhz` si existe (sino `card0/gt_max_freq_mhz`); si no hay → sin soporte |
| GPU bounds | `gt_RPn_freq_mhz` → fallback `gt_min_freq_mhz`; `gt_RP0_freq_mhz` → fallback `gt_max_freq_mhz`; fallback 300/1100 |
| `SetGPUFreq(mhz)` | clamp a bounds; escribe `gt_min_freq_mhz = min` y luego `gt_max_freq_mhz = mhz` |
| ASPM | lee `/sys/module/pcie_aspm/parameters/policy` y extrae lo que está entre `[...]`; escribe el policy tal cual |
| WiFi power save | `iw dev` / `iw dev X get power_save` / `set power_save on\|off` + fallback `/sys/module/iwlwifi/parameters/power_save` (`Y`/`N`) |
| Kbd backlight | glob `/sys/class/leds/*kbd_backlight`; get = `brightness != "0"`; set on = `max_brightness`, off = `"0"` |
| Audio power save | `/sys/module/snd_hda_intel/parameters/power_save` (`"0"`=off) + `.../power_save_controller` (`Y`/`N`) |
| LCD brightness | glob `/sys/class/backlight/*`: `(cur*100)/max`; fallback `brightnessctl -m` (campo 4 sin `%`); fallback 100 |
| `SetLCDBrightness(p)` | clamp 1..100; `(p*max)/100` en todos los backlights; **además** `brightnessctl set N%` |
| Bluetooth / WiFi enable | `rfkill list bluetooth\|wifi` → off si contiene `Soft blocked: yes`; `rfkill block\|unblock` |
| Autosuspend | globs `/sys/bus/usb/devices/*/power/control` y `/sys/bus/pci/devices/*/power/control`: get = alguno `"auto"`; set: `"on"` o `"auto"` |
| Watchdog | `/proc/sys/kernel/nmi_watchdog` `"1"` |
| VM writeback | `/proc/sys/vm/dirty_writeback_centisecs`, clamp 100..6000 |
| ProcessPurge | escribe `3` en `/proc/sys/vm/drop_caches` |

Los fallbacks **son parte del contrato**: si un path no existe, la función devuelve el fallback de la
tabla y el daemon no muere (el Rust actual ya cumple buena parte de esto; no lo rompas).

## 3. CLI y servicio (`main.go` + `service/service.go`)

Flags de Go, salida exacta (importan los strings porque son la interfaz que Juan usa):

| Flag | Salida |
|---|---|
| `--daemon` \| `daemon` | corre el loop en foreground (requiere root) |
| `--start` \| `start` | `⚡ WattWarden background daemon started.` |
| `--stop` \| `stop` | `🛑 WattWarden background daemon stopped.` |
| `--status` \| `status` | `WattWarden Daemon Status: [ACTIVE] (Running in background)` / `... : [INACTIVE]` |
| `--brightness on\|off` | `Auto-brightness set to: true\|false` |
| `--brightness` (sin arg) | `Auto-brightness is currently: <bool>` |
| `--install-service` | `✅ WattWarden background service installed and started successfully.` |
| `--uninstall-service` | `✅ WattWarden background service uninstalled.` |
| sin flag | TUI dashboard |
| sin root | `Error: You must run this program with administrator/root privileges to change system power settings.` + exit 1 |

`StartBackgroundDaemon`: `SyncInstalledBinary()` → setea `auto_extreme_enabled = true` y guarda config →
si existe `/etc/systemd/system/wattwarden.service` hace `systemctl restart` y si `IsDaemonActive()` ok,
listo → si no, arranca el loop in-process → si no, spawnea daemon detached.

`SyncInstalledBinary`: si el ejecutable actual **no** es `/usr/local/bin/wattwarden` y `geteuid()==0`,
copia el binario ahí con modo 0755.

Unidad systemd (texto exacto):
```
[Unit]
Description=WattWarden Auto Power and Hardware Management Daemon
After=multi-user.target

[Service]
Type=simple
ExecStart=/usr/local/bin/wattwarden --daemon
Restart=always
RestartSec=3
KillMode=process

[Install]
WantedBy=multi-user.target
```

## 4. Niveles low/medium/high (extensión de Juan, no existe en Go)

Contradicción que hay que resolver: el spec original decía "en Rust el techo ya es 100%" → **falso**,
Go Clampea al 40% y el binario que corre en la Vostro es el Rust roto. Regla nueva:

- `high` (**default**) = **§1.1 y §1.2 verbatim**, techo 40%, escalones discretos, 5 s, EPP `power`,
  brillos 12/20/30. Cero diferencias con Go.
- `medium` = mismo lazo, `ceiling 0.7`, tick 5 s, `idle_cores max(2, ncpu/2)`, brillos +10.
- `low` = mismo lazo, `ceiling 1.0`, tick 5 s, todos los cores en carga, EPP
  `balance_power`/`balance_performance`, turbo según umbral 0.5, brillos +20.

O sea: los niveles **parametrizan el lazo de Go** (techo, cores, EPP, umbral de turbo, delta de brillo)
y `high` es exactamente Go. Ningún nivel cambia el algoritmo: escalones discretos, 40%-base, los mismos
writes.

## 5. Verificación obligatoria (sin esto no está terminado)

1. `cargo build --all-targets && cargo test && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`.
2. Tests con sysfs simulado: raíz override por env `WATTWARDEN_SYSFS_ROOT` (default `/`) para poder
   apuntar a un `temp_dir()` con estructura falsa y **afirmar los valores exactos escritos** en cada
   caso: a batería con load 0.0 / 0.2 / 0.5 / 1.0 y enchufado. Deben dar exactamente:
   `discretePower` 0 / 0.333 / 0.667 / 1.0; `targetCPU = min + dp*(min+0.4*(hwMax-min) - min)`; etc.
3. Test de compatibilidad de config: un JSON de Go `{"auto_extreme_enabled":true,"auto_brightness":false}`
   se lee en Rust sin error y `auto_brightness == false`; y lo que escribe Rust lo puede leer Go.
4. Golden test de CLI: `--status`, `--brightness`, `--help` con los strings de §3.
5. En hardware (Vostro, con batería): mismo día, correr binario Go (build de `master`) y binario Rust
   y comparar `scaling_max_freq`, `online`, `constraint_0_power_limit_uw`, `gt_max_freq_mhz`, brillo
   tras 60 s en cada rama. Los valores tienen que coincidir.
   Arnés listo: `sudo scripts/ab_parity.sh <bin-go> <bin-rust> 30` (snapshot + restore automático).
   ⚠️ **Arquitecturas distintas**: la Pi es `aarch64`, la Vostro y la G15 son `x86_64`. El binario
   Rust de la prueba A/B se compila **en la Vostro/G15** (`cargo build --release`), no se copia
   desde la Pi. El binario Go de referencia también (o `GOOS=linux GOARCH=amd64 go build` desde acá,
   que sí es válido porque Go cross-compila). Con Rust, cross-compilar a x86_64 desde la Pi requiere
   toolchain extra → no vale la pena.

---

## 6. macOS y Windows — paridad con el Go (`hal/backend_darwin.go`, `hal/backend_windows.go`)

El Rust no tenía paridad: `macos/mod.rs` y `windows/mod.rs` devolvían valores inventados (rangos
absolutos 1000..4500 MHz, límite 3500, EPP `balance` vs el `default` de Go, `is_charging` por
PowerShell, perfiles `powercfg -setactive <GUID>` que Go no usa) y tenían **0 tests**. Ahora cada
función de Go está portada, y los tests del Go están portados también.

Los dos módulos se compilan en **todas** las plataformas (no sólo en la suya): son wrappers de CLI
(salvo `GetSystemPowerStatus`, que va `cfg(windows)`), y esa decisión es la que permite que los tests
de paridad corran acá en Linux contra binarios falsos en `PATH` (`crates/wattwarden-platform/src/
{exec,parse}.rs`). `lib.rs` sigue re-exportándolos como `PlatformBackend` sólo en su plataforma.

### 6.1 DarwinBackend → `wattwarden-platform::macos`

| Go | Rust | Regla |
|---|---|---|
| `GetOS` | `MacOsBackend::os_name` | `"macOS"` (lo consume la TUI para elegir filas y la línea de resumen) |
| `GetNumCPUs` | `CpuGovernor::num_cpus` | `available_parallelism` (Go `runtime.NumCPU`), fallback 1 |
| `GetCores` | `CpuGovernor::online_cores` | = `num_cpus` |
| `SetCores` | `CpuGovernor::set_online_cores` | no-op |
| `GetFreqLimit` / `SetFreqLimit` | `freq_limit` / `set_freq_limit` | `set` no-op; `get` → `Err(Unsupported)` (Go devuelve 0 = "no hay control") |
| `GetBatteryPercentage` | `PowerSource::battery_percentage` | `pmset -g batt`, dígitos antes del `%`, fallback 100 |
| `IsCharging` | `is_charging` | `contains("AC Power") && !contains("discharging")`; `pmset` que falla ⇒ `""` ⇒ **false** (Go, no el `true` que había) |
| `GetBatteryTime` | `time_remaining` | `H:MM remaining` → `H:MM`; si no, enchufado → `"Charging"`; si no → `"Calculating..."` |
| `GetPowerConsumptionWatts` | `consumption_watts` | `ioreg -rn AppleSmartBattery`, `abs(Current)*Voltage/1e6`, 0 si falta alguno |
| `GetRAPLPL1/PL2` + setters | `rapl: None` | macOS no expone RAPL → sin controlador |
| `GetTurbo` / `SetTurbo` | `turbo_enabled` / `set_turbo_enabled` | `true` / no-op |
| `GetEPP` / `SetEPP` | `energy_performance_preference` / `set_…` | **`"default"`** / no-op |
| `GetGPUFreq` / `SetGPUFreq` | `gpu: None` | sin control de GPU |
| `GetASPM` / `SetASPM` | `aspm: None` | sin ASPM |
| `GetLCDBrightness` | `DisplayManager::brightness_percent` | `brightness -l`, `int(v*100)`, fallback 100 |
| `SetLCDBrightness` | `set_brightness_percent` | clamp 1..100, `brightness 0.00..1.00` |
| `GetBluetooth` | `peripherals::bluetooth_enabled` | `defaults read … ControllerPowerState` != "0" |
| `SetBluetooth` | `set_bluetooth_enabled` | `defaults write … -int v` **y** `blueutil --power v` |
| `getMacWifiDevice` / `GetWifiEnable` / `SetWifiEnable` | `wifi_enabled` / `set_wifi_enabled` | `networksetup -listallhardwareports` (fallback `en0`), `-getairportpower` / `-setairportpower` |
| `GetWifiPowerSave` / `Set…` | tweaks | `false` / no-op |
| `GetKbdBacklight` / `Set…` | peripherals | `false` / no-op |
| `GetAudioPowerSave` / `Set…` | tweaks | `false` / no-op |
| `GetAutosuspend` / `Set…` | tweaks | **`false`** / no-op (antes `true`) |
| `GetWatchdog` / `SetWatchdog` | `nmi_watchdog` / `set_nmi_watchdog` | **`true`** / no-op (antes `false`) |
| `GetVMWriteback` / `SetVMDirty` | `vm_writeback_seconds`(5) / set | Go 500 cs = 5 s (el default del trait reescala a 500 cs) |
| `SetNMIWatchdog` | `set_nmi_watchdog` | no-op |
| `SetBrightnessTarget` / `SetRefreshRate` / `SetHyprEffects` | — | no-op en Go; sin trait equivalente (no se inventó API) |
| `ProcessPurge` | `process_purge` | `purge` |
| `ApplyModePerformance` | `apply_mode_performance` | `pmset -a lowpowermode 0` + `tcpkeepalive 1` + `displaysleep 10` |
| `ApplyModeExtreme` | `apply_mode_extreme` | `pmset -a lowpowermode 1` + `tcpkeepalive 0` + `displaysleep 3` |
| `ApplyModeRestore` | `apply_mode_restore` | igual que Performance |
| `getMacLoad` | `MacOsBackend::load_average` | `sysctl -n vm.loadavg`, primer campo; el lazo compartido divide por NCpu (Go divide dentro) |
| `GetAutoBrightness` / `SetAutoBrightness` | `Config::auto_brightness` + `DaemonRunner` | compartido: el daemon relee la config |
| `IsDaemonRunning` / `StopDaemon` / `StartAutoExtremeDaemon` | `DaemonRunner` + `PidManager` | compartido con Linux |

**Sin par (Go tampoco lo implementa) → sigue `Unsupported`/ausente:** charge threshold, RAPL, GPU,
ASPM, C-states y compositor activo. En Rust el patrón es `None`/`Err(Unsupported)`, nunca un rango
absoluto inventado.

### 6.2 WindowsBackend → `wattwarden-platform::windows`

| Go | Rust | Regla |
|---|---|---|
| `GetOS` | `WindowsBackend::os_name` | `"Windows"` |
| `GetNumCPUs` / `GetCores` / `SetCores` | `num_cpus` / `online_cores` / `set_online_cores` | = NCpu / no-op |
| `GetFreqLimit` / `SetFreqLimit` | `freq_limit` / `set_freq_limit` | `set` no-op; `get` → `Err(Unsupported)` |
| `getPowerStatus` | `get_power_status` | `kernel32!GetSystemPowerStatus` (FFI, sin fork), `cfg(windows)` |
| `GetBatteryPercentage` | `battery_percentage` | `BatteryLifePercent` ≤100, fallback 100 |
| `IsCharging` | `is_charging` | `ACLineStatus == 1`, fallback `true` |
| `GetBatteryTime` | `time_remaining` | `"Charging"`/`"Calculating..."`/`"%dh %02dm"` |
| `GetPowerConsumptionWatts` | `consumption_watts` | `powershell` BatteryStatus, `Voltage*Discharge/1e6` sólo descargando |
| `GetRAPLPL1/PL2` + setters | `rapl: None` | sin RAPL |
| `GetTurbo` | `turbo_enabled` | `powercfg /query … PERFBOOSTMODE` no es `0x00000000` |
| `SetTurbo` | `set_turbo_enabled` | `2`/`0` en **AC y DC** + `-setactive` (antes faltaba DC) |
| `GetEPP` / `SetEPP` | `energy_performance_preference` / set | **`"default"`** / no-op |
| `GetGPUFreq` / `Set…` | `gpu: None` | sin control de GPU |
| `GetASPM` / `Set…` | `aspm: None` | sin ASPM |
| `GetLCDBrightness` | `brightness_percent` | `powershell` WmiMonitorBrightness, fallback 100 |
| `SetLCDBrightness` | `set_brightness_percent` | clamp **0**..100 (Go permite 0), WmiSetBrightness |
| `GetBluetooth` | `bluetooth_enabled` | `true` |
| `SetBluetooth` | `set_bluetooth_enabled` | `powershell Set-Service bthserv Running/Stopped` (antes no-op) |
| `GetWifiEnable` / `SetWifiEnable` | `wifi_enabled` / `set_wifi_enabled` | `netsh interface show/set interface` (antes `true`/no-op) |
| `GetWifiPowerSave` / `Set…` | tweaks | `false` / no-op |
| `GetKbdBacklight` / `Set…` | peripherals | `false` / no-op |
| `GetAudioPowerSave` / `Set…` | tweaks | `false` / no-op |
| `GetAutosuspend` / `Set…` | tweaks | **`false`** / no-op (antes `true`) |
| `GetWatchdog` / `SetWatchdog` | `nmi_watchdog` / set | **`true`** / no-op (antes `false`) |
| `GetVMWriteback` / `SetVMDirty` | `vm_writeback_seconds`(5) / set | 500 cs = 5 s |
| `setWinProcThrottle` | `set_proc_throttle` | `PROCTHROTTLEMAX` clamp 1..100 en AC y DC + `-setactive` |
| `ApplyModePerformance` / `ApplyModeRestore` | `apply_mode_performance` / `apply_mode_restore` | throttle 100 |
| `ApplyModeExtreme` | `apply_mode_extreme` | throttle 1 + brillo 10 |
| `getWinLoad` | `WindowsBackend::load_average` | `typeperf`, fracción `0..1` × NCpu para el lazo compartido |
| `GetAutoBrightness` / `SetAutoBrightness` | `Config::auto_brightness` + `DaemonRunner` | compartido |
| `IsDaemonRunning` / `StopDaemon` / `StartAutoExtremeDaemon` | `DaemonRunner` + `PidManager` | compartido |
| `SetBrightnessTarget` / `SetRefreshRate` / `SetHyprEffects` / `SetNMIWatchdog` | — | no-op en Go; sin trait equivalente |

**Sin par → `Unsupported`/ausente:** charge threshold, RAPL, GPU, ASPM, C-states, compositor.

### 6.3 Tests

Portados de `backend_darwin_test.go` y `backend_windows_test.go` (mismos binarios falsos en `PATH`,
mismas aserciones) en los `mod tests` de cada backend. Además, toda la aritmética de parseo está en
`crate::parse` (sin `cfg`), con un test por regla de Go; eso es lo que corre en Linux. Detalle y
resultados en `delegacion/EVIDENCIA-mac-win.md`.

### 6.4 No verificado en hardware

No hay máquina Apple ni Windows en este entorno, y no se puede linkear contra `kernel32` desde Linux
(no hay MinGW/SDK). Verificado: los 5 `clippy --all-targets -- -D warnings` (host + 4 targets) y los
tests de paridad de macOS/Windows ejecutados en Linux. **No verificado en hardware**: la ejecución
real de `pmset`/`ioreg`/`networksetup`/`brightness`/`blueutil` en macOS y de `GetSystemPowerStatus`/
`powercfg`/`powershell`/`netsh`/`typeperf` en Windows, ni el link contra `kernel32`.

### 6.5 Dashboard: `GetOS()` decide filas y línea de resumen

Go `buildMenuItems` (`ui/cli.go:350-585`) arma un menú distinto según `GetOS()` (Linux trae
HARDWARE LIMITS/PERIPHERALS/SYSTEM TWEAKS; Windows sólo HARDWARE LIMITS con Turbo + PERIPHERALS &
NETWORKING + SYSTEM MEMORY; macOS sólo PERIPHERALS & NETWORKING + SYSTEM MEMORY), y la línea de
resumen (`cli.go:171-172`) es una sola para todas: `OS: %s | Battery: %d%% (%s) | Est: %s | Power:
%.1fW`.

El Rust tenía la lista de Linux cableada en todas las plataformas y la línea de resumen con
`"OS: Linux"` fijo. Ahora `MacOsBackend/WindowsBackend/LinuxBackend/FallbackBackend::os_name()`
(Go `GetOS()`) alimenta `build_menu(os)` y `summary_line(...)`, con tests golden por OS. El `match`
es en runtime (no `cfg`) a propósito: así todos los `ActionItem` se construyen en todos los targets y
los que esa plataforma esconde no disparan `dead_code` en el clippy cruzado. `AutoExtremeLevel` es la
única fila extra (extensión Rust documentada en §4) y `k`/`j`/`h`/`l` los únicos atajos extra; las
teclas `w`/`s`/`a`/`d` aceptan mayúscula y minúscula como Go.
