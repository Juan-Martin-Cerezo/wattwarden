# Prompt Junior — WattWarden NUNCA toca nada sin un ajuste explícito del usuario (opt-in puro)

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`**.

Regla de Juan, textual: *"wattwarden solo gestiona, si no tenés activado ningún profile o config no tiene por qué tocar
nada automáticamente, a eso me refería con que obedece al usuario."*

Esto **se aparta a propósito del Go** (el Go prende todo por default). Donde este prompt y el Go discrepen, gana este prompt.

## El problema, verificado

Los defaults de `Config` (`crates/wattwarden-core/src/config.rs:143-166`) son **todo encendido**:
`auto_extreme_enabled = true`, `auto_brightness = true`, `profile = Normal`, `battery_charge_limit = 80`.
Resultado: una instalación nueva, sin que el usuario toque nada, escribe cores, freq, RAPL, GPU, EPP, turbo, brillo
y el límite de carga. Y el daemon además sigue aplicando el perfil aunque el usuario nunca lo eligió.

## Lo que hay que hacer

1. **Defaults = no tocar nada** (opt-in puro):
   - `auto_extreme_enabled` → default **false**.
   - `auto_brightness` → default **false**.
   - `profile` → pasa a `Option<PowerProfile>` con default **None** ("el usuario no eligió perfil" = no aplicar perfil).
     Cuidado con la compatibilidad: una config que diga `"profile": "Normal"` explícito tiene que seguir aplicándolo.
   - `battery_charge_limit` → default **None** (hoy default 80: no tocar el hardware si el usuario no lo pidió).
2. **Cada escritura, guardada por su ajuste.** El daemon sólo escribe cuando el usuario habilitó *ese* ajuste:
   - `profile: Some(p)` → aplicar perfil `p`.
   - `auto_extreme_enabled` → escalera adaptativa (con `auto_extreme_level`).
   - `auto_brightness` → lazo de brillo por ventana activa.
   - `battery_charge_limit: Some(n)` → fijar el umbral de carga a `n`.
3. **Sin ningún ajuste habilitado el daemon es de SOLO LECTURA**: no escribe *nada* (ni cores, ni freq, ni RAPL,
   ni GPU, ni EPP, ni turbo, ni ASPM, ni wifi/audio power save, ni autosuspend, ni nmi_watchdog, ni vm writeback,
   ni backlight, ni kbd backlight, ni charge threshold). Sigue leyendo estado, logueando y sirviendo la UI/CLI: eso no es tocar.
4. El estado de carga (enchufado/batería) **no habilita ni deshabilita nada por sí mismo**; se sigue usando para
   mostrar en UI y para los periféricos de ahorro cuando la escalera está activa.
5. Los ajustes de ahorro (ASPM, wifi/audio power save, kbd backlight, autosuspend, nmi watchdog, vm writeback)
   son parte del modo del usuario: se aplican **sólo** si ese modo está habilitado.

## Tests (obligatorios, con `WATTWARDEN_SYSFS_ROOT`)

- **Test estrella**: config por defecto (recién creada, sin tocar nada) → **cero escrituras**. Recorré el sysfs falso
  antes y después, y afirmá que ningún archivo de hardware cambió (y que no apareció ninguno nuevo).
- Cada ajuste por separado habilita **sólo** sus escrituras: `profile` solo; `auto_extreme_enabled` solo;
  `auto_brightness` solo; `battery_charge_limit` solo.
- `battery_charge_limit: None` → no se toca el umbral de carga.
- Compatibilidad: `"profile": "Normal"` explícito sigue aplicándose (test de round-trip de la config).
- Actualizá los tests que asumían los defaults encendidos, sin perder cobertura.

## Aceptación (los 4 en 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `feat(config): opt-in puro — sin ajuste del usuario el daemon no escribe nada`.
Prohibido sudo, prohibido tocar `/etc`, prohibido correr el A/B.
