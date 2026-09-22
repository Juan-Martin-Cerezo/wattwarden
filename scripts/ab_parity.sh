#!/usr/bin/env bash
# ab_parity.sh — comparación A/B de hardware: binario Go (referencia) vs binario Rust.
#
# Uso (EN LA VOSTRO, con batería, como root):
#   sudo ./ab_parity.sh /path/wattwarden-go /path/wattwarden-rust [segundos] [--on-ac]
#
# Qué hace: apaga el daemon instalado, corre cada binario en modo --daemon N segundos
# (default 30) muestreando sysfs cada 1 s, junta los dos traces y los diffea.
#
# Seguridad: toma un snapshot de TODOS los valores muestreados antes de empezar y los
# restaura al terminar (incluso si algo falla). No toca el binario instalado ni la config.
set -uo pipefail

if [ "$(id -u)" -ne 0 ]; then echo "Necesita root (sysfs de escritura)." >&2; exit 1; fi
if [ $# -lt 2 ]; then sed -n '2,7p' "$0"; exit 1; fi

BIN_A="$1"; BIN_B="$2"; SECS="${3:-30}"; MODE_FLAG="${4:-}"
for b in "$BIN_A" "$BIN_B"; do
  [ -x "$b" ] || { echo "No ejecutable: $b" >&2; exit 1; }
done
OUT="$(mktemp -d /tmp/ww-ab.XXXXXX)"
echo "Artefactos en: $OUT"

# --- nodos a muestrear (los que Go escribe y el reporte de Juan toca) ---
BAT="$(ls -d /sys/class/power_supply/BAT* /sys/class/power_supply/BATT 2>/dev/null | head -1)"
BL="$(ls -d /sys/class/backlight/* 2>/dev/null | head -1)"
GPU=""
for c in /sys/class/drm/card1 /sys/class/drm/card0; do [ -e "$c/gt_max_freq_mhz" ] && GPU="$c" && break; done
RAPL=$(ls -d /sys/class/powercap/intel-rapl:0 2>/dev/null | head -1)

KEYS=()
for i in $(seq 0 15); do
  [ -e "/sys/devices/system/cpu/cpu$i/online" ] && KEYS+=("/sys/devices/system/cpu/cpu$i/online")
done
[ -e /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq ] && KEYS+=(
  /sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq
  /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq
  /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
  /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference)
[ -n "$RAPL" ] && KEYS+=("$RAPL"/constraint_*_power_limit_uw "$RAPL"/constraint_*_name)
[ -n "$GPU" ] && KEYS+=("$GPU"/gt_min_freq_mhz "$GPU"/gt_max_freq_mhz)
[ -n "$BL" ] && KEYS+=("$BL"/brightness "$BL"/max_brightness)
[ -n "$BAT" ] && KEYS+=("$BAT"/capacity "$BAT"/status /sys/class/power_supply/AC/online)
KEYS+=(
  /sys/module/pcie_aspm/parameters/policy
  /sys/module/iwlwifi/parameters/power_save
  /sys/module/snd_hda_intel/parameters/power_save
  /sys/module/snd_hda_intel/parameters/power_save_controller
  /proc/sys/kernel/nmi_watchdog
  /proc/sys/vm/dirty_writeback_centisecs
  /sys/devices/system/cpu/intel_pstate/no_turbo
  /sys/devices/system/cpu/cpufreq/boost
  /sys/class/leds/*kbd_backlight/brightness)

# Expandir globs, deduplicar, conservar solo lo que existe
mapfile -t NODES < <(for k in "${KEYS[@]}"; do for f in $k; do [ -f "$f" ] && echo "$f"; done; done | sort -u)
echo "Nodos muestreados: ${#NODES[@]}"

sample() { # $1 = archivo destino
  : > "$1"
  for f in "${NODES[@]}"; do printf '%s\t%s\n' "$f" "$(cat "$f" 2>/dev/null | tr -d '\n')" >> "$1"; done
}
snapshot() { for f in "${NODES[@]}"; do echo "$(cat "$f" 2>/dev/null | tr -d '\n')" > "$OUT/orig_$(echo "$f" | tr / _)"; done; }
restore() {
  for f in "${NODES[@]}"; do
    s="$OUT/orig_$(echo "$f" | tr / _)"
    [ -s "$s" ] && printf '%s' "$(cat "$s")" > "$f" 2>/dev/null
  done
}

echo "--- snapshot de estado original"
snapshot
trap 'echo "--- restaurando estado"; restore' EXIT

# Detener servicios/daemons para que no ensucien la medición
systemctl stop wattwarden.service 2>/dev/null
pkill -x wattwarden 2>/dev/null; sleep 1

run_case() { # $1=binario $2=etiqueta
  local bin="$1" tag="$2" trace="$OUT/trace-$2.tsv"
  echo "--- [$tag] $(basename "$bin") — $SECS s"
  if [ "$MODE_FLAG" = "--on-ac" ]; then echo "(asumiendo AC: el binario decide por IsCharging igual)"
  fi
  "$bin" --daemon >"$OUT/log-$tag.txt" 2>&1 &
  local pid=$!
  local i=0
  : > "$trace"
  while [ $i -lt "$SECS" ]; do
    sleep 1; i=$((i+1))
    { echo "### t=${i}s"; sample "$OUT/s.$i"; cat "$OUT/s.$i"; } >> "$trace"
  done
  kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  sleep 1
}

run_case "$BIN_A" go
restore; sleep 1
run_case "$BIN_B" rust
restore

# --- resumen: último valor de cada nodo en cada corrida ---
last_of() { awk -v t="$2" '$1=="###" && $0 ~ ("t=" t "s") {f=1; next} /^###/ {f=0} f {print}' "$1" | tail -n +1; }

{
  echo "path	GO (final)	RUST (final)	IGUAL?"
  for f in "${NODES[@]}"; do
    g=$(awk -v p="$f" -F'\t' '$1==p{v=$2} END{print v}' "$OUT/trace-go.tsv")
    r=$(awk -v p="$f" -F'\t' '$1==p{v=$2} END{print v}' "$OUT/trace-rust.tsv")
    mark=$([ "$g" = "$r" ] && echo OK || echo "DIF")
    printf '%s\t%s\t%s\t%s\n' "$f" "${g:-?}" "${r:-?}" "$mark"
  done
} | tee "$OUT/diff.tsv"

echo
echo "=== DIFERENCIAS ==="
awk -F'\t' 'NR>1 && $4=="DIF"' "$OUT/diff.tsv" | sed 's/^/  /' || true
echo
echo "Traces completos: $OUT/trace-go.tsv  $OUT/trace-rust.tsv"
echo "Logs: $OUT/log-go.txt  $OUT/log-rust.txt"
echo "Estado restaurado. Si algo quedó raro: revisá los orig_* en $OUT."
