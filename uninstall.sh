#!/bin/sh
# Desinstalador de wayvpet (spec 0002 M4). Simétrico a install.sh: quita los
# binarios, datos, `.desktop`, icono y unidad systemd, pero **nunca** toca la
# configuración del usuario en ~/.config/wayvpet.
#
#   ./uninstall.sh                 # PREFIX=$HOME/.local  (o /usr/local si root)
#   sudo ./uninstall.sh --prefix /usr/local
#   ./uninstall.sh --keep-service  # no tocar la unidad systemd de usuario
#
# POSIX sh (sin bashismos).
set -eu

PREFIX=""
WANT_SERVICE=1

say()  { printf '\033[1;36m>>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
	case "$1" in
		--prefix) PREFIX="${2:?--prefix necesita una ruta}"; shift 2 ;;
		--prefix=*) PREFIX="${1#--prefix=}"; shift ;;
		--keep-service) WANT_SERVICE=0; shift ;;
		-h|--help) sed -n '2,12p' "$0"; exit 0 ;;
		*) die "opción desconocida: $1" ;;
	esac
done

IS_ROOT=0
[ "$(id -u)" = 0 ] && IS_ROOT=1
if [ -z "$PREFIX" ]; then
	if [ "$IS_ROOT" = 1 ]; then PREFIX="/usr/local"; else PREFIX="$HOME/.local"; fi
fi

SUDO=""
if [ "$IS_ROOT" = 0 ] && command -v sudo >/dev/null 2>&1; then SUDO="sudo"; fi

# Unidad systemd de usuario (la instala `wayvpet --install-service` en el HOME).
if [ "$WANT_SERVICE" = 1 ] && [ "$IS_ROOT" = 0 ] && command -v wayvpet >/dev/null 2>&1; then
	say "Quito la unidad systemd de usuario"
	wayvpet --uninstall-service || warn "No se pudo (quizá no estaba instalada)."
fi

SRC="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
if [ -f "$SRC/Makefile" ]; then
	say "make uninstall PREFIX=$PREFIX"
	if [ "$PREFIX" = "/usr/local" ] || [ "$PREFIX" = "/usr" ]; then MK_SUDO="$SUDO"; else MK_SUDO=""; fi
	$MK_SUDO make -C "$SRC" uninstall PREFIX="$PREFIX"
else
	# Sin árbol de fuentes: borrado a mano de las rutas conocidas.
	say "Sin Makefile a mano; borro las rutas conocidas bajo $PREFIX"
	B="$PREFIX/bin"; D="$PREFIX/share"
	SUDO_RM=""
	[ -w "$B" ] || SUDO_RM="$SUDO"
	$SUDO_RM rm -fv \
		"$B/wayvpet" "$B/wayvpetctl" "$B/wayvpet-config" "$B/wayvpet-find-devices" \
		"$D/man/man1/wayvpet.1" \
		"$D/applications/wayvpet.desktop" "$D/applications/wayvpet-config.desktop" \
		"$D/icons/hicolor/64x64/apps/wayvpet.png" \
		"$PREFIX/lib/systemd/user/wayvpet.service"
	$SUDO_RM rm -rf "$D/wayvpet"
fi

# Refresca los índices de escritorio solo si el directorio sigue compartido con
# otras apps (quedan .desktop). Si era exclusivo de wayvpet, no hay nada que
# reindexar y se limpia el mimeinfo.cache huérfano.
APPS="$PREFIX/share/applications"
if [ -d "$APPS" ] && ls "$APPS"/*.desktop >/dev/null 2>&1; then
	command -v update-desktop-database >/dev/null 2>&1 && \
		update-desktop-database "$APPS" >/dev/null 2>&1 || true
else
	rm -f "$APPS/mimeinfo.cache" 2>/dev/null || true
	rmdir "$APPS" 2>/dev/null || true
fi
if [ -d "$PREFIX/share/icons/hicolor/64x64/apps" ]; then
	command -v gtk-update-icon-cache >/dev/null 2>&1 && \
		gtk-update-icon-cache -qtf "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

# Autoarranque roto del nombre viejo, por si quedó.
rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/bongocat.desktop"

echo
say "wayvpet desinstalado."
echo "    Tu configuración en ${XDG_CONFIG_HOME:-$HOME/.config}/wayvpet NO se ha tocado."
