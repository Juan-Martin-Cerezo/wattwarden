# Evidencia — CI verde en las 3 plataformas (clippy macOS/Windows)

Rama: `feat/paridad-go-rust`. Host de trabajo: Linux `aarch64-unknown-linux-gnu`, rustc/cargo 1.98.1.
No se tocó `/home/juan/wattwarden` (sólo lectura), ni `.github/workflows/`, ni se corrió el A/B.

## 0. Objetivo

`cargo clippy --workspace --all-targets -- -D warnings` sin un solo warning en los tres targets del
CI (`ubuntu-latest`, `macos-latest`, `windows-latest`) y CI verde.

## 1. Targets de lint cruzado instalados

```bash
rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-gnu x86_64-pc-windows-msvc
```

`macos-latest` es hoy arm64 y `windows-latest` es MSVC, así que además de los dos targets del
enunciado linteé los hosts reales del CI y los dos x86_64/MinGW restantes.

## 2. Antes (rojo)

`cargo clippy --workspace --all-targets --target x86_64-apple-darwin -- -D warnings`:

```
error[E0432]: unresolved import `wattwarden_platform::linux`
 --> crates/wattwarden-daemon/src/daemon.rs:8:26
  |
8 | use wattwarden_platform::linux::LinuxBackend;
  |                          ^^^^^ could not find `linux` in `wattwarden_platform`
  |
note: found an item that was configured out
 --> crates/wattwarden-platform/src/lib.rs:2:9
  |
1 | #[cfg(target_os = "linux")]
  |       ------------------- the item is gated behind the `linux` feature
2 | pub mod linux;
  |         ^^^^^

error: could not compile `wattwarden-daemon` (lib) due to 1 previous error
warning: build failed, waiting for other jobs to finish...
```

Idéntico en `x86_64-pc-windows-gnu`. Punto clave: **no era "un warning de clippy"**. El crate
`wattwarden-daemon` (que usan el CLI y la TUI) no compilaba fuera de Linux, clippy moría en el
primer error y eso tapaba todos los avisos de atrás (unused imports, dead_code, etc.). Por eso
nadie los había visto nunca desde el entorno Linux.

## 3. Después — clippy por target

Corrida **en frío**: `CARGO_TARGET_DIR` vacío (se recompilaron todas las dependencias de cada
target, no hay artefactos cacheados de por medio) y borrado al terminar.

```bash
cd /home/juan/ww-rust
CARGO_TARGET_DIR=/tmp/commandcode/xtarget \
  cargo clippy --workspace --all-targets --target <TARGET> -- -D warnings
```

```
### cargo clippy --workspace --all-targets --target x86_64-apple-darwin -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 33.13s

### cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.59s

### cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.81s

### cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.94s
```

Con `-- -D warnings` cualquier warning es error, así que `Finished` implica **cero warnings** y
cero errores en los cinco crates (`wattwarden-core`, `-platform`, `-daemon`, `-tui`, `-cli`) para
los cuatro targets.

## 4. Aceptación local (host Linux) — los 5 en 0

```bash
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && \
  cargo clippy --workspace --all-targets -- -D warnings
```

```
FMT_OK
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s      (build --all-targets)
test result: ok. 8 passed; 0 failed; ...      (cli, unittests src/main.rs)
test result: ok. 16 passed; 0 failed; ...     (core)
test result: ok. 26 passed; 0 failed; ...     (daemon)
test result: ok. 59 passed; 0 failed; ...     (platform)
test result: ok. 9 passed; 0 failed; ...      (platform, tests/go_parity.rs)
test result: ok. 6 passed; 0 failed; ...      (platform, tests/hw_agnostic.rs)
test result: ok. 6 passed; 0 failed; ...      (tui)
test result: ok. 0 passed; 0 failed; ...      (doc-tests x4)
CLIPPY_HOST:
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
```

130 tests, 0 fallos. **Ningún test se borró**: se gatearon a Linux los que construyen un `/sys` y
`/proc` falsos (en Linux siguen corriendo los mismos 130 que antes) y se agregó uno nuevo que
corre *sólo* fuera de Linux.

## 5. Qué cambié y por qué (la causa, no la manifestación)

### 5.1 Causa raíz: el daemon y la TUI hablaban la API concreta de Linux

`wattwarden-daemon` y `wattwarden-tui` usaban nombres que sólo existen en el backend Linux:

| uso (antes) | dónde | problema |
|---|---|---|
| `wattwarden_platform::linux::LinuxBackend` | `daemon.rs:8` | el módulo `linux` está `cfg(target_os = "linux")` |
| `backend.root()` (sysfs crudo) | `daemon.rs`, `tui/app.rs` x2, `tui/ui.rs` | sólo `LinuxBackend` tiene `root()` |
| `cpu.discovered_freq_bounds()` | `daemon.rs` | método inherente de `LinuxCpuGovernor`, no estaba en `CpuGovernor` |
| `gpu.discovered_gpu_bounds()` | `daemon.rs` | ídem en `GpuController` |
| `rapl.pl1/pl2_bounds_watts()` | `daemon.rs` | ídem en `RaplController` |

Los backends macOS/Windows/fallback ya existían y ya implementan los traits de `wattwarden-core`.
El arreglo es que **la API que consumen el daemon y la TUI exista en todos los backends**, con la
semántica de cada plataforma, sin tocar el camino Linux.

| archivo | cambio |
|---|---|
| `wattwarden-core/src/traits.rs` | `CpuGovernor::discovered_freq_bounds`, `GpuController::discovered_gpu_bounds` y `RaplController::pl1_bounds_watts`/`pl2_bounds_watts`, todos con default `None` (= "no hay rango descubrible, no escribas"). Un default `None` es seguro: una plataforma que no descubre rango no inventa un valor. |
| `wattwarden-core/src/traits.rs` | `SystemTweaksController::vm_writeback_centisecs`/`set_vm_writeback_centisecs` (Go `GetVMWriteback`/`SetVMWriteback` van en centisegundos, la TUI ajusta de a 1 cs). El default escala desde `vm_writeback_seconds`, que en macOS/Windows da exactamente el `500` de Go. |
| `platform/linux/{cpu,gpu,rapl}.rs` | los mismos métodos pasan de inherentes a la impl del trait (cuerpo idéntico, una sola fuente de verdad). |
| `platform/linux/mod.rs` | `LinuxBackend::load_average()` = `/proc/loadavg` por el `SysfsRoot` (misma lógica que estaba en el daemon, sigue siendo relocalizable por `WATTWARDEN_SYSFS_ROOT`). |
| `platform/linux/tweaks.rs` | override de `vm_writeback_centisecs` con el valor crudo del nodo (la TUI no pierde los pasos de 1 cs). |
| `platform/macos/mod.rs` | `load_average()` = `sysctl -n vm.loadavg` (Go `getMacLoad`, que también invoca `sysctl`); se devuelve el valor crudo porque el lazo compartido ya divide por `NumCPU`, igual que Go. El lazo corre cada 5 s, no es el hot loop de 300 ms de brillo. |
| `platform/windows/mod.rs` | `load_average()` = `typeperf \Processor Information(_Total)\% Processor Time` (Go `getWinLoad`), multiplicado por `num_cpus` para que `load / ncpu` del lazo reproduzca la misma potencia que Go. |
| `platform/fallback/mod.rs` | `load_average()` = `0.0` (paso idle, no se inventa carga). |
| `wattwarden-daemon/src/daemon.rs` | usa `PlatformBackend` (que en Linux **es** `linux::LinuxBackend`) y `backend.load_average()`. El resto del lazo es el mismo código. |

### 5.2 Un bug cross-platform real que traía el CI de yapa

`PidManager::is_running()` decidía si el daemon está vivo mirando `/proc/<pid>`. Eso es Linux:
en macOS (sin procfs) y en Windows el daemon se veía **siempre muerto**, y además
`test_pid_manager_lifecycle` (paso de tests del CI, después de clippy) habría fallado en las dos
plataformas. Ahora se le pregunta a cada plataforma como sabe responder — igual que el Go
`service.IsProcessAlive`:

* Linux: `/proc/<pid>` (sin cambios, comportamiento idéntico al de antes).
* macOS y demás Unices: `kill(pid, 0)` (`nix`, sin fork).
* Windows: `OpenProcess`/`CloseHandle` de `kernel32` (`os.FindProcess` en Go). Sin dependencias
  nuevas ni `tasklist`/`wmic` (AGENTS.md prohíbe forks en el lazo de vida del daemon).

### 5.3 Warnings que aparecieron al destaparse el árbol (todos por código Linux-only sin gatear)

| warning (`-D`) | archivo | arreglo |
|---|---|---|
| `dead_code: request_quit is never used` | `tui/run.rs` | el mecanismo de shutdown por señal es Unix-only (en Windows Ctrl-C llega como key event y el loop ya lo maneja): `QUIT_REQUESTED`, `request_quit`, `install_signal_handlers` y `quit_requested` quedan `#[cfg(unix)]`. |
| `unused import: std::process::Command` | `tui/app.rs`, `cli/main.rs` | sólo Linux (systemd) y Windows (taskkill) invocan procesos: `#[cfg(any(target_os = "linux", not(unix)))]`, con el comentario que lo explica. |
| `unused import: std::path::Path` | `cli/main.rs` | se usa sólo en el bloque `cfg(target_os = "linux")` → `#[cfg(target_os = "linux")]`. |
| `unused import: std::path::Path` | `cli/service.rs` | se usa sólo en `sync_installed_binary` (`cfg(unix)`) → `#[cfg(unix)]`. |
| `unused import: super::*` | `cli/service.rs` | el único test del módulo afirma el texto del unit systemd → el módulo es `#[cfg(all(test, target_os = "linux"))]`, con comentario. |
| `unused import: std::sync::atomic::…` | `tui/run.rs` | ídem señales: `#[cfg(unix)]`. |
| `useless conversion to the same type` | `daemon.rs` (test nuevo) | `std::env::temp_dir()` ya es `PathBuf`. |
| tests que no compilan fuera de Linux | `platform/tests/go_parity.rs`, `tests/hw_agnostic.rs` | `#![cfg(target_os = "linux")]` + doc del porqué: afirman sysfs falso y controladores `linux::*`. |
| tests del daemon con sysfs falso | `daemon.rs` | `#[cfg(all(test, target_os = "linux"))]` + doc. |

**Cero `#[allow(...)]` nuevos.** Todo se resolvió con `cfg` puntuales (código de una plataforma que
en la otra no existe) y con defaults de trait.

### 5.4 Cobertura nueva fuera de Linux

`daemon.rs` gana `non_linux_tests::ladder_degrades_gracefully_without_discoverable_ranges`
(`#[cfg(all(test, not(target_os = "linux")))]`, se ejecuta en el CI de macOS/Windows): el backend
de la plataforma arranca, deshabilitado el lazo no escribe nada y habilitado reporta
`target_freq`/`target_gpu`/`target_rapl` en 0 — ni pánico ni valor inventado donde no hay rango
descubrible (la garantía de AGENTS.md). Antes esas dos plataformas tenían cero tests del daemon.

## 6. Lo que NO pude verificar acá (honestidad)

* No hay linker/SDK de macOS ni MinGW en esta máquina. `cargo clippy` no linkea, así que lo
  verificado es **compilación** de los 4 targets cruzados; el link final (y la ejecución de los
  tests de macOS/Windows) sólo lo puede probar el CI.
* En particular el FFI de Windows (`OpenProcess`/`CloseHandle`, `#[link(name = "kernel32")]`) está
  verificado en compilación; el link contra `kernel32` lo hace el CI de Windows. Si esa ruta no
  linkeara, el paso de tests de Windows lo mostraría (no hay forma de comprobarlo desde Linux).
* El test nuevo `non_linux_tests` se ejecuta recién en el CI de macOS/Windows (acá sólo se compila).
* Observación fuera de alcance, **no la toqué**: `PidManager::new()` usa `/var/run` si existe y si
  no `/tmp/wattwarden.pid`, que en Windows resuelve a `C:\tmp\...`; el Go (`GetPIDPath`) usa
  `ProgramData`. No afecta al CI (los tests pasan ruta explícita por `with_path`), pero es una
  diferencia de paridad pendiente para la ronda de "macOS/Windows funcionales".

## 7. Workflow

No modifiqué `.github/workflows/ci.yml`: no hizo falta. El workflow ya corría clippy y los tests en
la matriz de 3 sistemas; lo que estaba roto era el código. El único paso que sigue siendo
Linux-only es `cargo fmt --check` (a propósito).

## 8. Comandos para reproducir

```bash
cd /home/juan/ww-rust
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && \
  cargo clippy --workspace --all-targets -- -D warnings

for t in x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-gnu x86_64-pc-windows-msvc; do
  cargo clippy --workspace --all-targets --target "$t" -- -D warnings
done
```
