# Cómo contribuir a wayvpet (overlay para Wayland)

¡Gracias por el interés! 🐱 Este documento cubre el flujo de trabajo del código
**Rust** (Fase 0.5 de la migración). La versión en inglés vive en
`CONTRIBUTING.en.md` y describe el árbol C histórico.

## Requisitos

- Un compositor Wayland con `wlr-layer-shell` (Sway, Hyprland, river, KWin,
  COSMIC, …).
- Rust estable (edición 2021, MSRV **1.75**). Instálalo con [rustup](https://rustup.rs).
- Cabeceras de desarrollo de Wayland (`libwayland-dev` / `wayland-devel`).
- Pertenecer al grupo `input` para leer el teclado: `sudo usermod -aG input $USER`
  y volver a iniciar sesión.

`wayland-scanner` y `wayland-protocols` **no** hacen falta: los bindings
generados (incluido `protocols/cosmic-toplevel-info-unstable-v1.xml`) se
compilan desde el propio crate.

## Compilar y ejecutar

```bash
git clone <este-repo>
cd wayland-wayvpet-reload

cargo build            # debug
cargo build --release  # release (optimizado, LTO, panic=abort)

# ejecutar con vigilancia de la configuración
cargo run -p wayvpet -- -c wayvpet.conf.example -w
```

Utilidades sin compositor: `--print-default-config`, `--validate`, `--dry-run`.
Otras opciones: `--toggle`, `--supervise`, `--monitor NOMBRE`, `--no-toplevel`.

## Antes de enviar cambios

Ejecuta lo mismo que la CI (todo debe pasar):

```bash
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test
cargo build --release
```

`cargo fmt` aplica el formato. `scripts/check_comment_lang.sh` ayuda a detectar
comentarios en inglés residuales (no bloquea).

## Estilo de código

- **Comentarios y documentación nueva en español**, descriptivos: el *porqué* y
  los invariantes, no lo obvio. Los **identificadores** del código siguen en
  inglés.
- Sigue el patrón del código de alrededor (densidad de comentarios, nombres,
  idioma).
- `wayvpet-common` es lógica pura: **sin** Wayland, sin hilos, sin procesos,
  `#![forbid(unsafe_code)]`. La única E/S permitida está en su módulo `io`.
- En `wayvpet`, `unsafe` está denegado salvo `#[allow(unsafe_code)]` acotado a
  un punto de FFI, con un comentario `// SAFETY:` que justifique cada bloque.
- Añade pruebas para todo lo determinista (parseo, validación, aritmética,
  máquina de estados). Las pruebas de `wayvpet-common` van en el mismo módulo
  (`#[cfg(test)] mod tests`).
- Antes de tocar el modelo de procesos, hilos o el ciclo de vida de Wayland, lee
  `ARQUITECTURA.md`.

## Privacidad y seguridad

- `enable_debug=0` fuera de diagnóstico.
- El keycode **no** debe salir del proceso lector ni registrarse: solo cruza el
  bit de pata ya reducido.
- Conserva la validación de rutas `/dev/input/`, el fichero PID con `flock` /
  `O_NOFOLLOW` / `0600`, la lista blanca de seccomp y los topes de
  desbordamiento existentes.
- Nunca subas rutas de dispositivo personales ni configuración real.

## Mensajes de commit

[Conventional Commits](https://www.conventionalcommits.org), como en el historial:
`feat:`, `fix:`, `docs:`, `refactor:`, o con ámbito: `fix(build):`,
`feat(rust):`. Un commit, un cambio con foco.

En los PR: explica el comportamiento y la motivación, enlaza incidencias, lista
los comandos de validación, y para fallos en tiempo de ejecución indica
compositor y configuración. Incluye captura o grabación si cambia algo visible
del overlay.

## Reportar fallos

Incluye:

- Compositor y versión (`echo $XDG_CURRENT_DESKTOP`, versión del compositor).
- Contenido del `wayvpet.conf` (sin rutas personales).
- Salida del terminal (`wayvpet` es hablador por `stderr`).
- Si es de pantalla completa o multi-monitor: qué aplicación, qué salidas.
