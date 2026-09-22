# Prompt Junior — macOS y Windows FUNCIONALES en Rust (paridad con el Go, la spec)

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).

## Punto de partida medido

```
crates/wattwarden-platform/src/macos/mod.rs     369 líneas,  0 tests, 2 rutas Unsupported
crates/wattwarden-platform/src/windows/mod.rs   347 líneas,  0 tests, 2 rutas Unsupported
```

El Go (la especificación ejecutable) tiene `hal/backend_darwin.go` + `backend_darwin_test.go` y
`hal/backend_windows.go` + `backend_windows_test.go`. **El Rust tiene que hacer lo mismo que el Go, no menos.**

## Objetivo

Portar los backends de macOS y Windows a la paridad funcional con el Go:

1. Listar función por función el backend Go de cada plataforma (`GetNumCPUs`, `SetFreqLimit`, `SetEPP`, `SetTurbo`,
   `SetGPUFreq`, `SetRAPLPL1/PL2`, `SetCores`, brillo, batería, ASPM, power save, perfiles) y **portar cada una**.
2. Las rutas que hoy devuelven `Unsupported` deben implementarse **si el Go las implementa**. Si el Go también las deja
   sin implementar, dejá `Unsupported` y **documentalo** en `PARITY.md` (paridad = mismo comportamiento, incluso el faltante).
3. Portar los **tests del Go** (`backend_darwin_test.go`, `backend_windows_test.go`) a tests de Rust, con mocks.
   No inventes cobertura: portá la que existe y agregá la que falte para lo que toques.
4. Nada de `Command::new("sh"/"cat")` en bucles calientes (lo prohíbe `AGENTS.md`), y nada de pánicos si el hardware
   no existe: el binario tiene que arrancar en cualquier máquina de esa plataforma (capability pattern + fallback).
5. Todo rango/límite **descubierto del hardware** de esa plataforma, nunca absolutos (regla de Juan), y nunca leer
   para descubrir un nodo que el propio programa escribe (el "trinquete" que ya arreglamos en Linux).

## Verificación (no hay hardware Apple/Windows acá)

- `cargo clippy --workspace --all-targets --target x86_64-apple-darwin -- -D warnings` y lo mismo con
  `x86_64-pc-windows-gnu` (lint cruzado; clippy no necesita SDK ni linker).
- Los tests que puedas hacer correr en Linux (lógica pura, mocks, sin `cfg` de plataforma) **tienen que correr acá**.
  Lo que sólo corre en CI de macOS/Windows, decilo explícitamente.
- Dejá en `delegacion/EVIDENCIA-mac-win.md`: qué funciones portaste (tabla función Go -> función Rust), qué quedó
  `Unsupported` y por qué, y el resultado de los dos clippy cruzados.

## Honestidad obligatoria

Si algo no se puede verificar sin hardware real, **escribilo en el informe y en `PARITY.md`** como "no verificado en
hardware". No declares equivalencia que no probaste.

## Aceptación (los 4 en 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `feat(platform): backends macOS y Windows con paridad funcional al Go + tests portados`.
Prohibido sudo, prohibido tocar `/etc`, prohibido correr el A/B.
