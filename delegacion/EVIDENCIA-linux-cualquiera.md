# Evidencia — Linux en CUALQUIER computadora (AMD, ARM, sin batería, sin Intel RAPL)

Rama: `feat/paridad-go-rust`. Host de trabajo y de prueba: **Raspberry Pi 5, `aarch64`**
(`Linux 6.18.34+rpt-rpi-2712`), rustc/cargo 1.98.1.
No se tocó `/home/juan/wattwarden`, no se usó `sudo`, no se tocó `/etc`, no se corrió el A/B
y no se ejecutó el daemon como root.

## 0. Objetivo

La regla de Juan —*"cualquier ajuste se adapta a cada hardware de computadora"*— y `AGENTS.md`
(*"The application must boot flawlessly on a desktop PC, rack server, Raspberry Pi, or container"*).
Antes de esto los dos únicos equipos probados eran notebooks Intel x86_64, y había nodos
**Intel-only cableados**: `intel-rapl:0`, `intel_pstate/no_turbo`, EPP, `gt_max_freq_mhz` y `BAT0`.

## 1. La máquina real de prueba (la Pi, `aarch64`)

```
$ uname -srm
Linux 6.18.34+rpt-rpi-2712 aarch64

$ cat /proc/device-tree/model
Raspberry Pi 5 Model B Rev 1.0

$ ls /sys/class/powercap
ls: cannot access '/sys/class/powercap': No such file or directory

$ ls /sys/class/backlight          # existe pero vacío

$ ls /sys/class/power_supply       # existe pero vacío (sin BAT0)

$ ls /sys/class/drm
card0
card1
card1-HDMI-A-1
card1-HDMI-A-2
card1-Writeback-1
card1-Writeback-2
renderD128
version

$ ls /sys/devices/system/cpu/cpufreq
boost
ondemand
policy0

$ cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_driver
cpufreq-dt

$ ls /sys/devices/system/cpu/intel_pstate
ls: cannot access '/sys/devices/system/cpu/intel_pstate': No such file or directory

$ ls /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference
ls: cannot access '.../energy_performance_preference': No such file or directory

$ cat /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq
1500000
2400000
```

O sea: **sin powercap**, **sin `intel_pstate`**, **sin EPP**, **sin batería**, **sin backlight** y
con `cpufreq-dt` + `cpufreq/boost`. Es exactamente el caso (a) del enunciado, medido en hardware real.

### Build + `--status` en la Pi (como usuario normal, sin root)

```
$ cargo build --release
    Finished `release` profile [optimized] target(s) in 1m 09s

$ ./target/release/wattwarden --status
WattWarden Daemon Status: [INACTIVE]
```

Sale `0` y no toca nada: `--status` sólo lee el PID y consulta al servicio. El binario arranca,
informa y termina sin fallar en una máquina que **no tiene RAPL Intel ni batería**.

## 2. Qué se hizo dinámico

| Nodo | Antes | Ahora |
|---|---|---|
| **RAPL / powercap** | ruta fija `sys/class/powercap/intel-rapl:0` | se **glob**ea `/sys/class/powercap/*` y se acepta **toda** zona que exponga una tabla de constraints (`constraint_0_name`), sea cual sea su nombre (`intel-rapl:0`, `intel-rapl-mmio:0`, `amd-rapl:0`, …). Se soportan **varios** dominios a la vez (multi-package) y la **ausencia total**. |
| **Turbo** | se asumía `intel_pstate/no_turbo` | `intel_pstate/no_turbo` si existe → si no `cpufreq/boost` → si no existe **ninguno**, no se escribe (y `turbo_enabled()` sigue reportando el default de Go). |
| **EPP** | se escribía el nodo | se escribe **sólo si el nodo existe** en cada CPU, y con un valor que esté en `energy_performance_available_preferences` (o el más cercano); si el nodo no existe, no se escribe ni se crea. |
| **GPU** | `card1/gt_max_freq_mhz` → `card0/gt_max_freq_mhz` | se recorren **todas** las `cardN` (orden de Go: `card1`, `card0`, luego el resto) y se prueba el nodo en la raíz de la card y bajo su link `device/`. Cardinales de conector (`card1-HDMI-A-1`, `renderD128`, `version`) **no** son cards. Sin nodo de frecuencia en ninguna: **no se escribe** y el controlador no se construye. |
| **Backlight** | se asumía un dispositivo | clase vacía o ausente ⇒ `DisplayManager` no se construye ⇒ no se toca nada y la UI lo oculta. |
| **Batería** | asumía `BAT0` | sin ninguna `power_supply` con batería ⇒ **Stationary AC Mains**: arranca, informa 0 % y estado de línea AC, y **no escribe nada relacionado con carga**. |

Extra: el descubrimiento de RAPL/GPU ocurre **una sola vez**, al construir el backend. Los
"no se escribe por falta de nodo/rango" quedan en `debug!` (el daemon corre con filtro `info`), así
que el log no se ensucia por tick; el daemon emite **una** línea al arrancar con las capacidades
detectadas (`cat > /var/log/wattwarden.log`):

```
Hardware capabilities: battery=false (stationary_mains=true), cpu_freq=true, cpu_cores=true,
rapl=false, gpu=false, backlight=false, charge_threshold=false, compositor=false
```

## 3. Qué ausencias se manejan y cómo

| Ausencia | Comportamiento |
|---|---|
| Sin `/sys/class/powercap` (Pi, ARM, contenedor) | `LinuxRapl::with_root` → `InterfaceNotFound` (a `debug!`), el backend queda con `rapl: None`, la escalera saltea RAPL y **no escribe** nada. `capabilities().has_rapl == false`. |
| Sin `intel_pstate/no_turbo` (AMD, ARM) | se usa `cpufreq/boost`; si tampoco existe, `set_turbo_enabled` no escribe. |
| Sin EPP (AMD `acpi-cpufreq`, ARM `cpufreq-dt`) | no se escribe ni se **crea** `energy_performance_preference`; el `scaling_governor` que sí existe se sigue ajustando (paridad Go). |
| Sin nodo de frecuencia de GPU (Pi v3d/axi, AMD DPM-only, servidor) | `gpu: None`, no se escribe nada. |
| Sin `/sys/class/backlight` o clase vacía (servidor, contenedor) | `backlight: None`, no se escribe nada. |
| Sin batería (desktop, servidor, contenedor) | fuente estacionaria: `battery_percentage()==0`, `is_stationary()==true`, sin pánicos ni `?` que aborten el arranque; `battery_charge_limit` del config se ignora porque `supports_threshold()` es `false`. |
| Nodo writable ausente | toda escritura pasa por un chequeo de existencia (`SysfsRoot::exists`) antes de `fs::write`: un nodo que no existe **no se crea**. |

### Nota de interpretación sobre EPP (honestidad)

`PARITY.md` §2 fija como contrato de paridad con el Go: *"EPP: lee `cpu0/cpufreq/energy_performance_preference`;
escribe **todos** `cpu*/cpufreq/energy_performance_preference`"*. El gate nuevo es: **el nodo tiene que
existir** y, **cuando el kernel publica `energy_performance_available_preferences`, el valor pedido tiene que
estar en esa lista** (si no, se mapea al más cercano de `EPP_ORDER` o no se escribe). Si el nodo existe pero el
kernel **no** publica la lista, se mantiene el write-through de Go para no romper la paridad (los golden tests
`go_parity::f_epp_*` y `daemon::test_c/test_d` la afirman). En el hardware que motiva el cambio (AMD/ARM) el nodo
directamente **no existe**, y ahí no se escribe nada.

## 4. Tests nuevos

### Envs completos contra sysfs falso — `crates/wattwarden-daemon/tests/hw_environments.rs` (nuevo)

Cada uno arma el árbol, hace `PlatformBackend::with_root`, corre **el lazo real del daemon**
(`apply_boot_settings` con perfil Performance + `apply_logic_step` en carga 0 / mitad / máxima +
`apply_brightness_step`) con **todos los opt-in encendidos**, y afirma: (1) **no falla** (ni panic ni
`Err` que aborte) y (2) **no aparece ni desaparece ningún nodo** (el conjunto de archivos es idéntico
antes y después ⇒ no se escribió en ningún nodo inexistente ni se creó un `BAT0`/EPP/powercap nuevo).

* `arm_pi_boots_without_rapl_battery_or_backlight` — (a) ARM/Pi: sin powercap, sin `intel_pstate`,
  sin EPP, sin batería, con `cpufreq`. Afirma que `boost` y el governor de la Pi **sí** se usan, que
  la frecuencia queda dentro del rango descubierto (1.5–2.4 GHz) y que **no** aparecen
  `intel_pstate/no_turbo`, `energy_performance_preference`, powercap ni `BAT0`.
* `amd_vendor_named_powercap_is_discovered_and_intel_nodes_are_untouched` — (b) AMD: zona
  `amd-rapl:0` descubierta (`domains()` la ve, `pl1_bounds_watts() == Some((2, 65))`), se escribe
  **dentro de su propio rango** (nunca el literal 115 W), y no se tocan `no_turbo` ni EPP.
* `desktop_without_battery_boots_and_writes_nothing_charge_related` — (c) Desktop: sin `BAT*`,
  fuente estacionaria, `battery_charge_limit = Some(80)` no crea ninguna batería.
* `container_without_backlight_powercap_or_battery_boots` — (d) Contenedor: sin backlight, sin
  powercap, sin batería. El único archivo que cambia es el `proc/loadavg` que reescribe el test.

### Unit tests de controlador

* `linux::rapl`:
  * `a_non_intel_powercap_zone_name_is_discovered` — zona `amd-rapl:0` (nombre vendor) descubierta,
    techo propio respetado (`999 W` ⇒ `65 W`).
  * `every_discovered_zone_is_written_within_its_own_range` — dos zonas (multi-package): ambas
    descubiertas y escritas, cada una en su rango.
  * `powercap_entries_without_constraints_are_not_zones` — una entrada de powercap sin constraints
    no hace que el controlador se declare presente (⇒ `Err`).
* `linux::gpu`:
  * `a_card_other_than_card0_or_card1_is_discovered` (`card2`).
  * `the_frequency_node_is_found_under_the_device_link` (layout `cardN/device/…`).
  * `connector_entries_are_not_mistaken_for_cards` (`card0-HDMI-A-1`, `renderD128`, `version`).
  * `a_write_never_creates_a_missing_writable_node`.
* `linux::cpu`: `epp_absent_is_not_written_nor_created` (con governor presente: se ajusta el governor,
  **no** se crea el nodo EPP).
* `linux::battery`: `empty_power_supply_class_is_stationary_and_creates_nothing`.

Se conservaron **todos** los tests previos (`go_parity.rs`, `hw_agnostic.rs`, los unit tests de
plataforma y daemon): el único cambio en ellos fue que el helper de árbol falso de `rapl.rs` ahora
crea una zona con `constraint_0_name`, que es lo que el descubrimiento busca (antes la ruta venía fija).

## 5. Aceptación (los 4 en 0) — host aarch64

```bash
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && \
  cargo clippy --workspace --all-targets -- -D warnings
```

```
FMT_CHECK_OK
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.07s     (build --all-targets)

     Running unittests src/main.rs (wattwarden-cli)
test result: ok. 8 passed; 0 failed; ...
     Running unittests src/lib.rs (wattwarden-core)
test result: ok. 16 passed; 0 failed; ...
     Running unittests src/lib.rs (wattwarden-daemon)
test result: ok. 26 passed; 0 failed; ...
     Running tests/hw_environments.rs (wattwarden-daemon)      <-- NUEVO
test result: ok. 4 passed; 0 failed; ...
     Running unittests src/lib.rs (wattwarden-platform)
test result: ok. 85 passed; 0 failed; ...
     Running tests/go_parity.rs (wattwarden-platform)
test result: ok. 9 passed; 0 failed; ...
     Running tests/hw_agnostic.rs (wattwarden-platform)
test result: ok. 6 passed; 0 failed; ...
     Running unittests src/lib.rs (wattwarden-tui)
test result: ok. 8 passed; 0 failed; ...

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s     (clippy -D warnings)
```

**162 tests, 0 fallos, 0 warnings de clippy** (`-D warnings` ⇒ `Finished` = cero avisos).

## 6. Lo que NO se verificó (honestidad)

* **AMD real**: no hay ninguna máquina AMD en este entorno. El caso (b) está cubierto con un sysfs
  falso (`amd-rapl:0` + `acpi-cpufreq` sin EPP) y con el descubrimiento name-agnostic verificado en
  las unit tests; **no** se ejecutó en hardware AMD.
* **Desktop/servidor/contenedor reales**: cubiertos con sysfs falso; la Pi (sin batería, sin RAPL,
  sin backlight) es el único hardware real de estas ausencias.
* **RAPL multi-package real**: verificado en sysfs falso, no en un equipo dual-socket.
* En la Pi **no** se corrió el daemon con root (prohibido por el enunciado): lo verificado ahí es
  `cargo build --release` + `--status` como usuario normal.
* La rama `feat/paridad-go-rust` no tiene CI en la Pi: los 4 comandos de aceptación se corrieron a mano.

## 7. Comandos para reproducir

```bash
cd /home/juan/ww-rust

# aceptación
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && \
  cargo clippy --workspace --all-targets -- -D warnings

# entornos (ARM/Pi, AMD, desktop sin batería, contenedor)
cargo test -p wattwarden-daemon --test hw_environments -- --nocapture

# descubrimiento dinámico (RAPL/GPU/turbo/EPP/batería)
cargo test -p wattwarden-platform --lib linux::

# la máquina real (Pi, aarch64), sin root
cargo build --release && ./target/release/wattwarden --status
```
