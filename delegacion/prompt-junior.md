# Prompt Junior (Command Code) — paridad de CLI/servicio/TUI de WattWarden

Sos el **junior**. Trabajás en `/home/juan/ww-rust`, rama `feat/paridad-go-rust`, **después** de que el PM (Antigravity)
terminó el daemon. El binario Go de `master` FUNCIONA y es la especificación (el Rust es una reescritura que divergió).

## Leé primero
1. `PARITY.md` **§3 (CLI y servicio)** — strings y flags exactos. Si el contrato y el Go discrepan, gana el Go.
2. `TUI-GAP.md` — la lista de diferencias del dashboard, con archivo:línea de cada lado. Es tu checklist.
3. Referencia Go: `/home/juan/wattwarden/main.go` (121 líneas) y `ui/cli.go` (719). NO los modifiques.

## TU TRABAJO (sólo estas crates)
`crates/wattwarden-cli/**` y `crates/wattwarden-tui/**`. **NO toques** `wattwarden-platform` (lo acaba de tocar el PM) ni `wattwarden-daemon`.

1. **CLI (paridad byte a byte con `main.go`)**: `--daemon|daemon`, `--start|start`, `--stop|stop`, `--status|status`, `--brightness <on|off>` (y sin argumento = informar), `--install-service`, `--uninstall-service`, `--help|-h|help`, sin flags = TUI. Strings EXACTOS de `PARITY.md` §3, incluidos los emojis (`⚡ WattWarden background daemon started.`, `🛑 ... stopped.`, `WattWarden Daemon Status: [ACTIVE] (Running in background)` / `[INACTIVE]`, `Auto-brightness set to: true|false`, `Auto-brightness is currently: <bool>`, `✅ ... installed and started successfully.`, `✅ ... uninstalled.`).
   - **Flag desconocido NO es error de parseo**: en Go cae al chequeo de root y sale `Error: You must run this program with administrator/root privileges to change system power settings.` con **exit 1**. Con clap hoy da `error: unexpected argument` → hay que reproducir el comportamiento de Go.
   - Sin root y sin flags: mismo mensaje, exit 1.
   - `SyncInstalledBinary`: si el ejecutable actual ≠ `/usr/local/bin/wattwarden` y somos root, copiarse ahí con modo 0755.
   - Unidad systemd con el **texto exacto** de `PARITY.md` §3 (`Restart=always`, `RestartSec=3`, `KillMode=process`, `ExecStart=/usr/local/bin/wattwarden --daemon`). Mismo tratamiento para el plist de macOS y las tareas de Windows si ya existen.
   - `--start`: sincronizar binario → `auto_extreme_enabled=true` + guardar config → si existe la unidad, `systemctl restart` y verificar activo → si no, arrancar el lazo in-process → si no, spawneár daemon detached.
2. **TUI (checklist `TUI-GAP.md`)** — sólo lo marcado como "llevar a paridad":
   - Booleanos `[ACTIVE]` / `[OFF]` (hoy muestra `[true]`/`[false]`) y `Auto Extreme Mode` derivado del **estado real del daemon**, no de `config.profile`.
   - **Crítico (C1)**: Go llama `StopDaemon()` antes de **cada** escritura manual de hardware. Hoy el Rust no lo hace → el lazo pisa el ajuste del usuario a los 10 s. Reproducilo.
   - `VM Writeback` en **500** (no 5), velocidad del gráfico con `+`/`-` y `Ctrl-R`, toast `System Restored`, los **3 headers espaciadores** entre secciones y el **6º renglón del logo** (info baja 1 fila), ancho de menú **52**, techo del gráfico **12**, banda roja de 1 fila, `+1` del emoji en el modal.
   - Sacar los ítems inventados: **`BMS Battery Charge Ceiling`** y el **panel de C-states**. **Mantener `Auto Extreme Level`** (lo pidió Juan) y **EPP/ASPM de solo lectura** (en Go no se ciclan; hoy escriben hardware).
   - Mantené `[N/A]` visible cuando falta hardware (Go muestra todo, no oculta).
3. **Señales**: que la TUI salga limpio con Ctrl-C/SIGTERM (Go: `RunDaemon` atrapa las señales; el dashboard sale con su tecla).

## Reglas duras
- **NO sudo. NO tocar `/etc` ni `/sys` real. NO usar el hardware real** (la Pi no tiene batería).
- Para probar CLI/TUI: exit codes y strings con `assert` sobre el output capturado; nada de TUI interactiva.
- Violación de `AGENTS.md` que hay que arreglar: `main.rs` hace `LinuxBackend::new()?` y **aborta** si falla la init → tiene que degradar con fallback y avisar, no morir.
- Tests de strings obligatorios (golden) para `--help`, `--status`, `--brightness` y el mensaje de root.

## Criterio de aceptación
`cargo fmt` / `cargo build --all-targets` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` **los cuatro verdes**, con los tests nuevos incluidos.

Commit convencional (`feat(cli): paridad de flags/strings con Go + fixes de la TUI`) + `git push` a `feat/paridad-go-rust`.
Reportá: commits, archivos, salida de los 4 comandos, y qué de `TUI-GAP.md` quedó pendiente. **Prohibido terminar el turno sin commits.**
