#!/usr/bin/env bash
# levels_cmp.sh — compara los 3 niveles del auto-extreme bajo CARGA CONTROLADA, en la máquina donde corre.
# Uso: sudo bash scripts/levels_cmp.sh      (en la Dell, con el daemon instalado)
# Cambia el nivel editando config.json: la CLI 'wattwarden level X' NO existe (cae al dashboard, como en Go).
set -uo pipefail
CFG=/etc/wattwarden/config.json
RAPL=/sys/class/powercap/intel-rapl:0
NPROC=$(nproc)

offline_cores() {   # cpu0 no se puede apagar: se excluye por patrón
  local n=0
  for c in /sys/devices/system/cpu/cpu[1-9]*/online; do
    [ -f "$c" ] && [ "$(cat "$c" 2>/dev/null)" = "0" ] && n=$((n+1))
  done
  echo "$n"
}

snap() {
  printf '   cores_off=%s  max_freq=%s  rapl_pl1=%sW  rapl_max=%sW  epp=%s  gov=%s\n' \
    "$(offline_cores)" \
    "$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq 2>/dev/null)" \
    "$(( $(cat $RAPL/constraint_0_power_limit_uw 2>/dev/null || echo 0) / 1000000 ))" \
    "$(( $(cat $RAPL/constraint_0_max_power_uw 2>/dev/null || echo 0) / 1000000 ))" \
    "$(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null)" \
    "$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null)"
}

echo "máquina: ${NPROC} cores | cpuinfo_max=$(cat /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq 2>/dev/null)" \
     "| batería: $(cat /sys/class/power_supply/BAT0/status 2>/dev/null)"
for lvl in low medium high; do
  sed -i "s/\"auto_extreme_level\": \"[a-z]*\"/\"auto_extreme_level\": \"$lvl\"/" "$CFG" || exit 1
  pids=""
  for _ in $(seq "$NPROC"); do yes >/dev/null 2>&1 & pids="$pids $!"; done   # carga: un busy-loop por core
  sleep 18
  echo "== $lvl (CON CARGA) =="; snap
  for p in $pids; do kill "$p" 2>/dev/null; done
  sleep 12
  echo "== $lvl (en reposo) =="; snap
done
echo "nivel final: $(grep -o '"auto_extreme_level": "[a-z]*"' "$CFG")"
pkill -x yes 2>/dev/null; echo "carga limpiada"
