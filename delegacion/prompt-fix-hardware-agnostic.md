# Prompt Junior — TODO valor sale del hardware descubierto (cero absolutos, misma calidad en cualquier máquina)

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).

Regla de Juan: *"hacé que cualquier ajuste se adapte a cada hardware de computadora."*
Esto **se aparta a propósito del Go** en los absolutos hardcodeados (el Go asume la laptop de Juan). Gana este prompt.

## Evidencia medida en dos máquinas reales (Dell G15 y Dell Vostro, x86_64)

| Dato | G15 | Vostro |
|---|---|---|
| cores (`nproc`) | 20 | **3** |
| freq min..max | 400000..4600000 | 400000..1600000 |
| RAPL PL1 (`constraint_0`) `max_power_uw` | **45000000** | **15000000** |
| RAPL PL1 que **hoy escribe WattWarden** | **115000000** ❌ | **34000000** ❌ |
| `constraint_1_max_power_uw` | 0 (no expuesto) | 0 (no expuesto) |
| backlight `max_brightness` | 96000 | 7500 |
| GPU | `card1/gt_max_freq_mhz` | `card1/gt_max_freq_mhz` |
| governors | performance powersave | performance powersave |
| EPP disponibles | default performance balance_performance balance_power power | igual |

## Los tres bugs concretos a arreglar

1. **RAPL fuera de rango.** Se escribe 115 W / 34 W cuando el máximo declarado es 45 W / 15 W.
   Portá `constraint_N_max_power_uw` como **techo real por constraint** (PL1 y PL2 por separado, no el mismo número
   para los dos): el valor de cada constraint se calcula escalando dentro de *su propio* rango y **nunca lo supera**.
   - Si `constraint_N_max_power_uw` es **0 o no existe**, no inventes (115 W es un invento): **no escribas ese
     constraint** y logueá que el hardware no expone rango. Nada de fallbacks absolutos para RAPL.
2. **Cores estáticos en máquinas chicas.** Hoy `High` usa `max_cores = (ncpu/2).max(1)`: en la Vostro (3 cores) eso da
   **1**, o sea cero adaptación. Reformulá la rampa de cores para que **siempre tenga escalones** en cualquier tamaño:
   el techo de cores por nivel debe derivarse de `ncpu` con proporción y **mínimo 2** cuando `ncpu >= 2`
   (ej. dividir la rampa en 4 escalones y cortar por proporción del nivel), garantizando al menos 2 valores distintos
   entre `discrete_power`=0 y su máximo. Dejá escrito en el comentario la fórmula y por qué.
3. **Fallbacks absolutos.** `400/1600`, `400/4500`, `300/1100`, `5/115` W: sólo pueden usarse si el descubrimiento
   falla **por completo**, y tienen que estar marcados como tal. Regla: **si no se puede descubrir el rango, no se
   escribe** (y se registra el motivo). El descubrimiento manda siempre.

## Además (repaso de todo lo que se toca)

- Frecuencia: min/max por CPU desde `cpuinfo_min_freq`/`cpuinfo_max_freq` (ya está) — verificá que ninguna escritura
  pueda quedar fuera de rango.
- GPU: descubrir `gt_RPn`/`gt_RP0` o el par min/max disponible; si el rango descubierto es degenerado (min == max),
  no escribir.
- Turbo: sólo si existe `intel_pstate/no_turbo` **o** `cpufreq/boost` (no asumir que existe).
- Governor/EPP/ASPM: elegir **de las listas disponibles** del hardware (`scaling_available_governors`,
  `energy_performance_available_preferences`, `pcie_aspm/parameters/policy`); si el valor pedido no está disponible,
  usar el más cercano disponible o no escribir.
- Backlight: porcentajes sobre `max_brightness` descubierto (nunca valores absolutos como 100/500).
- Charge threshold: respetar el rango soportado por la batería; no escribir si el nodo no existe.
- `vm.writeback`, `nmi_watchdog`, autosuspend, wifi/audio power save: los límites son del kernel (100..6000 cs, etc.)
  pero si el nodo no existe, no escribir.

## Tests (obligatorios)

- **Dos sysfs falsos que repliquen las dos máquinas de la tabla** (3 cores / 15 W / 1.6 GHz y 20 cores / 45 W / 4.6 GHz).
- Afirmar que con el mismo `discrete_power` **los valores escritos escalan según cada máquina** y que
  **nunca se supera el máximo descubierto** (test explícito para RAPL: no puede quedar arriba de `max_power_uw`).
- Afirmar que la rampa de cores tiene **al menos 2 escalones distintos** con `ncpu = 3`.
- Afirmar que si el rango no se puede descubrir (nodos ausentes/0), **no se escribe** ese nodo.
- No borres cobertura existente: adaptá los tests que asumían 115 W.

## Aceptación (los 4 en 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `feat(hw): todos los valores derivados del hardware descubierto (cero absolutos, sin escribir fuera de rango)`.
Prohibido sudo, prohibido tocar `/etc`, prohibido correr el A/B (lo corro yo en las dos Dell).
