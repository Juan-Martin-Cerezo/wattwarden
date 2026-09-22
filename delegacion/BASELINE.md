# Baseline REAL antes de la ronda de delegación (2026-09-22, medido en la Pi)

Estado del árbol en el commit base de la ronda. **Esto NO está verde**: arreglarlo es la primera tarea,
antes de la paridad. No lo maquilles, no borres tests para "hacerlo pasar".

## `cargo fmt --check` → 0 (ok, ya formateado)
## `cargo build --all-targets` → 0 (compila)

## `cargo test --workspace` → 101 (5 fallas, todas en `crates/wattwarden-platform/tests/go_parity.rs`)
| Test | Línea del panic | Qué es |
|---|---|---|
| `d_rapl_discovers_by_name_and_writes_microwatts` | 219 | RAPL: descubre constraint por nombre / escribe microwatts clampeados |
| `e_gpu_bounds_and_write_order_match_go` | 254 | GPU: bounds con fallback RPn/RP0 y orden min→max |
| `g_backlight_percent_matches_go` | 317 | backlight: `(cur*100)/max` y clamp 1..100 |
| `h_peripherals_and_tweaks_match_go` | 340 | kbd/audio/autosuspend/watchdog/vm writeback |
| `backend_with_root_reaches_every_subsystem` | 433 | `LinuxBackend::with_root` + `apply_profile(Extreme)` |

Las 43 pruebas de los módulos (cpu, battery, sysfs, tweaks, etc.) **pasan**. Las que fallan son las de
integración que escribí contra la semántica de Go: **la duda a resolver es si el que está mal es mi test
o el módulo** — se decide leyendo `/home/juan/wattwarden/hal/backend_linux.go`, que es la referencia.
No las borres: arreglá el módulo, o corregí el test **citando la línea del Go** que prueba tu versión.

## `cargo clippy --workspace --all-targets -- -D warnings` → 101 (8 errores)
- `this if statement can be collapsed` (1, en la crate platform)
- `doc list item overindented` (5, doc-comments de la crate platform)
- `this let-binding has unit value` (2)

## Fuera de alcance de esta ronda
Windows/macOS (no hay hardware para probar) y la validación A/B contra el binario Go real
(`scripts/ab_parity.sh`, requiere la Vostro/G15 encendidas).
