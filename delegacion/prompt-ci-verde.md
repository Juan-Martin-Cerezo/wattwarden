# Prompt Junior — Dejar el CI VERDE en las 3 plataformas (clippy macOS/Windows)

Sos el **junior**. Repo `/home/juan/ww-rust`, rama `feat/paridad-go-rust`. **No toques `/home/juan/wattwarden`** (sólo leer).

## Hecho, verificado con `gh run list`

El CI del repo corre una matriz `ubuntu-latest`, `macos-latest`, `windows-latest` y viene **rojo**:

```
ubuntu-latest  - Test & Build: SUCCESS
macos-latest   - Test & Build: FAILURE  -> paso que falla: "Run clippy linter"
windows-latest - Test & Build: FAILURE  -> paso que falla: "Run clippy linter"
```

O sea: en Linux compila y pasa; en macOS y Windows **falla `cargo clippy --workspace --all-targets -- -D warnings`**.
Nadie miró nunca esos avisos porque el entorno local es Linux.

## Objetivo

Que `cargo clippy --workspace --all-targets -- -D warnings` pase **sin warnings en los tres targets**, y que el CI quede verde.

## Cómo (sin hardware Apple/Windows: lint cruzado)

```bash
rustup target add x86_64-apple-darwin x86_64-pc-windows-gnu   # (o el target que use el CI: mirá .github/workflows/ci.yml)
cargo clippy --workspace --all-targets --target x86_64-apple-darwin -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

Clippy en modo chequeo **no necesita linker ni SDK**, así que esto se puede correr acá. Si un target no se puede instalar,
decilo en el informe y arreglá igual los warnings que se vean leyendo el código `#[cfg(target_os = ...)]`.

Arreglá **la causa**, no la manifestación: nada de `#[allow(...)]` nuevos sobre bloques enteros, nada de borrar código
funcional para silenciar. Si un `allow` es genuinamente correcto (código específico de plataforma no usado en ese target),
ponelo **puntual, con un comentario que explique por qué**.

## Evidencia obligatoria

Escribí `delegacion/EVIDENCIA-ci.md` con: comando exacto, salida de clippy por target (últimas líneas, incluyendo el
`Finished` sin warnings), y qué cambiaste. Sin esa evidencia la ronda no cuenta.

## Aceptación (los 5 en 0)

```
cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Commit: `fix(ci): clippy sin warnings en macOS y Windows (CI verde en las 3 plataformas)`.
Prohibido sudo, prohibido tocar `/etc`, prohibido correr el A/B, prohibido modificar `.github/workflows/` (si el workflow
necesita un cambio, explicá por qué en el informe pero **no lo toques**).
