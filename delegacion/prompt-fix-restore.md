# Prompt Junior — cerrar la divergencia de la rama CARGA/RESTORE (evidencia A/B real)

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).
Referencia ejecutable: `/home/juan/wattwarden/hal/backend_linux.go` → **si el .go y el Rust discrepan, gana el .go.**

## El bug, con evidencia

Corrí el arnés A/B (`scripts/ab_parity.sh`) en hardware real (Dell Vostro, x86_64, **enchufada y cargando**),
30 s cada binario, 30 nodos sysfs muestreados. Resultado: **27 OK, 3 DIF**, y las 3 son de la rama de CARGA:

```
/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw    go=115000000   rust=45000000     DIF
/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw    go=115000000   rust=65000000     DIF
/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor           go=performance  rust=powersave    DIF
```

Es decir: con la batería cargando, el Go **restaura** el equipo (RAPL al máximo de rango = 115 W en PL1 y PL2,
governor `performance`), y el Rust deja el perfil `Normal` (45/65 W, `powersave`). Los otros 27 nodos
(freq min/max, cores online, no_turbo, EPP, ASPM, power_save de audio y wifi, nmi_watchdog, writeback,
threshold de carga al 80 %) ya coinciden: **no los rompas.**

## Tu tarea

1. Leé en el Go exactamente qué hace en el camino de **carga/restore** (`ApplyModeRestore` y quien lo llama desde
   `service/service.go`: fijate la condición `IsCharging()` y qué pasa cada tick) y **portalo verbatim**:
   qué escribe en `constraint_0_power_limit_uw` y `constraint_1_power_limit_uw` (¿el máximo del rango leído, o
   `max_power_range_uw`?), qué governor pone, y **en qué orden** respecto de los otros nodos.
2. Revisá si el Rust hace ese camino por `PowerProfile::Normal` (perfil de config) cuando debería hacerlo por el
   camino de restore del daemon. **El perfil de config no debe ganarle al restore**: es el bug de fondo.
3. Escribí tests con `WATTWARDEN_SYSFS_ROOT` (sysfs falso) que afirmen los valores exactos de la tabla de arriba,
   citando en el comentario la línea del `.go`. Un test que falle si alguien vuelve a meter 45/65 en el camino de carga.
4. **No** corras el A/B contra hardware (necesita sudo de Juan y es mi trabajo, no el tuyo). Verificá con los tests.

## Aceptación (corré los 4)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Los cuatro tienen que dar exit 0. Commit con mensaje `fix(daemon): rama de carga/restore identica a Go (RAPL al maximo + governor performance)`.
Prohibido borrar o relajar tests para hacerlos pasar, y prohibido tocar `Cargo.lock` de más.
