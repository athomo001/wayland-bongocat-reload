#!/bin/sh
# Publica una versión de wayvpet en un comando (spec 0002 M5).
#
#   scripts/release.sh 3.1.0            # fija la versión, compila, genera
#                                      # tarball + .deb + .rpm + SHA256SUMS
#   scripts/release.sh 3.1.0 --publish # además crea la release en GitHub (gh)
#   scripts/release.sh --artifacts     # solo re-genera artefactos con la
#                                      # versión actual (no toca Cargo.toml)
#
# No hace `git push` ni `git tag` por ti salvo con --publish.
# POSIX sh.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

say()  { printf '\033[1;36m>>\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

NEWVER=""
PUBLISH=0
for a in "$@"; do
	case "$a" in
		--publish) PUBLISH=1 ;;
		--artifacts) NEWVER="KEEP" ;;
		-h|--help) sed -n '2,14p' "$0"; exit 0 ;;
		[0-9]*.[0-9]*.[0-9]*) NEWVER="$a" ;;
		*) die "argumento no reconocido: $a" ;;
	esac
done
[ -n "$NEWVER" ] || die "falta la versión (ej: 3.1.0) o --artifacts"

# 1. fijar la versión en los crates que se publican
if [ "$NEWVER" != "KEEP" ]; then
	[ -z "$(git status --porcelain)" ] || die "el árbol de trabajo tiene cambios sin confirmar."
	say "Fijo la versión a $NEWVER"
	# Los 3 crates del workspace + los 2 de fuera del workspace (helper de
	# actualización y pack de vpets, spec 0015 / 0008 §8.4).
	for f in crates/wayvpet/Cargo.toml crates/wayvpetctl/Cargo.toml \
		crates/wayvpet-config/Cargo.toml crates/wayvpet-update/Cargo.toml \
		crates/wayvpet-vpets/Cargo.toml; do
		sed -i "s/^version = \".*\"/version = \"$NEWVER\"/" "$f"
	done
	CARGO=$(command -v cargo)
	"$CARGO" update -p wayvpet -p wayvpetctl -p wayvpet-config --offline >/dev/null 2>&1 || true
	( cd crates/wayvpet-update && "$CARGO" update --offline >/dev/null 2>&1 ) || true
	( cd crates/wayvpet-vpets  && "$CARGO" update --offline >/dev/null 2>&1 ) || true
	git add crates/*/Cargo.toml Cargo.lock crates/wayvpet-update/Cargo.lock crates/wayvpet-vpets/Cargo.lock
	git commit -m "release: v$NEWVER"
	VER="$NEWVER"
else
	VER=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/wayvpet/Cargo.toml | head -n1)
	say "Versión actual: $VER"
fi

# 2. compilar y generar artefactos: base + extras (vpets, helper de actualización)
say "make pkg && make pkg-extras"
make pkg
make pkg-extras

# 3. publicar (opcional)
if [ "$PUBLISH" = 1 ]; then
	command -v gh >/dev/null 2>&1 || die "necesito 'gh' (GitHub CLI) para --publish."
	TAG="v$VER"
	say "git tag $TAG && push"
	git tag -a "$TAG" -m "wayvpet $TAG"
	git push origin HEAD "$TAG"
	say "gh release create $TAG"
	gh release create "$TAG" dist/wayvpet* dist/SHA256SUMS \
		--title "wayvpet $TAG" --generate-notes
	say "Release publicada: https://github.com/athomo001/wayvpet/releases/tag/$TAG"
else
	say "Artefactos en dist/ (sin publicar). Para publicar:  $0 $VER --publish"
fi
