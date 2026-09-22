#!/usr/bin/env bash
# run-pi.sh — ronda de delegación PM(Antigravity) → junior(Command Code) EN LA PI.
# Uso: bash delegacion/run-pi.sh
# Verifica por EXIT CODES, nunca parseando texto. Deja log en ~/.cache/wattwarden/.
set -uo pipefail

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
REPO="$HOME/ww-rust"
BRANCH="feat/paridad-go-rust"
CACHE="$HOME/.cache/wattwarden"
TS="$(date +%Y%m%d-%H%M%S)"
LOG="$CACHE/round-$TS.log"
mkdir -p "$CACHE"; rm -f "$CACHE/BLOCKED"
cd "$REPO" || exit 1

log() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }

AGY="$(command -v agy || command -v antigravity)"
CC="$(command -v command-code)"
[ -x "$AGY" ] && [ -x "$CC" ] || { log "FALTA un harness (agy=$AGY cc=$CC)"; exit 1; }

has_flag() { "$1" --help 2>&1 | grep -q -- "$2"; }

# Cuenta SOLO commits del agente: los de orquestacion (mios) no valen como trabajo.
agent_commits() { git log --format=%s "$1"..HEAD | grep -vcE '^(docs|chore)\(delegacion\)|^fix\(parity\): evidencia|^feat\(parity\): contrato'; }

# ---------- preparación: rama + baseline commiteado ----------
git rev-parse --verify "$BRANCH" >/dev/null 2>&1 || git checkout -q -B "$BRANCH"
git rev-parse --abbrev-ref HEAD | grep -qx "$BRANCH" || git checkout -q "$BRANCH"
if ! git diff --quiet || [ -n "$(git status --porcelain)" ]; then
  git add -A && git commit -q -m "feat(parity): contrato PARITY.md/TUI-GAP.md + backend linux portado desde Go + arnes A/B + package de delegacion"
  log "baseline commiteado: $(git rev-parse --short HEAD)"
fi
git pull -q --ff-only 2>/dev/null

{ echo "--- agy"; "$AGY" --version; echo "--- cc"; "$CC" --version; } >>"$LOG" 2>&1
BASE="$(git rev-parse HEAD)"
touch "$CACHE/RUNNING"
log "BASE=$BASE  rama=$BRANCH"

baseline_test() { cargo test --workspace >/dev/null 2>&1; echo $?; }
T0="$(baseline_test)"
log "baseline cargo test exit=$T0"

# ---------- 1) PM: Antigravity ----------
log "=== PM (agy) arrancando ==="
AGY_ARGS=(-p "$(cat delegacion/prompt-pm.md)" --dangerously-skip-permissions)
has_flag "$AGY" --model && AGY_ARGS+=(--model gemini-3.8-flash-high)
has_flag "$AGY" --effort && AGY_ARGS+=(--effort high)
has_flag "$AGY" --print-timeout && AGY_ARGS+=(--print-timeout 60m)
[ "${SKIP_PM:-0}" = "1" ] || timeout 3900 "$AGY" "${AGY_ARGS[@]}" >>"$LOG" 2>&1
AGY_EXIT=$?
NEW_PM=$(agent_commits "$BASE")
log "agy exit=$AGY_EXIT commits_agente=$NEW_PM"

# nudge si el PM no dejó commits (patrón conocido: agy sale sin trabajar)
for i in 1 2; do
  [ "${SKIP_PM:-0}" = "1" ] && break
  [ "$NEW_PM" -gt 0 ] && break
  log "PM sin commits -> nudge $i"
  timeout 2400 "$AGY" -c -p "No hiciste commits. Implementá la paridad del daemon COMPLETA en este turno: los dos tickers (5s logica / 300ms brillo), escalones discretos 0/0.333/0.667/1.0, techo 40%, EPP power + turbo por escalon, los 7 perifericos por tick, cambio de rama por IsCharging() en cada tick, SIGTERM/SIGHUP, y los tests con sysfs falso. Prohibido terminar el turno sin commits." --dangerously-skip-permissions >>"$LOG" 2>&1
  NEW_PM=$(git rev-list "$BASE"..HEAD --count)
  log "nudge $i -> commits_nuevos=$NEW_PM"
done

MID="$(git rev-parse HEAD)"

# ---------- 2) Junior: Command Code ----------
log "=== Junior (command-code) arrancando ==="
MODEL="${WATTWARDEN_JUNIOR_MODEL:-meta/muse-spark-1.3-contributor}"
log "modelo junior: $MODEL"
PROMPT_JUNIOR="${PROMPT_JUNIOR:-delegacion/prompt-junior.md}"
CC_ARGS=(-p "$(cat "$PROMPT_JUNIOR")" --trust --dangerously-skip-permissions --tools-all --max-turns 240 -m "$MODEL")
has_flag "$CC" --skip-onboarding && CC_ARGS+=(--skip-onboarding)
timeout 3900 "$CC" "${CC_ARGS[@]}" >>"$LOG" 2>&1
CC_EXIT=$?
NEW_JR=$(agent_commits "$MID")
log "command-code exit=$CC_EXIT commits_agente=$NEW_JR"

for i in 1 2; do
  [ "$NEW_JR" -gt 0 ] && break
  log "junior sin commits -> nudge $i"
  timeout 2400 "$CC" -c -p "No hiciste commits. Completá la paridad de CLI/TUI en este turno: flags y strings exactos de Go (incluido que un flag desconocido NO es error de parseo), SyncInstalledBinary, unidad systemd exacta, StopDaemon() antes de cada escritura manual, [ACTIVE]/[OFF], y los tests golden. Prohibido terminar el turno sin commits." --trust --dangerously-skip-permissions --tools-all --max-turns 240 -m "$MODEL" >>"$LOG" 2>&1
  NEW_JR=$(git rev-list "$MID"..HEAD --count)
  log "nudge $i -> commits_nuevos=$NEW_JR"
done

# ---------- 3) verificación DURA por exit codes ----------
# Rescatar restos del agente ANTES de verificar: si no, el trabajo queda sin commitear y
# la ronda da rojo por higiene del repo (paso el 22/09 con config.rs + daemon.rs, 796 lineas).
if [ -n "$(git status --porcelain)" ]; then
  git add -u && git commit -q -m "wip(delegacion): restos del agente sin commitear (rescatados por el runner)"
  log "restos del agente rescatados en $(git rev-parse --short HEAD)"
fi

log "=== verificación ==="
cargo fmt >>"$LOG" 2>&1; FMT=$?
cargo fmt --check >>"$LOG" 2>&1; FMT_C=$?
cargo build --all-targets >>"$LOG" 2>&1; BUILD=$?
cargo test --workspace >>"$LOG" 2>&1; TEST=$?
cargo clippy --workspace --all-targets -- -D warnings >>"$LOG" 2>&1; CLIPPY=$?
DIRTY="$(git status --porcelain | wc -l)"
TOTAL=$(git rev-list "$BASE"..HEAD --count)

ROJO=""
[ "$FMT_C" != 0 ] && ROJO="$ROJO fmt"
[ "$BUILD" != 0 ] && ROJO="$ROJO build"
[ "$TEST"  != 0 ] && ROJO="$ROJO test"
[ "$CLIPPY" != 0 ] && ROJO="$ROJO clippy"
[ "$DIRTY" != 0 ] && ROJO="$ROJO arbol_sucio($DIRTY)"
[ "$NEW_PM" = 0 ] && [ "$NEW_JR" = 0 ] && ROJO="$ROJO cero_commits_agente"

echo "round=$TS base=$BASE total_commits=$TOTAL pm_commits=$NEW_PM jr_commits=$NEW_JR fmt=$FMT_C build=$BUILD test=$TEST clippy=$CLIPPY dirty=$DIRTY" > "$CACHE/last-summary"
rm -f "$CACHE/RUNNING"

if [ -n "$ROJO" ]; then
  echo "ROJO:$ROJO" > "$CACHE/BLOCKED"
  echo "❌ WattWarden paridad: ronda $TS con problemas →$ROJO"
  echo "   commits=$TOTAL (PM=$NEW_PM, junior=$NEW_JR) | log: $LOG"
  grep -E "^error|FAILED|test result: FAILED" "$LOG" | tail -6
  exit 1
fi

git push -q origin "$BRANCH" >>"$LOG" 2>&1 && pushed="pusheado" || pushed="PUSH FALLO"
echo "✅ WattWarden paridad: ronda $TS verde — $TOTAL commits nuevos ($pushed), build/test/clippy/fmt OK, árbol limpio."
echo "   log: $LOG"
git log --oneline "$BASE"..HEAD | head -12
