# Prompt Junior — Rangos de frecuencia SÓLO desde fuentes inmutables (fix del trinquete)

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).

## El bug, medido en hardware real (Dell Vostro, Intel i5-8250U, 3 cores)

```
cpuinfo_min_freq = 400000        cpuinfo_max_freq = 1600000      (información INMUTABLE del chip)
scaling_available_frequencies = (vacío: intel_pstate no lo expone)
scaling_max_freq = 2400000       (estado MUTABLE: lo escribe el propio daemon)
```

El daemon escribió **2400000 = 2.4 GHz**, arriba del máximo que declara el hardware (1.6 GHz).

**Causa probable: trinquete.** El rango se descubre leyendo `scaling_max_freq` (mutable). El camino de
"restore"/perfil escribe un centinela tipo `99999`, el kernel lo clampa al turbo del chip, y en la lectura
siguiente ese valor clampado se toma como "el máximo del hardware" → el rango se auto-infla y nunca vuelve.

## Cambios

1. **Descubrimiento de frecuencia sólo desde `cpuinfo_min_freq` / `cpuinfo_max_freq`** (por CPU, agregando:
   min = el menor de los min, max = el mayor de los max). **Prohibido** usar `scaling_min_freq` /
   `scaling_max_freq` / `scaling_available_frequencies` como fuente de descubrimiento: son estado mutable.
   Dejalos sólo para **escribir**.
2. **Clamp explícito** en toda escritura de frecuencia a `[min_descubierto, max_descubierto]`. Si el valor
   calculado excede, se escribe el máximo descubierto y se registra el motivo en el log.
3. **Nada de centinelas** (`99999`) en los caminos de máximo: escribí el **máximo descubierto**.
4. Si `cpuinfo_*` no existe, es 0 o es degenerado (`min == max`): **no escribir frecuencia** (ni la rampa ni
   el perfil) y loguear por qué.
5. Revisá lo mismo en cualquier otro rango que hoy se lea de un archivo escribible (RAPL ya usa
   `constraint_N_max_power_uw`; GPU revisá que no lea un nodo que él mismo escribe).

## Tests obligatorios

- **Test del trinquete**: sysfs falso con `scaling_max_freq = 2400000` (inflado) y `cpuinfo_max_freq = 1600000`.
  Afirmar que el daemon **nunca** escribe frecuencia > 1600000, y que con el techo al 100 % el valor es
  **exactamente 1600000**, no 2400000. Este test es el corazón del arreglo.
- **Degenerado**: `cpuinfo_min_freq == cpuinfo_max_freq` → no se escribe nada.
- **Clamp**: pedir un valor enorme (99999) → se escribe el máximo descubierto, no el pedido.
- **Dos máquinas**: mantener los dos sysfs falsos que replican las Dell (3 cores/15 W/1.6 GHz y
  20 cores/45 W/4.6 GHz) y que sigan afirmando escalado por hardware y respeto de techos.
- No borres cobertura existente; adaptá lo que asuma el descubrimiento viejo.

## Aceptación (los 4 en 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `fix(hw): rango de frecuencia descubierto sólo de cpuinfo_* (inmutable) + clamp al máximo real (fix del trinquete)`.
Prohibido sudo, prohibido tocar `/etc`, prohibido correr el A/B (lo corro yo en las dos Dell).
