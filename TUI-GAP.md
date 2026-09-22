# TUI-GAP.md — dashboard Go (`ui/cli.go`) vs dashboard Rust (`crates/wattwarden-tui/`)

Generado por un análisis de solo lectura contra el código real (no de memoria). Fuente de verdad:
`/home/juan/wattwarden/ui/cli.go` (719 líneas) y `main.go`. Contrato general: `PARITY.md` §3/§4.
Regla: si este doc y el código Go discrepan, gana el código Go.

> Nota del operador: el análisis quedó truncado al final por límite de iteraciones del agente que lo
> hizo; las secciones A–F y la lista de decisiones pendientes están completas. Nada del repo fue
> modificado por ese agente.

---

# TUI-GAP.md — Go (`ui/cli.go`) vs Rust (`crates/wattwarden-tui/`)

## A. Estructura del menú

**A1. Falta el espaciador `Header("")` entre secciones (paridad).**
- Go: `cli.go:411`, `:454`, `:487` → `{IsHeader: true, Name: ""}` (fila vacía).
- Rust: `app.rs:56-111` no tiene ningún `Header("")` → las 3 secciones quedan pegadas, 3 filas menos.
- Impacto: el menú se ve más apretado que en Go; además cambia el `len(items)` que Go usa en el layout angosto (`cli.go:203`) y el cálculo del scrollbar.

**A2. Fila de info 1 renglón más arriba (paridad).**
- Go: `cli.go:143-150` el logo tiene **6** entradas (la 6ª es `"                                                                    "`), y `infoY := artY + len(asciiArt) + 1` (`:160`) = fila 8.
- Rust: `ui.rs:10-16` `ASCII_LOGO` tiene **5** entradas; `ui.rs:79` `info_y = art_y + ASCII_LOGO.len() + 1` = fila 7.
- Impacto: todo el layout baja/subo 1 fila vs Go; el logo pierde el renglón de aire inferior.

**A3. Ancho de menú: 50 → 52 (paridad).**
- Go: `cli.go:191` `menuW = 50` (modo horizontal). Rust: `ui.rs:94` `mw = 52`.
- Impacto: la columna de valores arranca 2 columnas más a la derecha; con `maxNameLen = menuW-20` los nombres se truncan 2 caracteres más tarde.

**A4. Alto del gráfico (paridad).**
- Go horizontal: `graphH = h - graphY - 4` (`cli.go:196`). Narrow: cap `if graphH > 15 { graphH = 15 }` (`:207`).
- Rust: `ui.rs:98` `gh = h.saturating_sub(gy + 9).max(6)`; narrow `ui.rs:107` `.min(12)`.
- Impacto: el gráfico es más bajo en ambos modos (12 vs 15 máximo) → cambia la escala visual de W por fila.

**A5. ítems con gate por hardware: en Go **no existe** (decisión pendiente de Juan).**
- Go: `buildMenuItems` (`:350-585`) arma la lista completa si `osName == "Linux"` (`:414`); **todos** los ítems se muestran siempre, incluso sin GPU/RAPL/backlight.
- Rust: `app.rs:63-65` AutoBrightness sólo si `backlight.is_some()`; `:73-75` GpuFreq si `gpu.is_some()`; `:76-79` RAPL PL1/PL2 si `rapl.is_some()`; `:80-82` Turbo si `turbo_enabled().is_ok()`; `:83-85` EPP si `energy_performance_preference().is_ok()`; `:86-88` ASPM si `aspm.is_some()`; `:93-95` Brightness si `backlight.is_some()`; `:99-101` ChargeLimit si `supports_threshold()`.
- Impacto: en una máquina sin GPU/RAPL el usuario que usó Go ve **filas que desaparecieron**, y además queda un header `─── [ HARDWARE LIMITS ] ───` sin nada abajo si todos los gates fallan.
- Veredicto: **es lo que manda `AGENTS.md`** ("UI must inspect capabilities before displaying controls") → conservar el gate, pero agregar el fallback de Go: mostrar la fila con `N/A`. Recomiendo **gate + fila N/A**, no las dos cosas puras.

## B. Textos y valores EXACTOS (esto es lo que el usuario nota)

**B1. Booleanos: `[ACTIVE]/[OFF]` → `[true]/[false]` — REGRESIÓN, llevar a paridad.**
- Go: `cli.go:251-252` `if valStr == "true" { displayVal = "[ACTIVE]" }; if valStr == "false" { displayVal = "[OFF]" }`. Afecta Turbo (`:448`), Keyboard Light (`:477`), Bluetooth (`:480`), WiFi Enable (`:483`), WiFi Power Save (`:489`), Audio Power Save (`:492`), Autosuspend PCI/USB (`:495`), Watchdog Kernel (`:498`).
- Rust: `ui.rs:433` Turbo `format!("[{}]", t)` → `[true]`; idem `ui.rs:463, 467, 471, 479, 483, 487, 491`.
- Impacto: 8 filas cambian de texto. Un usuario de Go ve `[OFF]` y ahora lee `[false]`.

**B2. Color verde+negrita en ítems activos: lo agrega Rust, Go no lo tiene (decisión pendiente de Juan).**
- Go: toda fila no seleccionada usa `defStyle` sin color (`cli.go:265`).
- Rust: `ui.rs:594-603`, si `is_active` → `Color::Green` + `BOLD`.
- Trade-off: ayuda a leer estado de un vistazo, pero rompe la fidelidad visual y hay que recalcular `is_active` por ítem (hoy `false` para Cores/Freq/RAPL/etc.).

**B3. `Auto Extreme Mode`: estado derivado de cosas distintas — llevar a paridad.**
- Go: `cli.go:360-362` `if b.IsDaemonRunning() || service.IsDaemonActive() { return "ACTIVE" }` → **estado real del daemon** (PID + `systemctl is-active`, `service.go:90-110`).
- Rust: `ui.rs:344-352` `if app.config.profile == PowerProfile::AutoExtreme`.
- Impacto: si el daemon corre por systemd y el config dice `Normal`, Go muestra `[ACTIVE]` y Rust `[EXECUTE]`. Divergencia de verdad.

**B4. `Performance Mode` / `Extreme Mode`: Go siempre `[EXECUTE]`, Rust alterna a `[ACTIVE]` (decisión pendiente de Juan).**
- Go: `cli.go:356, 358` GetVal constante `"EXECUTE"` → `[EXECUTE]` siempre.
- Rust: `ui.rs:326-343` según `config.profile`.
- Trade-off: el `[ACTIVE]` es informativo pero **inventa un estado que Go no persiste**; si el perfil se aplicó por CLI sin actualizar config queda mintiendo.

**B5. `EEP` y `ASPM`: Go son sólo lectura, Rust los cicla y ESCRIBE HARDWARE (decisión pendiente de Juan — toca el lazo adaptativo).**
- Go: `cli.go:451` Energy Perf Pref y `:452` PCIe ASPM Policy tienen **sólo `GetVal`, sin `Inc`/`Dec`/`Action`** → Enter/←/→ no hacen nada.
- Rust: `app.rs:254-265` Enter cicla `performance → balance_performance → balance_power → power` y llama `set_energy_performance_preference`; `app.rs:266-278` cicla `powersave → performance → default` y llama `set_aspm_policy`. Toasts nuevos: `"ENERGY PERF PREF: {}"` (`app.rs:263`) y `"PCIE ASPM POLICY: {}"` (`app.rs:275`).
- Impacto: son acciones **que escriben hardware** y hoy no respetan ningún lazo (Rust no frena el daemon al escribir — ver C1). Si el daemon está activo, el usuario pelea con el loop.

**B6. `VM Writeback (s)`: unidad mostrada distinta — llevar a paridad o decidir.**
- Go: `cli.go:501` `fmt.Sprintf("%d", b.GetVMWriteback())` y `GetVMWriteback` (`backend_linux.go:622-625`) devuelve **centisegundos crudos** (`dirty_writeback_centisecs`) → la pantalla muestra **500** con label `"VM Writeback (s)"`. Inc/Dec ±**1 centisegundo** (`:502-503`).
- Rust: `tweaks.rs:122-125` `vm_writeback_seconds() = cs/100` → muestra **5**; `ui.rs:495` label `"VM Writeback (s)"`. Inc/Dec ±**1 s** = ±100 centisegundos (`app.rs:431-436, 529-534`).
- Impacto: mismo hardware, número distinto (500 vs 5) y sensibilidad de la tecla 100× distinta. Go es incoherente con su propio label; **marcado como decisión de Juan**: paridad exacta con Go = mostrar centisecs y ±1, o mantener los segundos (más coherente) y aceptar la divergencia.

**B7. `LCD Brightness (%)`: formato del valor y toast.**
- Go (`cli.go:456`) muestra `[100]` (sólo el número, `%d`) y al ajustar **no** muestra toast de brillo, sólo `"AUTO BRIGHTNESS: OFF"` (`:463, 473`).
- Rust: `ui.rs:459` `format!("[{}%]", b)` → `[100%]`; y agrega toast `"LCD BRIGHTNESS: {}%"` (`app.rs:420, 518`).
- Impacto: texto del valor cambia (`[100]`→`[100%]`); toast extra es mejora cosmética aceptable.

**B8. `Process Purge` — igual.** Go `cli.go:504-505` `"EXECUTE"` + `Application: b.ProcessPurge(); showToast("PROCESSES PURGED")`. Rust `ui.rs:497` `[EXECUTE]` + `app.rs:336-339` mismo toast. ✅ paridad.

**B9. Toast de `R` (hotkey) distinto — llevar a paridad.**
- Go: hotkey `r/R/Ctrl-R` (`cli.go:646-650`) → `showToast("System Restored")`; el ítem de menú Restore (`:410`) → `"RESTORE MODE ACTIVATED"`. **Son dos mensajes distintos.**
- Rust: `run.rs:61-63` llama `restore_defaults()` (`app.rs:189-196`) que siempre dice `"RESTORE MODE ACTIVATED"` → se perdió `"System Restored"`.

**B10. Header de resumen.**
- Go: `cli.go:171-172` `"OS: %s | Battery: %d%% (%s) | Est: %s | Power: %.1fW"` con `GetOS()` y `GetBatteryTime()`; si no hay batería muestra lo que devuelva el backend (`0%`).
- Rust: `ui.rs:74-77` idéntico en formato, pero `ui.rs:58-59` agrega rama nueva: `"OS: Linux | Power: AC Mains (Stationary Workstation) | Battery: None"`, y `ui.rs:75` **hardcodea `"OS: Linux"`** en vez de consultar el backend.
- Impacto: la rama stationary es la degradación elegante que pide `AGENTS.md` → **conservar**. El `"OS: Linux"` hardcodeado es paridad OK en Linux.

**B11. Modal Extreme: título 1 columna corrido (paridad).**
- Go: `cli.go:338-339` `textX := boxX + (boxW - len([]rune(l.text)))/2` y **`if l.y == 1 { textX += 1 }`** (ajuste por el emoji `⚠️`).
- Rust: `ui.rs:725-727` no tiene ese `+1`.
- Textos del modal: idénticos (`cli.go:331-334` vs `ui.rs:711-722`), incluido `"⚠️  WARNING: EXTREME MODE"`, `"This will minimize all hardware performance."`, `"Press 'R' at any time to restore normal operation."`, `"[ Y - Confirm ]    [ N - Cancel ]"`, box 62×9.

## C. Acciones que escriben hardware y el lazo adaptativo

**C1. Go FRENA el daemon antes de cada escritura manual; Rust NO (llevar a paridad, crítico).**
- Go: **todos** los `Inc`/`Dec` de límites llaman `b.StopDaemon()` primero: Cores (`cli.go:418-419`), CPU Freq (`:425-426`), Freq iGPU (`:432-433`), RAPL PL1 (`:439-440`), RAPL PL2 (`:446-447`), Turbo (`:449-450`), Keyboard Light (`:478-479`), Bluetooth (`:481-482`), WiFi Enable (`:484-485`), WiFi Power Save (`:490-491`), Audio Power Save (`:493-494`), Autosuspend (`:496-497`), Watchdog (`:499-500`), VM Writeback (`:502-503`).
- Rust: `app.rs:344-538` (`handle_left`/`handle_right`) no llama a nada equivalente — no hay `stop_daemon` en `App`.
- Impacto: con el daemon activo, el loop de 5 s (Go) pisa el ajuste manual del usuario al siguiente tick. En Go el usuario gana; en Rust no.

**C2. Items que NO escriben hardware en Go y sí en Rust** (ya cubierto en B5: EPP, ASPM). Además `ChargeLimit` (Rust `app.rs:424-430, 522-527`) no existe en Go y escribe el umbral de carga.

**C3. `Inc`/`Dec` de Cores.** Go `SetCores(GetCores()±1)` clampeado dentro del HAL 1..NumCPUs (`PARITY.md` §2); Rust clampea en UI (`app.rs:352` `.saturating_sub(1).max(1)`, `app.rs:450` `(+1).min(num_cpus)`). Impacto: equivalente. OK.

**C4. `FreqLimit`/`GpuFreq`/`RAPL` ±pasos coinciden.** Go ±100 MHz / ±50 MHz / ±2 W; Rust igual (`app.rs:364-366, 373-375, 383-385, 461-464, 471-473, 481-483`) pero **con clamp en UI** (Go delega el clamp al HAL). Sin impacto perceptible.

**C5. Perfiles: los writes de Extreme/Performance/Restore difieren a nivel HAL** (fuera del alcance del TUI, pero el TUI los dispara).
- Go `ApplyModeExtreme` (`backend_linux.go:658-677`): cores 2, freq min, RAPL min, turbo off, EPP `power`, GPU min, ASPM `powersave`, wifi ps on, kbd off, audio ps on, **LCD 10**, autosuspend on, watchdog off, VMWriteback 6000. **No purga procesos.**
- Rust `linux/mod.rs:115-145`: igual + **`process_purge()`** (`:144`) que Go NO hace en Extreme.
- Go `ApplyModeRestore` (`:680-698`): EPP `"default"`, ASPM `"default"`, VMWriteback 500, LCD 100.
- Rust `PowerProfile::Normal` (`linux/mod.rs:146-175`): EPP `"balance_performance"`, **RAPL PL1=45 / PL2=65 hardcodeados** (`:156-157`), VMWriteback 5 s. Divergencia real → reportar al agente que porte el servicio.

## D. Teclas y señales

**D1. Teclas: Rust agrega `j/k/h/l`, Go usa `w/s/a/d` (decisión pendiente de Juan).**
- Go: Up o `w/W` (`cli.go:651`), Down o `s/S` (`:662`), Right o `d/D` (`:670`), Left o `a/A` (`:675`), `r/R`/Ctrl-R (`:646`), `q/Q`/Esc (`:643`), Enter (`:680`).
- Rust: `run.rs:64-75` agrega `k`(up), `j`(down), `h`(left), `l`(right) además de `w/s/a/d`. Falta **Ctrl-R** (Go lo tiene).
- Impacto: no rompe nada; es azúcar vim. Conservar, avisando.

**D2. Faltan `+`/`-` (velocidad del gráfico) — REGRESIÓN.**
- Go: `cli.go:626-640`, `+` baja `refreshDelay` en 500 ms (mín. 500 ms) con toast `"Update Speed: %v"`; `-` lo sube hasta 10 s. Default 2 s (`:593`), ticker que postea `EventInterrupt` (`:596-605`).
- Rust: `run.rs:37` poll fijo de 250 ms y `app.tick()` en cada iteración (`run.rs:32`) — no hay control de velocidad.
- Impacto doble: (a) no se puede ajustar la velocidad; (b) el historial de 400 muestras se llena en ~100 s en Rust vs ~800 s en Go → **el eje temporal del gráfico es ~8× más comprimido**.

**D3. Salida y Ctrl-C.**
- Go: `q/Q/Esc` → `close(d.quit); return` (`cli.go:643-645`) y `s.Fini()` restaura el terminal (`:718`). En modo raw tcell, **Ctrl-C no está manejado** (no hay case para `KeyCtrlC`) → no sale.
- Rust: `run.rs:58-60` `q/Q/Esc` → `should_quit`; `run.rs:23-25` restaura raw mode + alternate screen. **Tampoco maneja Ctrl-C** (está en raw mode, es un `KeyCode::Char('c')` que cae en `_ => {}`, `run.rs:79`). Paridad de facto (ambos ignoran Ctrl-C).
- Impacto: **SIGTERM/SIGHUP matando la TUI deja el terminal sucio en los dos** (ni Go ni Rust instalan handler en el path de TUI; Go los instala sólo en `RunDaemon`, `service.go:124-126`).
- **Divergencia en el daemon:** Go atrapa `SIGINT, SIGTERM, SIGHUP` (`service.go:125`) y llama `b.StopDaemon()` (`:128`). Rust sólo `tokio::signal::ctrl_c()` (`daemon.rs:94`) → **SIGTERM/SIGHUP no están atrapados** (systemd usa SIGTERM: el `KillMode=process` + `SIGTERM` dejaría el PID file y el hardware sin restaurar).

**D4. Privilegios al arrancar la TUI.**
- Go: `main.go:108-111` sin root → `"Error: You must run this program with administrator/root privileges to change system power settings."` y `os.Exit(1)`.
- Rust: `main.rs:147-149` sin root → `eprintln!("Warning: Running without root privileges. Some hardware controls will be read-only.")` y **sigue al TUI**.
- Veredicto: mejora deliberada y alineada con `AGENTS.md` → **conservar**, pero es un cambio de contrato de §3 que Juan debe bendecir (un usuario de Go espera que se niegue a arrancar).

**D5. `main.rs:150` `LinuxBackend::new()?` aborta la TUI si falla la init del backend** → contradice `AGENTS.md` §"NO Panicking … The application must boot flawlessly on a desktop PC". Go en ese caso imprime `"Error: No backend implementation available for this OS."` (`main.go:13`) y sale limpio. Corregir.

## E. Extras del Rust que Go no tiene (marcar como decisión)

**E1. Panel `draw_cstates` (`ui.rs:274-321`).** Header `"─── [ CPU C-STATE RESIDENCY ] {…─}"` y `"No cpuidle C-state telemetry detected"`; columnas `"{:<4}: {:>7}ms"` (`:306`). Sólo en modo horizontal (`ui.rs:121-123`). Go no tiene nada equivalente → decisión de Juan (ocupa filas que en Go eran del gráfico). Ojo: `:305` `state.time_microseconds / 1000` muestra ms acumulados crudos.

**E2. Ítem `Auto Extreme Level` (`ui.rs:353-360`, `app.rs:227-235`).** Label `"Auto Extreme Level"`, valores `[LOW]/[MEDIUM]/[HIGH]`, Enter cicla (`config.rs:133-139`) y persiste, toast `"AUTO EXTREME LEVEL: {LEVEL}"`. Es §4 de PARITY.md → **conservar** (extensión explícita de Juan), pero el nivel default `High` debe ser equivalente literal a Go.

**E3. Ítem `BMS Battery Charge Ceiling` (`ui.rs:473-476`, `app.rs:424-430, 522-527`).** Label y valor `[80%]` no existen en Go (Go sólo tiene `rfkill` de BT/WiFi, nada de charge threshold). Decisión de Juan; si el HAL lo clampea 50..100 en UI, ya es divergencia del HAL.

**E4. Umbrales de color del gráfico (paridad menor).**
- Go: `cli.go:99-100` `if yOffset > height/2 {Yellow}; if yOffset > height-2 {Red}` → la banda roja es de **1** fila (sólo `yOffset == height-1`).
- Rust: `ui.rs:224-229` `>= height-2` → banda roja de **2** filas.
- Go `drawBarGraph` sólo corta si `len(dataList)==0` (`:62`); Rust agrega guard `width < 20 || height < 3` (`ui.rs:172`). Impacto cosmético.

**E5. `main.rs:150` carga config y la pasa al `App`, pero a diferencia de Go (`cli.go:697-698` `SetAutoBrightness(cfg.AutoBrightness)`) no sincroniza el flag en memoria del backend.** Impacto: en Rust el valor mostrado (`config.auto_brightness`) puede no coincidir con lo que el daemon lee del archivo.

## F. Lo que SÍ está en paridad (no tocar)

- Logo ASCII idéntico (`cli.go:144-148` vs `ui.rs:11-15`), estilo título Aqua/Cyan+Bold (`cli.go:140` / `ui.rs:49-50`).
- `PLEASE RESIZE TERMINAL (Minimum size: 70x20)` + gate `w < 70 || h < 20` (`cli.go:132-134` / `ui.rs:34-35`).
- Bloques `[' ',' ','▂','▃','▄','▅','▆','▇','█']` y `graphMax = 15.0` (`cli.go:58,64` / `ui.rs:18,118`).
- Eje inferior `└` + `─`×(width-14), etiquetas `"%4.1 W"` con cortes 6 W/12 W (`cli.go:71,116-123` / `ui.rs:190,249-268`).
- Footer literal `"[UP/DOWN] Navigate | [L/R] Adjust | [ENTER] Apply | [R] Restore | [Q] Quit"` (`cli.go:345` / `ui.rs:153`).
- Indicadores `" ▲ SCROLL UP FOR MORE OPTIONS ▲ "` / `" ▼ SCROLL DOWN FOR MORE OPTIONS ▼ "`, fórmula del scrollbar, toast amarillo 3 s en `h-3`, `historial` sólo con `watts > 0` y cap 400, truncado de nombre a `maxNameLen-3 + "..."` (bytes en Go, chars en Rust → puede diferir en 1 carácter con el emoji `⚡`), formatos de fila `" > {:<w} {:>15} "` / `"   {:<w} {:>15} "`.
- Toasts `"PERFORMANCE MODE ACTIVATED"`, `"EXTREME MODE ACTIVATED"`, `"AUTO EXTREME DAEMON STOPPED"`, `"AUTO EXTREME RUNNING (BACKGROUND)"`, `"AUTO BRIGHTNESS: ON/OFF"`, `"PROCESSES PURGED"`, `"RESTORE MODE ACTIVATED"` — todos presentes textualmente en Rust.

---

### Resumen para el padre

- **No se creó `/home/juan/ww-rust/TUI-GAP.md`**: agoté el presupuesto de iteraciones durante la lectura. El contenido íntegro está arriba, listo para copiar. Ningún archivo del repo fue modificado (solo lectura respetada).
- **Regresiones claras (llevar a paridad):** booleanos `[ACTIVE]/[OFF]` → `[true]/[false]` en 8 filas; `Auto Extreme Mode` usa `config.profile` en vez del estado real del daemon; faltan `+`/`-` de velocidad y Ctrl-R; se perdió el toast `"System Restored"`; `VM Writeback` muestra 5 en vez de 500; faltan los 3 headers espaciadores y el 6º renglón del logo (info 1 fila más arriba); ancho de menú 52 vs 50; techo del gráfico 12 vs 15; banda roja de 2 filas vs 1; modal sin el `+1` del emoji.
- **Crítico de comportamiento:** Go llama `StopDaemon()` antes de **cada** escritura manual de hardware (`cli.go:418-503`); Rust nunca lo hace → el lazo pisa los ajustes del usuario.
- **Decisiones pendientes de Juan:** gate de ítems por capacidad (Rust, alineado a `AGENTS.md`) vs mostrar siempre con `N/A` (Go); color verde de activos; EPP/ASPM ciclables (escriben hardware, no existen en Go); panel C-states; ítems `Auto Extreme Level` y `BMS Battery Charge Ceiling`; TUI sin root (warning vs exit 1); `LinuxBackend::new()?` abortando la TUI (viola `AGENTS.md`); daemon Rust sin handler de SIGTERM/SIGHUP (Go los atrapa en `service.go:125`).