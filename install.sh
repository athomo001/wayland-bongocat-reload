#!/usr/bin/env bash
# Instalador de un comando para wayvpet (spec 0002).
#
#   ./install.sh              instala en ~/.local  (sin sudo)
#   PREFIX=/usr/local sudo ./install.sh
#   DESTDIR=/tmp/pkg PREFIX=/usr ./install.sh     (para empaquetar)
#   ./install.sh --uninstall  quita los binarios (no toca tu config)
#
# Variables: PREFIX (por defecto ~/.local), DESTDIR (vacío), NO_BUILD=1 para
# saltar el `cargo build`.
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
DESTDIR="${DESTDIR:-}"
BINDIR="$DESTDIR$PREFIX/bin"
DATADIR="$DESTDIR$PREFIX/share/wayvpet"
MANDIR="$DESTDIR$PREFIX/share/man/man1"
APPDIR="$DESTDIR$PREFIX/share/applications"
CONFDIR="${XDG_CONFIG_HOME:-$HOME/.config}/wayvpet"

cd "$(dirname "$0")"

if [ "${1:-}" = "--uninstall" ]; then
  rm -fv "$BINDIR/wayvpet" "$BINDIR/wayvpetctl" "$BINDIR/wayvpet-config" \
         "$DATADIR/wayvpet.conf.example" "$MANDIR/wayvpet.1" \
         "$DATADIR"/presets/*.conf \
         "$APPDIR/wayvpet-config.desktop"
  rmdir --ignore-fail-on-non-empty "$DATADIR/presets" "$DATADIR" 2>/dev/null || true
  echo "Listo. Tu configuración en $CONFDIR no se ha tocado."
  echo "Si instalaste el servicio:  wayvpet --uninstall-service"
  exit 0
fi

if [ "${NO_BUILD:-}" != "1" ]; then
  echo ">> cargo build --release"
  cargo build --release --locked -p wayvpet -p wayvpetctl -p wayvpet-config
fi

install -Dm755 target/release/wayvpet        "$BINDIR/wayvpet"
install -Dm755 target/release/wayvpetctl     "$BINDIR/wayvpetctl"
# Ventana gráfica de configuración (spec 0007): la abre "Configurar…" del tray.
install -Dm755 target/release/wayvpet-config "$BINDIR/wayvpet-config"
install -Dm644 wayvpet.conf.example          "$DATADIR/wayvpet.conf.example"
# Presets (spec 0008 §8.2): .conf parciales que aplica `wayvpetctl preset apply`.
for p in presets/*.conf; do [ -f "$p" ] && install -Dm644 "$p" "$DATADIR/presets/$(basename "$p")"; done
install -Dm644 packaging/wayvpet-config.desktop "$APPDIR/wayvpet-config.desktop"
[ -f man/wayvpet.1 ] && install -Dm644 man/wayvpet.1 "$MANDIR/wayvpet.1" || true

# Configuración del usuario: solo si no existe (nunca se pisa la del usuario).
if [ -z "$DESTDIR" ] && [ ! -e "$CONFDIR/wayvpet.conf" ]; then
  install -Dm644 wayvpet.conf.example "$CONFDIR/wayvpet.conf"
  echo ">> Configuración inicial en $CONFDIR/wayvpet.conf"
fi

echo
echo "Instalado:"
echo "  $BINDIR/wayvpet"
echo "  $BINDIR/wayvpetctl"
echo "  $BINDIR/wayvpet-config   (ventana de configuración; o el tray → Configurar…)"
case ":$PATH:" in
  *":$PREFIX/bin:"*) : ;;
  *) echo
     echo "⚠  $PREFIX/bin no está en tu PATH. Añade a ~/.bashrc:"
     echo "     export PATH=\"$PREFIX/bin:\$PATH\"" ;;
esac
echo
echo "Arranca ahora:        wayvpet -w"
echo "Autoarranque (systemd): wayvpet --install-service && \\"
echo "                        systemctl --user enable --now wayvpet.service"
