#!/usr/bin/env bash
# ci_watch.sh — vigila el CI de GitHub de ww-rust y, si está ROJO, lanza una ronda de reparación automática.
# Diseñado para cron no_agent: stdout VACÍO = silencio (nada que reportar).
# No interfiere con rondas en curso (centinela RUNNING o proceso run-pi vivo).
set -uo pipefail

REPO_DIR=/home/juan/ww-rust
CACHE=/home/juan/.cache/wattwarden
STATE=$CACHE/ciwatch
BRANCH=feat/paridad-go-rust
MAX_INTENTOS=3
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
mkdir -p "$STATE"

# ---------- no interferir con una ronda en curso ----------
no_hay_ronda() {
  [ -f "$CACHE/RUNNING" ] && return 1
  pgrep -f "[r]un-pi\.sh" >/dev/null 2>&1 && return 1
  pgrep -f "[c]ommand-code" >/dev/null 2>&1 && return 1
  pgrep -f "[f]or p in prompt-" >/dev/null 2>&1 && return 1   # cadena de rondas encadenadas
  return 0
}
no_hay_ronda || exit 0
# doble chequeo: una cadena encadenada tiene huecos de segundos entre rondas
sleep 90
no_hay_ronda || exit 0

cd "$REPO_DIR" 2>/dev/null || exit 0
SHA=$(git rev-parse HEAD 2>/dev/null) || exit 0
SHORT=$(git rev-parse --short HEAD 2>/dev/null)

# ---------- estado del CI para ESTE commit ----------
timeout 60 gh run list --branch "$BRANCH" --limit 15 \
  --json headSha,status,conclusion,databaseId >"$STATE/runs.json" 2>/dev/null || exit 0

read -r STATUS CONCLUSION RUNID <<<"$(python3 - "$STATE/runs.json" "$SHA" <<'PY'
import json,sys
try:
    runs = json.load(open(sys.argv[1]))
except Exception:
    print("unknown none none"); raise SystemExit
for r in runs:
    if r.get("headSha") == sys.argv[2]:
        print(r.get("status","unknown"), r.get("conclusion") or "none", r.get("databaseId","none"))
        break
else:
    print("none none none")
PY
)"

# ---------- sin corrida para este commit, o todavía corriendo: silencio ----------
case "$STATUS" in
  none|unknown|queued|in_progress|requested|waiting|pending) exit 0 ;;
esac

# ---------- verde: avisar una sola vez por commit ----------
if [ "$CONCLUSION" = "success" ]; then
  if [ ! -f "$STATE/green_$SHA" ]; then
    touch "$STATE/green_$SHA"
    rm -f "$STATE/pending_$SHA"
    echo "✅ CI verde en las 3 plataformas (ubuntu/macOS/Windows) para $SHORT — todo funcional y sin warnings."
  fi
  exit 0
fi

# ---------- rojo: reparar automáticamente ----------
INTENTOS=$(cat "$STATE/att_$SHA" 2>/dev/null || echo 0)
if [ "$INTENTOS" -ge "$MAX_INTENTOS" ]; then
  if [ ! -f "$STATE/tope_$SHA" ]; then
    touch "$STATE/tope_$SHA"
    echo "⚠️ CI sigue rojo en $SHORT tras $MAX_INTENTOS rondas automáticas. Necesita mano humana: revisá el run $RUNID."
  fi
  exit 0
fi
INTENTOS=$((INTENTOS + 1))
echo "$INTENTOS" >"$STATE/att_$SHA"

# job y paso que fallan + cola del log
FALLA=$(timeout 60 gh run view "$RUNID" --json jobs \
  --jq '.jobs[] | select(.conclusion=="failure") | "\(.name) -> " + ([.steps[] | select(.conclusion=="failure") | .name] | join(", "))' 2>/dev/null | head -3)
LOGLINE=$(timeout 90 gh run view "$RUNID" --log-failed 2>/dev/null | tail -60)

PROMPT="delegacion/prompt-auto-ci-$SHORT.md"
{
  echo "# Prompt Junior — Reparación automática del CI (intento $INTENTOS de $MAX_INTENTOS)"
  echo
  echo "Sos el **junior**. Repo \`/home/juan/ww-rust\`, rama \`$BRANCH\`. **No toques \`/home/juan/wattwarden\`** (sólo leer)."
  echo
  echo "El CI de GitHub falló en el commit \`$SHORT\` (run \`$RUNID\`), ANTES de que el trabajo llegue a las Dell."
  echo "El job/paso que falla:"
  echo
  echo '```'
  echo "${FALLA:-(no se pudo leer el job)}"
  echo '```'
  echo
  echo "Últimas líneas del log del paso fallado:"
  echo
  echo '```'
  echo "${LOGLINE:-(sin log disponible)}"
  echo '```'
  echo
  echo "## Qué hacer"
  echo
  echo "1. Reproducí el fallo en esta máquina (Linux) con el comando exacto del paso. Para lints de otra plataforma:"
  echo '   `rustup target add x86_64-apple-darwin x86_64-pc-windows-gnu` y `cargo clippy --workspace --all-targets --target <target> -- -D warnings`'
  echo "   (clippy en modo chequeo no necesita SDK ni linker)."
  echo "2. Arreglá **la causa**, no la manifestación: nada de \`#[allow(...)]\` nuevos para tapar warnings, nada de borrar"
  echo "   código funcional, nada de relajar tests. Si un \`allow\` es genuinamente correcto, que sea puntual y con comentario."
  echo "3. **Prohibido** tocar \`.github/workflows/\` para que el CI pase: el CI es el juez, no el acusado."
  echo "4. No rompas Linux: la paridad y los tests existentes tienen que seguir verdes."
  echo
  echo "## Aceptación (los 4 en 0)"
  echo
  echo '```'
  echo 'cargo fmt && cargo fmt --check && cargo build --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings'
  echo '```'
  echo
  echo "Commit: \`fix(ci): reparar fallo del CI en $SHORT ($(echo "$FALLA" | head -1 | cut -c1-60))\`."
  echo "Prohibido sudo, prohibido tocar \`/etc\`, prohibido correr el A/B."
} >"$PROMPT"

rm -f "$CACHE/RUNNING"
(cd "$REPO_DIR" && SKIP_PM=1 PROMPT_JUNIOR="$PROMPT" setsid nohup bash delegacion/run-pi.sh \
  >>"$STATE/launch_$SHORT.log" 2>&1 &)

echo "🔧 CI rojo en $SHORT → ronda de reparación automática #$INTENTOS lanzada (falla: $(echo "$FALLA" | head -1 | cut -c1-70))"
