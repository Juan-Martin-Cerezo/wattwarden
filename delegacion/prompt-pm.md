# Prompt PM (Antigravity) — paridad Go→Rust de WattWarden

Sos el **PM**. Trabajás en `/home/juan/ww-rust`, rama `feat/paridad-go-rust` (mismo clon que después usa el junior:
**secuencial, nunca en paralelo**). El binario Go de `master` FUNCIONA y es la especificación: el Rust es una
reescritura hecha de memoria que divergió y hoy no anda bien (reporte de Juan, 2026-09-22).

## Leé primero (son cortos y son el contrato)
1. `PARITY.md` — §1 daemon, §2 hardware, §4 niveles, §5 verificación. **Si el contrato y el código Go discrepan, gana el Go.**
2. `TUI-GAP.md` — diferencias del dashboard (el junior las arregla después; vos sólo las leé).
3. Referencia Go (NO la modifiques nunca): `/home/juan/wattwarden/service/service.go` (305), `hal/backend_linux.go` (1018, mirá `StartAutoExtremeDaemon` línea ~845), `main.go`, `ui/cli.go`.

## TU TRABAJO (crítico — hacelo vos, no lo delegues)
`crates/wattwarden-daemon/src/daemon.rs` + las divergencias de perfil en `crates/wattwarden-platform/src/linux/mod.rs`.

Lo que HOY está mal en el daemon Rust (por eso "ni el auto extreme normal funciona"):
1. Un solo ticker de **10 s**. Go usa **dos**: `5 s` para `applyLogic()` y **`300 ms`** para `applyBrightness()`, y ambos corren **una vez al arrancar** antes del loop.
2. Carga **continua** en vez de **cuantizada**: Go hace `discretePower = round(powerLevel * 3) / 3.0` → **0, 0.333, 0.667, 1.0**.
3. Techo **100 %** del rango. Go: `maxCPU = int(minCPU + (hwMax-minCPU) * 0.4)` → **40 %** (igual para GPU y RAPL).
4. `EPP` y `turbo` sólo si `manage_power_hints` (High=false) → en High **no escribe nada**. Go escribe **`EPP="power"` en cada tick** a batería y `turbo = discretePower >= 0.8` (o sea sólo en el escalón 1.0).
5. Faltan por completo en la rama de batería: `ASPM=powersave`, wifi power save on, kbd backlight off, audio power save on, autosuspend on, watchdog off, `VM writeback 6000`. Y en la rama enchufada: los opuestos (`ASPM=performance`, wifi off, audio off, autosuspend off, watchdog on, vm 500) + brillo 100 si auto-brightness.
6. El cambio de rama depende de un evento netlink: si no llega, nunca cambia. Go evalúa `IsCharging()` **en cada tick de 5 s**.
7. Faltan señales: Go atrapa **SIGINT, SIGTERM y SIGHUP** (`service.go:125`). Hoy sólo Ctrl-C → `systemctl stop` puede colgar el proceso.
8. Levantar la config de `/etc/wattwarden/config.json` en formato Go (`auto_extreme_enabled`, `auto_brightness`, indent 2 espacios) y PID en `/var/run/wattwarden.pid` (borrar al salir).

Niveles (extensión de Juan, no existe en Go) — `PARITY.md` §4:
- `high` (**default**) = **Go verbatim** (techo 40 %, escalones, 5 s, EPP `power`, brillos 12/20/30 fijos).
- `medium` = mismo lazo con techo `0.7`; `low` = techo `1.0` + EPP `balance_power`/`balance_performance` + turbo desde `0.5`. Ningún nivel cambia el algoritmo.
- Brillo por ventana en Go: terminal (o clase vacía) **12**, browser/IDE pesado **30**, resto **20** — re-aplicado cada 300 ms si cambió o si el valor real difiere.

Divergencias de perfil a corregir en `crates/wattwarden-platform/src/linux/mod.rs`:
- `PowerProfile::Extreme` llama `process_purge()` → **Go NO lo hace** en `ApplyModeExtreme` (drop_caches es sólo acción manual del dashboard). Sacalo.
- `PowerProfile::Normal` (= `ApplyModeRestore` de Go) debe escribir EPP **`default`** y RAPL al **máximo** del rango; hoy escribe `balance_performance` y 45/65 fijos.

## Reglas duras
- **NO sudo. NO escribir en `/sys` real. NO tocar `/etc`.** Todo se prueba con `WATTWARDEN_SYSFS_ROOT` + `LinuxBackend::with_root(root)` y un sysfs falso en `temp_dir` (patrón ya usado en `crates/wattwarden-platform/tests/go_parity.rs`: mirá ese archivo, sirve de molde).
- NO toques `/home/juan/wattwarden` (clon Go de referencia). NO `git push --force`. NO refactors de arquitectura.
- Sólo editan este turno: `crates/wattwarden-daemon/**` y `crates/wattwarden-platform/src/linux/mod.rs`.

## Criterio de aceptación (corré los 4 y pegá la salida en tu reporte)
`cargo fmt` / `cargo build --all-targets` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings`.
Tests nuevos obligatorios (con sysfs falso, afirmando **valores exactos**):
(a) a batería con load normalizado `0.0`, `0.2`, `0.5`, `1.0` → escalones `0 / 0.333 / 0.667 / 1.0` y `target_freq`, `target_cores`, `target_rapl` calculados con el techo 40 %;
(b) enchufado → el set completo de la rama AC;
(c) `turbo` sólo en el escalón 1.0 y EPP `power` en todos los escalones;
(d) `high` no reescribe turbo/EPP distintos de Go (test de preservación de comportamiento).

Al terminar: commit convencional (`feat(daemon): paridad con el lazo adaptativo de Go + niveles parametrizados`),
`git push` a `feat/paridad-go-rust`, y reportá en 10 líneas: commits, qué escribiste, salida de los 4 comandos
y qué quedó sin hacer (si algo quedó). **Prohibido terminar el turno sin commits.**
