#!/bin/sh
# Instalador de un comando para wayvpet (spec 0002 M3-M4).
#
#   curl -fsSL https://raw.githubusercontent.com/athomo001/wayvpet/main/install.sh | sh
#   ./install.sh                      # PREFIX=$HOME/.local  (sin sudo)
#   sudo ./install.sh --prefix /usr/local
#   ./install.sh --yes               # no interactivo (para CI / scripts)
#   ./install.sh --uninstall         # delega en ./uninstall.sh
#
# Opciones:
#   --prefix DIR        Prefijo de instalación (por defecto ~/.local, o
#                       /usr/local si se ejecuta como root)
#   --yes, -y          No preguntar nada (asume "sí" salvo lo peligroso)
#   --no-service       No instalar la unidad systemd de usuario
#   --no-input-group   No tocar la pertenencia al grupo `input`
#   --no-deps          No intentar instalar dependencias de compilación
#                      (se comprueba primero si ya están; si sí, ni se mira el
#                       gestor de paquetes ni se pide sudo)
#   --no-vpets         No instalar el pack de vpets pesados (miku/umbreon/gabumon)
#   --uninstall        Desinstalar (llama a uninstall.sh)
#
# POSIX sh a propósito (sin bashismos): funciona con dash/busybox.
set -eu

REPO_URL="https://github.com/athomo001/wayvpet.git"
ASSUME_YES=0
WANT_SERVICE=1
WANT_INPUT_GROUP=1
WANT_DEPS=1
WANT_VPETS=1
PREFIX=""

say()  { printf '\033[1;36m>>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

ask_yes() {
	# ask_yes "pregunta"  ->  0 si sí. Con --yes o sin TTY, asume sí.
	[ "$ASSUME_YES" = 1 ] && return 0
	[ -t 0 ] || return 0
	printf '%s [S/n] ' "$1"
	read -r _r || return 0
	case "$_r" in n|N|no|NO) return 1 ;; *) return 0 ;; esac
}

# ---- argumentos -------------------------------------------------------------
while [ $# -gt 0 ]; do
	case "$1" in
		--uninstall)
			d=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
			[ -x "$d/uninstall.sh" ] || die "no encuentro $d/uninstall.sh"
			shift
			exec "$d/uninstall.sh" "$@"
			;;
		--prefix) PREFIX="${2:?--prefix necesita una ruta}"; shift 2 ;;
		--prefix=*) PREFIX="${1#--prefix=}"; shift ;;
		-y|--yes) ASSUME_YES=1; shift ;;
		--no-service) WANT_SERVICE=0; shift ;;
		--no-input-group) WANT_INPUT_GROUP=0; shift ;;
		--no-deps) WANT_DEPS=0; shift ;;
		--no-vpets) WANT_VPETS=0; shift ;;
		-h|--help) sed -n '2,20p' "$0"; exit 0 ;;
		*) die "opción desconocida: $1 (prueba --help)" ;;
	esac
done

# ---- preámbulo ------------------------------------------------------------
[ "$(uname -s)" = "Linux" ] || die "wayvpet solo funciona en Linux."
if [ -z "${WAYLAND_DISPLAY:-}" ] && [ "${XDG_SESSION_TYPE:-}" != "wayland" ]; then
	warn "No parece una sesión Wayland. wayvpet necesita un compositor Wayland para dibujar."
fi

IS_ROOT=0
[ "$(id -u)" = 0 ] && IS_ROOT=1
if [ -z "$PREFIX" ]; then
	if [ "$IS_ROOT" = 1 ]; then PREFIX="/usr/local"; else PREFIX="$HOME/.local"; fi
fi

SUDO=""
if [ "$IS_ROOT" = 0 ] && command -v sudo >/dev/null 2>&1; then SUDO="sudo"; fi

# ---- detección de distribución ----------------------------------------------
# Familias objetivo (únicas soportadas oficialmente): arch, debian, fedora.
DISTRO_FAMILY="unknown"
if [ -r /etc/os-release ]; then
	# shellcheck disable=SC1091
	. /etc/os-release
	# ID_LIKE es una lista separada por espacios: se quiere el troceo.
	# shellcheck disable=SC2086
	for id in "${ID:-}" ${ID_LIKE:-}; do
		case "$id" in
			arch|manjaro|endeavouros|cachyos|garuda|arcolinux) DISTRO_FAMILY="arch"; break ;;
			debian|ubuntu|linuxmint|pop|elementary|zorin|kali|raspbian|devuan) DISTRO_FAMILY="debian"; break ;;
			fedora|nobara|rhel|centos|rocky|almalinux) DISTRO_FAMILY="fedora"; break ;;
		esac
	done
fi
say "Distribución: ${ID:-desconocida}  (familia: $DISTRO_FAMILY)"

# Lista de dependencias de compilación que faltan (vacío = están todas). Se
# comprueba con las herramientas reales, no con el gestor de paquetes: así una
# máquina que ya las tiene se salta el `apt-get update` / `sudo` y va directa a
# compilar. `pkg-config --exists` mira los `.pc` que traen los paquetes `-dev`.
missing_deps() {
	_m=""
	command -v cargo      >/dev/null 2>&1 || _m="$_m cargo"
	command -v make       >/dev/null 2>&1 || _m="$_m make"
	command -v pkg-config >/dev/null 2>&1 || _m="$_m pkg-config"
	command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || command -v clang >/dev/null 2>&1 || _m="$_m compilador-C"
	if command -v pkg-config >/dev/null 2>&1; then
		pkg-config --exists wayland-client 2>/dev/null || _m="$_m libwayland-dev"
		pkg-config --exists xkbcommon      2>/dev/null || _m="$_m libxkbcommon-dev"
	else
		_m="$_m libwayland-dev libxkbcommon-dev"
	fi
	printf '%s' "${_m# }"
}

# $pkgs va sin comillas a propósito: es una lista de paquetes que debe trocearse.
# shellcheck disable=SC2086
install_deps() {
	[ "$WANT_DEPS" = 1 ] || { say "Salto la instalación de dependencias (--no-deps)."; return 0; }

	_miss=$(missing_deps)
	if [ -z "$_miss" ]; then
		say "Dependencias de compilación ya presentes ($(cargo --version 2>/dev/null || echo cargo)); no instalo nada."
		return 0
	fi
	say "Faltan dependencias: $_miss"

	case "$DISTRO_FAMILY" in
		arch)
			pkgs="rust wayland libxkbcommon pkgconf make"
			say "Dependencias (pacman): $pkgs"
			ask_yes "¿Instalar con pacman?" && $SUDO pacman -S --needed --noconfirm $pkgs || warn "Sigo sin instalar dependencias."
			;;
		debian)
			pkgs="cargo rustc libwayland-dev libxkbcommon-dev pkg-config make"
			say "Dependencias (apt): $pkgs"
			ask_yes "¿Instalar con apt-get?" && { $SUDO apt-get update && $SUDO apt-get install -y $pkgs; } || warn "Sigo sin instalar dependencias."
			;;
		fedora)
			pkgs="cargo rust wayland-devel libxkbcommon-devel pkgconf make"
			say "Dependencias (dnf): $pkgs"
			ask_yes "¿Instalar con dnf?" && $SUDO dnf install -y $pkgs || warn "Sigo sin instalar dependencias."
			;;
		*)
			warn "Distribución no soportada oficialmente (solo Debian/Ubuntu, Fedora, Arch)."
			warn "Instala a mano: toolchain de Rust (>=1.85), libwayland, libxkbcommon, pkg-config, make."
			ask_yes "¿Continuar de todos modos?" || die "Cancelado."
			;;
	esac
}

# ---- obtener el código -----------------------------------------------------
SRC="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
CLONED=0
if [ -f "$SRC/Cargo.toml" ] && [ -d "$SRC/crates/wayvpet" ]; then
	say "Uso el árbol de fuentes en $SRC"
else
	command -v git >/dev/null 2>&1 || die "necesito git para clonar el código."
	SRC="$(mktemp -d)"
	CLONED=1
	say "Clono $REPO_URL en $SRC"
	git clone --depth 1 "$REPO_URL" "$SRC"
fi
cleanup() { [ "$CLONED" = 1 ] && rm -rf "$SRC"; }
trap cleanup EXIT INT TERM

# ---- dependencias + compilar + instalar ----------------------------------
install_deps

command -v cargo >/dev/null 2>&1 || die "no encuentro cargo tras instalar dependencias. ¿rustup en el PATH?"
command -v make  >/dev/null 2>&1 || die "no encuentro make."

cd "$SRC"
say "Compilo e instalo (make install PREFIX=$PREFIX)"
if [ "$PREFIX" = "/usr/local" ] || [ "$PREFIX" = "/usr" ]; then
	MAKE_SUDO="$SUDO"
else
	MAKE_SUDO=""
fi
# CHANNEL=source: el aviso de nueva versión (spec 0015) sabrá que actualizar es
# re-ejecutar este script, no `apt`/`dnf`/`pacman`.
$MAKE_SUDO make install PREFIX="$PREFIX" CHANNEL=source

# Pack de vpets pesados (miku/umbreon/gabumon, ~13 MB). En los .deb/.rpm es un
# paquete aparte; en una instalación desde fuente se incluye salvo --no-vpets.
if [ "$WANT_VPETS" = 1 ] && [ -d "$SRC/vpets" ]; then
	say "Instalo el pack de vpets (miku, umbreon, gabumon)"
	# shellcheck disable=SC2086
	$MAKE_SUDO make install-vpets PREFIX="$PREFIX"
fi

BINDIR="$PREFIX/bin"

# ---- config inicial del usuario (solo si no existe) ----------------------
CONFDIR="${XDG_CONFIG_HOME:-$HOME/.config}/wayvpet"
if [ "$IS_ROOT" = 0 ] && [ ! -e "$CONFDIR/wayvpet.conf" ]; then
	mkdir -p "$CONFDIR"
	cp "$SRC/wayvpet.conf.example" "$CONFDIR/wayvpet.conf"
	say "Configuración inicial en $CONFDIR/wayvpet.conf"
fi

# Limpia el autoarranque roto del nombre viejo.
rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/bongocat.desktop"

# ---- grupo input ---------------------------------------------------------
if [ "$WANT_INPUT_GROUP" = 1 ] && [ "$IS_ROOT" = 0 ]; then
	if id -nG "$(id -un)" 2>/dev/null | tr ' ' '\n' | grep -qx input; then
		say "Ya estás en el grupo 'input'."
	else
		_me=$(id -un)
		warn "No estás en el grupo 'input' (hace falta para leer /dev/input)."
		if ask_yes "¿Añadirte con 'sudo usermod -aG input $_me'?"; then
			$SUDO usermod -aG input "$_me" && say "Hecho. Cierra y abre sesión para que surta efecto."
		else
			warn "Salta. Añádete luego con:  sudo usermod -aG input $_me"
		fi
	fi
fi

# ---- teclado -----------------------------------------------------------
if [ -x "$BINDIR/wayvpet-find-devices" ] && [ "$IS_ROOT" = 0 ]; then
	say "Para localizar tu teclado luego:  wayvpet-find-devices"
fi

# ---- autoarranque (systemd de usuario) --------------------------------
if [ "$WANT_SERVICE" = 1 ] && [ "$IS_ROOT" = 0 ]; then
	if ask_yes "¿Activar el autoarranque con la sesión (systemd de usuario)?"; then
		"$BINDIR/wayvpet" --install-service || warn "No se pudo instalar la unidad."
		systemctl --user enable --now wayvpet.service 2>/dev/null \
			|| warn "Actívalo luego:  systemctl --user enable --now wayvpet.service"
	fi
fi

# ---- resumen ---------------------------------------------------------
echo
say "Instalado en $PREFIX"
echo "    $BINDIR/wayvpet, wayvpetctl, wayvpet-config, wayvpet-find-devices"
echo "    Busca «wayvpet» en el menú de aplicaciones (icono incluido)."
case ":${PATH}:" in
	*":$BINDIR:"*) : ;;
	*) echo
	   warn "$BINDIR no está en tu PATH. Añade a ~/.profile:"
	   echo "     export PATH=\"$BINDIR:\$PATH\"" ;;
esac
echo
echo "Arrancar ahora:  wayvpet -w      (o desde el menú de aplicaciones)"
echo "Configurar:      wayvpet-config  (o el icono de la bandeja → Configurar…)"
