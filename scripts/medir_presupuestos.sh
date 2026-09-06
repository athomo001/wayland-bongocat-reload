#!/usr/bin/env bash
# Mide la línea base de PRESUPUESTOS.md sobre un compositor real:
# tamaño de binario, RSS tras arranque y tras 60 s de reposo, y CPU en reposo.
# No necesita sysstat/pidstat: lee /proc directamente.
#
#   ./scripts/medir_presupuestos.sh [ruta-al-conf]
set -euo pipefail

CONF="${1:-wayvpet.conf.example}"
BIN=target/release/wayvpet

echo "== Compilando release =="
cargo build --release -p wayvpet -p wayvpetctl >/dev/null

echo
echo "== Tamaño de binario =="
for b in "$BIN" target/release/wayvpetctl; do
  printf '  %-28s %s bytes  (%s)\n' "$b" "$(stat -c%s "$b")" \
    "$(numfmt --to=iec --suffix=B "$(stat -c%s "$b")")"
done

echo
echo "== Arrancando overlay =="
"$BIN" -c "$CONF" & BPID=$!
trap 'kill "$BPID" 2>/dev/null || true' EXIT
sleep 5
[ -d "/proc/$BPID" ] || { echo "el overlay murió al arrancar"; exit 1; }

rss() { awk '/VmRSS/{print $2" "$3}' "/proc/$1/status"; }
cpu_ticks() { awk '{print $14+$15}' "/proc/$1/stat"; }   # utime+stime

echo "  RSS tras arranque : $(rss "$BPID")"
# proceso lector de input (hijo): RSS aparte
KID=$(pgrep -P "$BPID" -f "$BIN" || true)
[ -n "${KID:-}" ] && echo "  RSS proceso lector: $(rss "$KID")"

echo
echo "== 60 s de reposo (no toques el teclado) =="
T0=$(cpu_ticks "$BPID"); sleep 60; T1=$(cpu_ticks "$BPID")
HZ=$(getconf CLK_TCK)
PCT=$(awk -v d="$((T1 - T0))" -v hz="$HZ" 'BEGIN{printf "%.2f", d/hz/60*100}')

echo "  RSS tras 60 s idle: $(rss "$BPID")"
echo "  CPU en reposo     : ${PCT}%  (${HZ} Hz, $((T1 - T0)) ticks en 60 s)"

echo
echo "== --dry-run =="
( time "$BIN" -c "$CONF" --dry-run >/dev/null ) 2>&1 | sed 's/^/  /'
