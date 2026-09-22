#!/usr/bin/env bash
# vostro-install.sh — instala el WattWarden Rust (rama de paridad) y verifica que el daemon
# realmente esté escribiendo hardware como el Go. Pensado para correr UNA línea desde la Vostro:
#
#   cd ~/ww-rust; git pull -q; sudo bash delegacion/vostro-install.sh
#
# Idempotente: se puede correr las veces que quieras. No toca /etc/wattwarden/config.json
# (respeta tu config y tus brillos) y no borra nada.
set -uo pipefail

if [ "$(id -u)" -ne 0 ]; then echo "Corré con sudo." >&2; exit 1; fi
USER_HOME="$(getent passwd "${SUDO_USER:-juan}" | cut -d: -f6)"
REPO="$USER_HOME/ww-rust"
AS_USER="${SUDO_USER:-juan}"

echo "== 1/6 repo =="
cd "$REPO" || { echo "falta el repo en $REPO"; exit 1; }
git log --oneline -1

echo "== 2/6 build release (como $AS_USER) =="
BIN="$REPO/target/release/wattwarden"
if [ ! -x "$BIN" ] || [ -n "$(find "$REPO/crates" -newer "$BIN" -name '*.rs' -print -quit 2>/dev/null)" ]; then
  sudo -u "$AS_USER" env "PATH=$USER_HOME/.cargo/bin:/usr/bin:/bin" \
    bash -lc "cd '$REPO' && cargo build --release" 2>&1 | tail -5
else
  echo "binario al día"
fi
[ -x "$BIN" ] || { echo "FALLÓ el build: no existe $BIN"; exit 1; }
echo "binario: $(sha256sum "$BIN" | cut -c1-16) $(du -h "$BIN" | cut -f1)"

echo "== 3/6 instalar =="
install -m755 "$BIN" /usr/local/bin/wattwarden
/usr/local/bin/wattwarden --help >/dev/null 2>&1 && echo "--help OK (exit $?)"

echo "== 4/6 servicio =="
if [ ! -f /etc/systemd/system/wattwarden.service ]; then
  /usr/local/bin/wattwarden --install-service
else
  systemctl daemon-reload
fi
systemctl restart wattwarden
sleep 4
echo "servicio: $(systemctl is-active wattwarden) / $(systemctl is-enabled wattwarden 2>/dev/null)"

echo "== 5/6 estado del daemon =="
/usr/local/bin/wattwarden status
echo "--- últimas líneas del journal:"
journalctl -u wattwarden -n 12 --no-pager | tail -12

echo "== 6/6 ¿está escribiendo hardware? (espera 15 s, dos ticks de 5 s) =="
snap() {
  for f in /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq \
           /sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq \
           /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor \
           /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference \
           /sys/devices/system/cpu/cpu1/online /sys/devices/system/cpu/cpu3/online \
           /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw \
           /sys/class/backlight/*/brightness /sys/module/pcie_aspm/parameters/policy \
           /sys/module/iwlwifi/parameters/power_save /proc/sys/vm/dirty_writeback_centisecs \
           /proc/sys/kernel/nmi_watchdog /sys/class/power_supply/BAT0/status; do
    [ -f "$f" ] && printf '  %-62s %s\n' "$f" "$(cat "$f" 2>/dev/null)"
  done
}
echo "--- ahora:"; snap
sleep 15
echo "--- 15 s después (si cambió algo, el lazo está vivo):"; snap

echo
echo "LISTO. Para ver el dashboard (tu terminal, no por ssh):   sudo wattwarden"
echo "Para probar los niveles:   sudo wattwarden level medium   # low | medium | high"
echo "Para la comparación A/B contra el binario Go:   sudo bash $REPO/scripts/ab_parity.sh <go> <rust> 30"
