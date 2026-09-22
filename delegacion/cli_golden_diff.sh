#!/usr/bin/env bash
# cli_golden_diff.sh — compara CLI Rust (Vostro) vs CLI Go (binario amd64 real). Sin root.
set -uo pipefail
RUST="${1:-$HOME/ww-rust/target/release/wattwarden}"
GO="${2:-/tmp/wattwarden-go-amd64}"
PASS=0; FAIL=0
for args in "--status" "--brightness" "--help" "--version" "" "bogus-flag-xyz" "--level" "level low"; do
  r_out="$("$RUST" $args 2>&1)"; r_rc=$?
  g_out="$("$GO" $args 2>&1)"; g_rc=$?
  if [ "$r_out" = "$g_out" ] && [ "$r_rc" = "$g_rc" ]; then
    printf 'IGUAL  rc=%-3s  wattwarden %s\n' "$r_rc" "${args:-(sin args)}"; PASS=$((PASS+1))
  else
    printf 'DIF    args=%-14s rc rust=%s go=%s\n' "${args:-(sin args)}" "$r_rc" "$g_rc"; FAIL=$((FAIL+1))
    diff <(printf '%s\n' "$g_out") <(printf '%s\n' "$r_out") | head -8 | sed 's/^/       /'
  fi
done
echo "--- PASS=$PASS FAIL=$FAIL"
