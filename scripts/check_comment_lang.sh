#!/usr/bin/env bash
# check_comment_lang.sh — heuristica que marca comentarios escritos en ingles
# en el codigo fuente (C hoy, Rust tras la Fase 0.5).
#
# Regla del proyecto: comentarios y documentacion nueva en espanol. NO bloquea CI
# (da falsos positivos con nombres propios de Wayland/POSIX): es una ayuda para la
# revision.
#
#   scripts/check_comment_lang.sh [ruta ...]   # revisa las rutas, o src/ include/
#   scripts/check_comment_lang.sh --selftest   # comprueba la propia heuristica
#
# Salida != 0 si encuentra comentarios sospechosos de estar en ingles.

set -euo pipefail

# Palabras inglesas frecuentes en comentarios (palabra completa, sin mayusculas).
EN_WORDS="the|this|that|these|those|with|without|from|into|which|while|when|where|what|should|must|will|does|done|only|above|below|outside|inside|before|after|because|since|their|there|is|are|was|were|has|have|had|not|but|and|for|its|also|then|than|each|other|both|either|neither|always|never|note|ensure|keep|handle|return|returns|caller|thread|process|child|parent|lock|unlock|flush|skip|fallback|retry|wake"

# Terminos tecnicos que se dejan en ingles a proposito (no cuentan).
ALLOW_WORDS="layer|surface|roundtrip|eventfd|mmap|buffer|inotify|seccomp|landlock|pointer|keyboard|compositor|wayland|posix|shm|ipc|svg|png|apng|gif|hidpi|fractional|viewport|toplevel|hotplug|premultiplied|bgra|rgba|framebuffer"

# scan <fichero>...  -> imprime lineas sospechosas; devuelve 1 si hay alguna.
scan() {
  local rc=0 f n line body lower stripped count
  for f in "$@"; do
    [ -f "$f" ] || continue
    n=0
    while IFS= read -r line || [ -n "$line" ]; do
      n=$((n + 1))
      # Solo lineas que parezcan comentario de linea.
      case "$line" in
        *"//"*) body="${line#*//}" ;;
        "#"*|*" #"*|*$'\t'"#"*) body="${line#*#}" ;;
        *) continue ;;
      esac
      lower="$(printf '%s' "$body" | tr '[:upper:]' '[:lower:]')"
      stripped="$(printf '%s' "$lower" | grep -oE '[a-z]+' | grep -Ewv "$ALLOW_WORDS" || true)"
      count="$(printf '%s\n' "$stripped" | grep -Ewc "$EN_WORDS" || true)"
      if [ "${count:-0}" -ge 2 ]; then
        printf '%s:%d: %s\n' "$f" "$n" "$(printf '%s' "$line" | sed 's/^[[:space:]]*//')"
        rc=1
      fi
    done <"$f"
  done
  return $rc
}

selftest() {
  local tmp; tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' RETURN
  printf '%s\n' '// This flushes the buffer because the caller needs it now' >"$tmp/bad.c"
  printf '%s\n' '// Se hace flush del buffer porque el hilo lo necesita' >"$tmp/ok.c"
  scan "$tmp/bad.c" >/dev/null && { echo "selftest FAIL: no detecto el ingles" >&2; return 1; }
  scan "$tmp/ok.c"  >/dev/null || { echo "selftest FAIL: marco el espanol como ingles" >&2; return 1; }
  echo "selftest OK"
}

main() {
  if [ "${1:-}" = "--selftest" ]; then selftest; exit $?; fi
  local -a targets=()
  if [ "$#" -gt 0 ]; then
    targets=("$@")
  else
    while IFS= read -r p; do targets+=("$p"); done < <(
      find src include -type f \( -name '*.c' -o -name '*.h' -o -name '*.rs' \) 2>/dev/null || true)
  fi
  [ "${#targets[@]}" -gt 0 ] || { echo "sin ficheros que revisar"; exit 0; }
  if scan "${targets[@]}"; then
    echo "check_comment_lang: sin comentarios sospechosos de estar en ingles"
    exit 0
  else
    echo "check_comment_lang: revisa los comentarios de arriba (posible ingles)" >&2
    exit 1
  fi
}

main "$@"
