# Prompt Junior — la TUI se rompe visualmente: los logs del daemon caen en la terminal

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).
Referencia: `/home/juan/wattwarden` (Go) → **si el .go y el Rust discrepan, gana el .go.**

## El bug, con evidencia de hardware real

Juan abrió `sudo wattwarden` en la Dell Vostro (x86_64, Linux) y la pantalla **se corrompe mientras la usa**:
las líneas del daemon se imprimen ENCIMA del panel y de la gráfica:

```
INFO wattwarden_daemon::daemon: WattWarden daemon started successfully with PID 11069
INFO wattwarden_daemon::daemon: Battery charge threshold locked at 80%
Updated device 'intel_backlight' of class 'backlight':
```

Causa ya localizada (verificala vos, no la des por buena):

- `crates/wattwarden-tui/src/app.rs:264` → `Command::new(exe).arg("--daemon").spawn()`: el daemon **hereda stdout/stderr**
  de la TUI, que en ese momento tiene la terminal en modo alternativo. Cada log del hijo pisa el frame.
- `crates/wattwarden-cli/src/main.rs:61` → mismo patrón.
- `crates/wattwarden-cli/src/main.rs:127-130` → el `tracing_subscriber` fmt layer se instala **sin writer**, o sea a stdout.
- Además hay `tracing` de nivel info dentro del camino de hardware (p. ej. el setter de backlight) que imprime por acción.

## Tu tarea

1. **Leé primero qué hace el Go** cuando lanza el daemon en segundo plano (`main.go`, `service/service.go`, `ui/cli.go`):
   ¿a dónde van su stdout/stderr? ¿un archivo de log? ¿journald? ¿syslog? ¿`/dev/null`? ¿qué permisos y qué path?
   **Portá exactamente eso**: mismo destino, mismo path, misma política de rotación si la hay.
2. En Rust, todo spawn del propio ejecutable (`--daemon`) debe redirigir sus stdio igual que el Go
   (`Stdio::from(File::create(...))`, `Stdio::null()`, etc. — lo que diga el Go) y desacoplarse de la terminal
   (`stdin` no puede quedar en la tty). Aplicalo en **todos** los sitios: `wattwarden-tui/src/app.rs` y
   `wattwarden-cli/src/main.rs` (y `service.rs` si lanza algo).
3. La TUI **nunca** debe dejar que el logging caiga en la terminal mientras corre: si el Go no imprime logs en el
   dashboard, el Rust tampoco. Revisá si la TUI instala el subscriber y a dónde; lo que sea interactivo va a /dev/null
   o al archivo, nunca a stdout.
4. Bajá de nivel (o sacá) el logging por acción de hardware que hoy imprime en cada tick/por cada escritura
   (`Updated device ...`): si el Go no lo hace, es divergencia. No inventes logs nuevos.
5. Verificación: los tests que puedas automatizar (p. ej. que el helper de spawn construye el `Command` con stdio
   redirigido, con un fake root/tempdir), y **dejá escrito en el cuerpo del commit** el comando exacto con el que yo
   valido en hardware real: abrir la TUI, dejarla 60 s, y confirmar que no aparece ninguna línea de log en pantalla.

## Aceptación (los 4 con exit 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `fix(tui): logs del daemon fuera de la terminal (stdio redirigido como en Go)`.
Prohibido borrar/relajar tests. Prohibido tocar `/etc` o pedir sudo.

## Bonus (mismo commit si es chico)

`scripts/ab_parity.sh` deja el servicio `wattwarden.service` **apagado** cuando termina (hace snapshot/restore de sysfs
pero no del servicio). Arreglalo: si el servicio estaba activo antes de la prueba, tiene que quedar activo después.
