#!/usr/bin/env bash
# Instalador de un comando para Bongo Cat (spec 0002).
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
DATADIR="$DESTDIR$PREFIX/share/bongocat"
MANDIR="$DESTDIR$PREFIX/share/man/man1"
APPDIR="$DESTDIR$PREFIX/share/applications"
CONFDIR="${XDG_CONFIG_HOME:-$HOME/.config}/bongocat"

cd "$(dirname "$0")"

if [ "${1:-}" = "--uninstall" ]; then
  rm -fv "$BINDIR/bongocat" "$BINDIR/bongocatctl" "$BINDIR/bongocat-config" \
         "$DATADIR/bongocat.conf.example" "$MANDIR/bongocat.1" \
         "$APPDIR/bongocat-config.desktop"
  rmdir --ignore-fail-on-non-empty "$DATADIR" 2>/dev/null || true
  echo "Listo. Tu configuración en $CONFDIR no se ha tocado."
  echo "Si instalaste el servicio:  bongocat --uninstall-service"
  exit 0
fi

if [ "${NO_BUILD:-}" != "1" ]; then
  echo ">> cargo build --release"
  cargo build --release --locked -p bongocat -p bongocatctl -p bongocat-config
fi

install -Dm755 target/release/bongocat        "$BINDIR/bongocat"
install -Dm755 target/release/bongocatctl     "$BINDIR/bongocatctl"
# Ventana gráfica de configuración (spec 0007): la abre "Configurar…" del tray.
install -Dm755 target/release/bongocat-config "$BINDIR/bongocat-config"
install -Dm644 bongocat.conf.example          "$DATADIR/bongocat.conf.example"
install -Dm644 packaging/bongocat-config.desktop "$APPDIR/bongocat-config.desktop"
[ -f man/bongocat.1 ] && install -Dm644 man/bongocat.1 "$MANDIR/bongocat.1" || true

# Configuración del usuario: solo si no existe (nunca se pisa la del usuario).
if [ -z "$DESTDIR" ] && [ ! -e "$CONFDIR/bongocat.conf" ]; then
  install -Dm644 bongocat.conf.example "$CONFDIR/bongocat.conf"
  echo ">> Configuración inicial en $CONFDIR/bongocat.conf"
fi

echo
echo "Instalado:"
echo "  $BINDIR/bongocat"
echo "  $BINDIR/bongocatctl"
echo "  $BINDIR/bongocat-config   (ventana de configuración; o el tray → Configurar…)"
case ":$PATH:" in
  *":$PREFIX/bin:"*) : ;;
  *) echo
     echo "⚠  $PREFIX/bin no está en tu PATH. Añade a ~/.bashrc:"
     echo "     export PATH=\"$PREFIX/bin:\$PATH\"" ;;
esac
echo
echo "Arranca ahora:        bongocat -w"
echo "Autoarranque (systemd): bongocat --install-service && \\"
echo "                        systemctl --user enable --now bongocat.service"
