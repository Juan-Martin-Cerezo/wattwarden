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
