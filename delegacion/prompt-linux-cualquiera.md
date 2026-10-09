# Prompt Junior — Linux en CUALQUIER computadora (AMD, ARM, sin batería, sin Intel RAPL)

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).

## Regla de Juan (la que manda)

*"Cualquier ajuste se adapta a cada hardware de computadora"* y el binario tiene que arrancar sin fallar en
**cualquier máquina**. `AGENTS.md` del repo lo exige: *"The application must boot flawlessly on a desktop PC, rack
server, Raspberry Pi, or container"*.

## Problema medido

Hoy se asume hardware Intel de notebook:

- RAPL: ruta fija a `intel-rapl:0` (`/sys/class/powercap/intel-rapl:0/...`). En AMD, ARM, servidores o contenedores
  ese dominio no existe o tiene otro nombre.
- `intel_pstate/no_turbo` se asume presente (en AMD y en ARM no existe; los que usan `acpi-cpufreq` tienen `cpufreq/boost`).
- EPP (`energy_performance_preference`) no existe en varios equipos.
- GPU: sólo el nodo Intel `gt_max_freq_mhz`.
- Batería: en desktop/servidor/contenedor no hay `BAT0`.

Verificado: 2 notebooks Intel x86_64. **Nunca se corrió en AMD, ARM (Raspberry Pi), desktop sin batería ni contenedor.**

## Cambios

1. **RAPL descubierto dinámicamente**: buscar el/los dominios con glob bajo `/sys/class/powercap/` (no hardcodear
   `intel-rapl:0`), soportar varios dominios y la ausencia total. Si no hay dominio: **no escribir** y loguear el motivo.
2. **Turbo**: usar `intel_pstate/no_turbo` si existe, si no `cpufreq/boost`, si no existe ninguno: no escribir.
3. **EPP**: usar el nodo sólo si existe y el valor pedido está en su lista de disponibles; si no, no escribir.
4. **GPU**: descubrir el nodo disponible (Intel `gt_max_freq_mhz`, etc.). Si no hay nodo de frecuencia de GPU: no escribir.
5. **Backlight**: puede no existir (servidor/contenedor) -> no escribir, no fallar.
6. **Sin batería**: `StationaryPowerSource` / equivalente: el daemon arranca, informa la capacidad y **no escribe nada
   relacionado con carga** (nada de pánicos, nada de `?` que aborte el arranque).
7. Ninguna de estas ausencias puede hacer fallar el arranque ni ensuciar el log a cada tick (una vez al inicio basta).

## Tests obligatorios (sysfs falso, corren en Linux sin hardware)

- **(a) ARM/Pi**: sin `/sys/class/powercap/*`, sin `intel_pstate`, sin batería, con `cpufreq` presente -> arranca,
  arma el lazo, y **no escribe** en ningún nodo inexistente.
- **(b) AMD**: powercap con nombre distinto (no `intel-rapl:*`), sin `no_turbo`, sin EPP -> no escribe esos nodos.
- **(c) Desktop sin batería**: sin `power_supply/BAT*` -> arranca y no escribe nada de carga.
- **(d) Contenedor**: sin `/sys/class/backlight`, sin powercap, sin batería -> arranca.
- Afirmá en cada caso que el daemon **no falla** y que **no escribe** fuera de lo que existe.

## Evidencia obligatoria

`delegacion/EVIDENCIA-linux-cualquiera.md`: qué se hizo dinámico, qué ausencias se manejan y cómo, y los tests nuevos
con su nombre. Además corré en la **Raspberry Pi (esta máquina, aarch64)**:

```
cargo build --release
./target/release/wattwarden --status        # (sin root no toca nada: sólo informar)
```

y pegá la salida. La Pi NO tiene RAPL Intel ni batería: es el caso real de prueba.

## Aceptación (los 4 en 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `feat(hw): descubrimiento dinamico de RAPL/GPU/turbo/EPP y soporte de equipos sin bateria (AMD, ARM, desktop, contenedor)`.
Prohibido sudo, prohibido tocar `/etc`, prohibido correr el A/B, **prohibido ejecutar el daemon como root en la Pi**.
