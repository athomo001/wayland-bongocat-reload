# Empaquetado y publicación de wayvpet

Cómo generar los instaladores (`.deb`, `.rpm`, tarball, PKGBUILD) y publicar una
release. Para **instalar** wayvpet como usuario, mira el `README.md` de la raíz.

## Resumen

| Quiero… | Comando |
|---|---|
| Un tarball de fuentes reproducible | `make dist` |
| El `.deb` (Debian/Ubuntu) | `make deb` |
| El `.rpm` (Fedora) | `make rpm` |
| Todo + checksums en `dist/` | `make pkg` |
| Publicar una versión nueva en GitHub | `scripts/release.sh 3.1.0 --publish` |

Todo sale a `dist/` (ignorado por git).

## Requisitos

- El toolchain normal de compilación (Rust estable, `libwayland-dev` /
  `wayland-devel`, `pkg-config`, `make`).
- Para `make deb` / `make rpm`, dos herramientas de cargo:

  ```bash
  cargo install --locked cargo-deb cargo-generate-rpm
  ```

- Para `--publish`, la CLI de GitHub (`gh`) autenticada (`gh auth login`).

## `make pkg` — generar todos los artefactos

```bash
make pkg
```

Hace, en orden:

1. `make release` — `cargo build --release --workspace --locked`.
2. `make dist` — `git archive` del `HEAD` actual →
   `dist/wayvpet-<version>.tar.gz`. **Solo incluye lo que está en git**: si has
   añadido ficheros y no los has `git add`-eado, no entran en el tarball (sí en
   el `.deb`/`.rpm`, que empaquetan el árbol de trabajo).
3. `make deb` — `cargo deb` con la metadata de
   `crates/wayvpet/Cargo.toml` (`[package.metadata.deb]`).
4. `make rpm` — `cargo generate-rpm` con `[package.metadata.generate-rpm]`. RPM
   no admite `-` en la versión, así que `3.0.0-dev` se convierte en `3.0.0~dev`.
5. `make checksums` — `dist/SHA256SUMS`.

Resultado típico:

```
dist/
├── SHA256SUMS
├── wayvpet-<version>.tar.gz
├── wayvpet_<version>-1_amd64.deb
└── wayvpet-<version>-1.x86_64.rpm
```

### Comprobar el contenido de un paquete

```bash
dpkg -c dist/*.deb          # ficheros del .deb
dpkg -I dist/*.deb          # metadata de control (Depends, Maintainer, scripts)
rpm  -qlp dist/*.rpm        # ficheros del .rpm
```

## `scripts/release.sh` — publicar una versión

```bash
# 1) fija la versión, compila, genera artefactos (NO publica)
scripts/release.sh 3.1.0

# 2) además: git tag v3.1.0 + push + gh release create con los artefactos
scripts/release.sh 3.1.0 --publish

# solo re-generar artefactos con la versión actual del Cargo.toml
scripts/release.sh --artifacts
```

`release.sh 3.1.0` exige el árbol de trabajo **limpio**, fija `version = "3.1.0"`
en los tres crates publicables (`wayvpet`, `wayvpetctl`, `wayvpet-config`),
commitea `release: v3.1.0` y corre `make pkg`. Con `--publish` crea el tag, hace
`push` y `gh release create v3.1.0 dist/* --generate-notes`.

Al publicar la release, dos workflows se disparan solos:

- **`.github/workflows/package.yml`** — reconstruye `.deb`/`.rpm` en CI limpio,
  los valida (`dpkg -c` / `rpm -qlp`) y los adjunta a la release.
- **`.github/workflows/aur-publish.yml`** — actualiza el paquete `wayvpet` en el
  AUR desde `packaging/PKGBUILD` (necesita los secretos `AUR_USERNAME`,
  `AUR_EMAIL`, `AUR_SSH_PRIVATE_KEY` en el repo; si no están, el job falla a
  propósito para avisar).

## Qué instala cada canal

Todos los caminos instalan **el mismo árbol de ficheros** (lo define el
`Makefile`, objetivo `install`):

```
$PREFIX/bin/{wayvpet,wayvpetctl,wayvpet-config,wayvpet-find-devices}
$PREFIX/share/wayvpet/{wayvpet.conf.example,themes/**,presets/*.conf,install-channel}
$PREFIX/share/man/man1/wayvpet.1
$PREFIX/share/applications/{wayvpet,wayvpet-config}.desktop
$PREFIX/share/icons/hicolor/64x64/apps/wayvpet.png
$UNITDIR/wayvpet.service          # lib/systemd/user en /usr; share/systemd/user en ~/.local
```

El fichero `install-channel` marca de dónde vino la instalación
(`source` / `deb` / `rpm` / `arch` / `distro`). Lo usa el aviso de nueva
versión (spec 0015) para elegir el artefacto de descarga y para callarse cuando
la actualización la gestiona el gestor de paquetes de la distro.

| Canal | Cómo se instala | Cómo se actualiza | `install-channel` |
|---|---|---|---|
| Debian/Ubuntu | `apt install ./wayvpet_*.deb` | `apt` | `deb` |
| Fedora | `dnf install ./wayvpet-*.rpm` | `dnf` | `rpm` |
| Arch (AUR) | `makepkg` / `yay -S wayvpet` | `pacman`/`yay` | `arch` |
| Otras distros / desde fuente | `./install.sh` | re-ejecutar `install.sh` | `source` |

`install.sh` es POSIX sh, detecta la familia de distro por `/etc/os-release`,
instala las dependencias de compilación (con confirmación) y delega la copia de
ficheros en `make install`. `uninstall.sh` es su inverso (delega en
`make uninstall`); ninguno toca `~/.config/wayvpet`.

## Ficheros de esta carpeta

| Fichero | Para qué |
|---|---|
| `wayvpet.desktop` | Lanzador del menú de aplicaciones (arranca el overlay) |
| `wayvpet-config.desktop` | Lanzador de la ventana de configuración |
| `systemd/wayvpet.service` | Unidad systemd **de usuario**; el `ExecStart=` se reescribe a ruta absoluta al instalar |
| `deb/{postinst,postrm}` | Scripts del `.deb`: refrescan índices de escritorio, recuerdan el grupo `input` y `systemctl --user` |
| `channel/{deb,rpm,arch}` | Contenido del fichero `install-channel` según el paquete |
| `PKGBUILD` | Referencia para el AUR; el workflow rellena `pkgver` y `sha256sums` |
