# Prompt Junior — WattWarden obedece al USUARIO, no al estado de carga + niveles adaptativos

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).
Regla de Juan (dueño del proyecto), textual: *"Wattwarden sigue señales del usuario, no me gusta que se active solo o se
desactive solo dependiendo de la carga o no, si el usuario quiere se respeta."*
Esto **modifica a propósito** el comportamiento del Go. Donde este prompt y el Go discrepen, **gana este prompt**.

## 1) Sacar el auto-switch por carga (el bug de fondo)

Hoy, en `crates/wattwarden-daemon/src/daemon.rs`, la escalera adaptativa corre **sólo a batería** y con el cable puesto
se aplica "todo al máximo" y se abandona la lógica (port de `hal/backend_linux.go:919-932`, `if b.IsCharging()`).
Lo mismo en el lazo de brillo (`:874-880`: enchufado → brillo 100 % y `return`).

Cambios:
- La escalera adaptativa corre **siempre que el usuario haya habilitado el modo** (`auto_extreme_enabled` /
  `Auto Extreme Mode`), **enchufado o no**. El cable no decide nada.
- Si el usuario **no** lo habilitó, no se adapta nada (se respeta el perfil de config, como ahora).
- El lazo de brillo: si el usuario tiene auto-brightness ON, se adapta por ventana activa (terminal 12 / heavy UI 30 /
  resto 20) **siempre**, sin forzar 100 % por estar enchufado.
- NO borres la lectura del estado de carga: se sigue mostrando en la UI y sigue el threshold de carga al 80 %
  (`Battery charge threshold locked at 80%`), que es un ajuste aparte.
- Los periféricos de ahorro (ASPM powersave, wifi power save, kbd backlight off, audio power save, autosuspend,
  nmi watchdog off, vm writeback 6000) **quedan gatillados por batería**: son ahorro energético, no un modo.
  Dejalo así y comentalo en el código.

## 2) Niveles adaptativos (aprobados por Juan)

Misma escalera para los tres: `load/ncpu` → `discrete_power = round(x*3)/3` ∈ {0, 0.33, 0.67, 1.0}.
**Los cores siempre arrancan en 1 y escalan** (hoy `Low` clava `ncpu` y `Medium` nunca baja de `ncpu/2`: está mal).

| | High (Go, NO tocar) | Medium | Low |
|---|---|---|---|
| Cores online | 1 → ncpu/2 | 1 → ncpu | 1 → ncpu |
| Freq CPU | min → min+(max-min)*0.4 | → *0.7 | → *1.0 |
| RAPL PL1/PL2 | → *0.4 | → *0.7 | → *1.0 |
| GPU | → *0.4 | → *0.7 | → *1.0 |
| Turbo | `discrete_power >= 0.8` | `>= 0.67` | `>= 0.33` |
| EPP | `power` | `power` | `<0.5` → `balance_power`, si no `balance_performance` |

`High` tiene que quedar **byte a byte** como está hoy (ya validamos paridad exacta contra el Go en hardware real).

## 3) Tests (obligatorios, con `WATTWARDEN_SYSFS_ROOT`)

- La escalera corre con `AC/online = 1` **y** con `0` (mismo resultado para la misma carga simulada).
- Los tres niveles, con valores exactos: cores y freq en `discrete_power` = 0, 0.33, 0.67, 1.0 (12 asserts mínimo).
- **Test de regresión**: con el cable puesto, el daemon NO escribe el máximo del hardware; escribe lo que dice la escalera.
- Auto-brightness no fuerza 100 % por estar enchufado.
- Actualizá los tests existentes que asumían la rama de carga (incluido lo que toca `LogicStepResult::is_charging`), pero
  **sin borrar cobertura**: si un test viejo ya no aplica, reescribilo para el comportamiento nuevo, no lo elimines.

## Aceptación (los 4 en 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `feat(daemon): el modo sigue al usuario (sin auto-switch por carga) + niveles medium/low adaptativos`.
Prohibido sudo, prohibido tocar `/etc`, prohibido correr el A/B (es mío, en hardware real).
