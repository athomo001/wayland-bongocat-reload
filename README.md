# wayvpet — overlay para Wayland

[![Licencia: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)

<a href="https://www.buymeacoffee.com/athomo"><img src="https://img.buymeacoffee.com/button-api/?text=Comprame un cafecito&emoji=&slug=athomo&button_colour=FFDD00&font_colour=000000&font_family=Cookie&outline_colour=000000&coffee_colour=ffffff" /></a>

Un overlay para Wayland que muestra un gato bongo animado reaccionando a lo que
escribes. Este fork (**wayvpet**) reescribió el núcleo en Rust
y añadió instalación en un comando, control remoto (`wayvpetctl` + icono de
bandeja), temas/skins con animación de sprite sheet, entrada de ratón, modo de
edición con el ratón, y endurecimiento de seguridad/privacidad.

> 🇬🇧 English: [README.en.md](README.en.md) — desactualizado respecto a este
> fichero; si lo necesitas, pide que se sincronice.

![Demo](assets/demo.gif)

## Qué hace

- 🎯 Animación en tiempo real al teclear (y, opcional, al mover/clicar el ratón)
- 🎨 Temas/skins desde disco: SVG vectorial o sprite sheet PNG/APNG animado,
  con importador de mascotas de [wayland-vpets](https://github.com/furudbat/wayland-vpets)
- 🖱️ Modo edición: arrastra y redimensiona el gato con el ratón, sin editar
  ficheros
- 🔔 Icono en la bandeja del sistema: mostrar/ocultar, recargar, elegir tema,
  configurar, cerrar — todo con el ratón
- 🖥️ Control remoto (`wayvpetctl`) y socket IPC para automatizar/scriptear
- 🔥 Recarga de configuración en caliente (`-w`), incluidos los ficheros del
  tema activo
- 🎮 Se auto-oculta en aplicaciones a pantalla completa
- 🖥️ Soporte multi-monitor
- 😴 Modo reposo por inactividad o por horario
- 🔒 El lector de teclado corre en un proceso aparte con `seccomp`: nunca sabe
  ni registra qué tecla pulsaste, solo "izquierda o derecha"
- ⚡ Ligero: un solo binario Rust, sin runtime ni dependencias de escritorio
  pesadas

## Instalación

```bash
git clone https://github.com/athomo001/wayvpet.git
cd wayvpet
./install.sh
```

Compila con `cargo` (requiere Rust estable, MSRV 1.80) e instala en
`~/.local/bin` por defecto — sin `sudo`. Para instalar en todo el sistema:

```bash
PREFIX=/usr/local sudo ./install.sh
```

El instalador también copia una configuración inicial en
`~/.config/wayvpet/wayvpet.conf` **solo si no existe ya una tuya** — nunca
pisa una configuración existente.

### Permisos

Leer `/dev/input/` requiere pertenecer al grupo `input`:

```bash
sudo usermod -a -G input $USER
# Cierra sesión y vuelve a entrar
```

### Ejecutar

```bash
wayvpet -w   # -w = recarga en caliente al editar la configuración
```

El teclado (y el ratón, si `enable_mouse=1`) se **autodetectan**; solo hace
falta fijar `keyboard_device=`/`mouse_device=` a mano si la autodetección no
acierta (`./scripts/find_input_devices.sh` los lista).

## Configuración

Edita `~/.config/wayvpet/wayvpet.conf` — cada clave está documentada con
más detalle en [`wayvpet.conf.example`](wayvpet.conf.example).

<details>
<summary>Todas las opciones</summary>

| Opción | Valores | Por defecto | Descripción |
| --- | --- | --- | --- |
| `cat_height` | 10–200 | 40 | Tamaño del gato en píxeles |
| `cat_opacity` | 0–100 | 100 | Opacidad del gato en % |
| `cat_align` | left/center/right | center | Alineación horizontal |
| `cat_x_offset` | entero | 100 | Desplazamiento horizontal desde la alineación |
| `cat_y_offset` | entero | 10 | Desplazamiento vertical desde el anclaje |
| `theme` | nombre o ruta | vacío (embebido) | Tema activo — ver [Temas](#temas) |
| `mirror_x` / `mirror_y` | 0/1 | 0 | Voltear el gato en horizontal / vertical |
| `overlay_height` | 20–300 | 50 | Altura de la barra del overlay en píxeles |
| `overlay_opacity` | 0–255 | 150 | Opacidad del fondo (0 = transparente) |
| `overlay_position` | top/bottom | top | Borde de la pantalla |
| `layer` | background/bottom/top/overlay | top | Capa de Wayland |
| `keyboard_device` | ruta `/dev/input/…` | auto | Dispositivo evdev a monitorear (repetible) |
| `keyboard_name` | texto | — | Casar el dispositivo por nombre (para hotplug) |
| `enable_mouse` | 0/1 | 1 | Animar también con el ratón |
| `mouse_paw` | left/right/random | right | Qué pata usa el ratón |
| `mouse_move_interval` | ms | 50 | Cada cuánto un golpecito mientras mueves el ratón |
| `mouse_device` | ruta `/dev/input/…` | auto | Dispositivo del ratón (repetible) |
| `monitor` | lista separada por comas | auto | Monitores en los que dibujar |
| `idle_frame` | 0–4 | 0 | Fotograma en reposo (solo `classic`/temas SVG) |
| `happy_kpm` | 0–10000 | 0 (off) | Teclas/min para el estado "feliz" (si el tema lo trae) |
| `keypress_duration` | ms | 100 | Cuánto se mantiene la pata bajada tras una pulsación |
| `enable_hand_mapping` | 0/1 | 1 | Mapear teclas a mano izquierda/derecha |
| `idle_sleep_timeout` | segundos | 0 | Dormir tras inactividad (0 = desactivado) |
| `enable_scheduled_sleep` | 0/1 | 0 | Activar reposo por horario |
| `sleep_begin` / `sleep_end` | HH:MM | 00:00 | Inicio / fin del horario de reposo |
| `disable_fullscreen_hide` | 0/1 | 0 | Mantener el overlay visible en pantalla completa |
| `hotplug_scan_interval` | segundos | 30 | Cada cuánto rescanear dispositivos (0 = una vez) |
| `enable_ipc` | 0/1 | 1 | Socket de control para `wayvpetctl`/el tray |
| `enable_tray` | 0/1 | 1 | Icono en la bandeja del sistema |

`fps` sigue aceptándose por compatibilidad del fichero, pero ya no tiene
efecto: el bucle de animación se reprograma solo cuando hace falta, en vez de
sondear sin parar a un ritmo fijo.

Cambiar el número de monitores en marcha requiere reiniciar; el resto se
recarga en caliente con `-w` (incluidos los ficheros del tema activo).

</details>

## Temas

Un tema es una carpeta con SVG (vectorial) o una hoja de sprites PNG/APNG
(animada, con estados como `idle`/`writing`/`sleep`/`happy`/`boring`). Guía
completa en [`themes/README.md`](themes/README.md).

![Vpets: miku, umbreon, gabumon](assets/readme-vpets.gif)

Los vpets pesados (`miku`, `umbreon`, `gabumon`) van en el paquete aparte
`wayvpet-vpets`; los ligeros (`classic`, `pink`, `demo`) en el base. Regenerar
este GIF: `cargo run -p wayvpet --example gen_readme_gif`.

```bash
wayvpet theme list                  # temas instalados
wayvpet theme new mi-skin           # crea una plantilla en $XDG_DATA_HOME
wayvpet theme check mi-skin         # valida un tema sin arrancar el overlay
wayvpet theme import-vpets ORIGEN   # importa una mascota de wayland-vpets
```

`import-vpets` acepta una carpeta de mascota, un `.conf` estilo wayland-vpets,
una hoja PNG suelta, una carpeta de PNGs por estado, o un APNG — traduce las
claves, informa de lo que no pudo mapear, y nunca falla por un estado ausente.
Ver [`themes/COMUNIDAD.md`](themes/COMUNIDAD.md) (aviso de licencias/IP antes
de importar mascotas de terceros).

## Icono de bandeja

Con `enable_tray=1` (por defecto) aparece un icono en la bandeja del sistema.
Menú: **Mostrar/Ocultar** · **Configurar…** (abre `wayvpet-config`) · **Modo
edición** (con marca ✓; arrastra el vpet con el ratón) · **Reiniciar overlay** ·
**Recargar configuración** · **Tema ▸** (lista los temas instalados, marca el
activo, cambia con un clic) · **Acerca de** · **Cerrar**.

El protocolo es **StatusNotifierItem** (D-Bus puro, sin `libappindicator` ni
`libdbus`). Por escritorio:

| Escritorio | Estado |
| --- | --- |
| **KDE Plasma** | nativo, funciona sin nada |
| **COSMIC** | nativo (applet de estado del panel) |
| **Sway / Hyprland / wlroots** | con **waybar** (`"tray"`) o con un panel que traiga bandeja |
| **XFCE / MATE / Cinnamon / LXQt** | applet "Área de notificación / de estado" del panel |
| **GNOME** | necesita la extensión **AppIndicator and KStatusNotifierItem Support** ([extensions.gnome.org/extension/615](https://extensions.gnome.org/extension/615/appindicator-support/)). Ubuntu la trae de fábrica; Fedora Workstation no |

Si el panel o la extensión aún no están listos al iniciar sesión, `wayvpet`
**reintenta el registro ~30 s** antes de rendirse (así lo recoge un panel que
carga tarde o una extensión recién activada). Si aun así no aparece, arranca
`wayvpet-config` a mano o usa `wayvpetctl`.

`--no-tray` lo desactiva para una ejecución sin tocar la config.

## Modo edición (ratón)

Arrastra el gato con el botón izquierdo; la rueda cambia el tamaño. Al salir
persiste la posición/tamaño en el `.conf`. Un contorno marca los límites
mientras está activo. Tres formas de entrar/salir, todas equivalentes:

- **Tray**: clic en "Modo edición" (con marca ✓ mientras está activo) — la más
  fácil, sin terminal.
- `wayvpetctl edit on` / `wayvpetctl edit off`.
- IPC: `EDIT on` / `EDIT off` / `EDIT toggle`.

## Control remoto — `wayvpetctl`

Para automatizar o atar a atajos de teclado del compositor:

```bash
wayvpetctl show|hide|toggle      # mostrar/ocultar a mano
wayvpetctl theme next            # rotar temas
wayvpetctl theme set NOMBRE      # cambiar a un tema concreto
wayvpetctl state                 # estado actual de la instancia (JSON-like)
wayvpetctl get-live CLAVE        # leer una clave de la instancia en marcha
wayvpetctl set-live CLAVE VALOR  # cambiarla en caliente (sin tocar el fichero)
wayvpetctl save                  # persistir al .conf lo cambiado con set-live
wayvpetctl stop                  # cerrar la instancia
```

`wayvpetctl -h` lista todas las órdenes, incluidas las de fichero (`get`/
`set`/`dump`/`default`) que no necesitan una instancia en marcha.

## Autoarranque (systemd)

```bash
wayvpet --install-service
systemctl --user enable --now wayvpet.service
```

`wayvpet --uninstall-service` quita la unidad.

## Desinstalación

```bash
./install.sh --uninstall
```

Quita los binarios y la unidad de ejemplo instalados; **no toca** tu
`~/.config/wayvpet/wayvpet.conf`. Si instalaste el servicio systemd, quítalo
antes con `wayvpet --uninstall-service`.

## Actualizaciones

wayvpet **no comprueba si hay versiones nuevas** salvo que lo actives tú
(`check_updates=1` en el `.conf`, o la casilla "Avisar de nuevas versiones" en la
ventana de configuración). Con `check_updates=0` (por defecto) **nunca abre un
socket de red**.

Cuando lo activas: al **arrancar** wayvpet, un proceso hijo corto hace **una**
consulta HTTPS a la API de releases de GitHub (con un suelo de 1 h para no
repetir si reinicias en bucle) y termina. No hay timer, ni demonio, ni polling.
La consulta lleva solo `User-Agent: wayvpet/<versión>` — ningún identificador.

Si hay una versión más nueva, aparece **🔔 Versión nueva** en el icono de la
bandeja. Al pulsarlo se abre un diálogo (versión, fecha, notas) con:

- **Descargar** — baja el paquete que corresponde a tu instalación a
  `~/Descargas`, **verifica su SHA-256** y abre la carpeta. **No instala nada**:
  lo instalas tú con tu gestor.
- **Ver en el navegador** — abre la página del release.

El chequeo lo hace el binario **`wayvpet-update-check`**, que viene en el paquete
opcional `wayvpet-update` (`Recommends:` del paquete base; `make update-helper`
+ `make install-update-helper` desde fuente). Sin él, `check_updates=1` no hace
nada.

| Instalación | Cómo se actualiza | ¿Aviso? |
|---|---|---|
| `.deb` / `.rpm` sueltos, `install.sh`, AUR | descargas el paquete nuevo y lo instalas | sí (opt-in) |
| repositorios de la distro (`install-channel=distro`) | tu gestor de paquetes | **no** (lo gestiona la distro) |

Para scripts: `wayvpetctl update` imprime el estado sin tocar la red;
`wayvpetctl update --check` fuerza una consulta.

## Privacidad

El teclado (y el ratón) se leen en un **proceso aparte** con `seccomp`
(lista blanca de syscalls): al padre solo le llega un bit por pulsación
("pata izquierda" o "pata derecha"), nunca la tecla ni su código. `enable_debug`
existe en el fichero de config por compatibilidad pero no tiene ningún efecto
hoy — no hay ningún camino de código que registre teclas, con o sin él.

## Solución de problemas

<details>
<summary>Permiso denegado en el dispositivo de entrada</summary>

```bash
sudo usermod -a -G input $USER   # y vuelve a iniciar sesión
```

</details>

<details>
<summary>El gato no responde al teclado</summary>

1. `./scripts/find_input_devices.sh` para encontrar el dispositivo correcto
2. Fija `keyboard_device=` en la configuración
3. Reinicia wayvpet

</details>

<details>
<summary>No aparece en el monitor correcto</summary>

Pon `monitor=TU_MONITOR` (uno) o `monitor=MON1,MON2` (varios). Los nombres se
ven con `wlr-randr` o `hyprctl monitors`.

</details>

<details>
<summary>No aparece el icono de la bandeja</summary>

Revisa que tu panel tenga un applet de "área de estado"/bandeja del sistema
(en GNOME, la extensión AppIndicator). Si `wayvpet` imprime "icono de bandeja
activo" en la terminal pero no ves nada, es el panel, no wayvpet.

</details>

## Compilar

```bash
cargo build --release   # binarios en target/release/
cargo test               # toda la batería de tests
```

## Empaquetado (`.deb`, `.rpm`, tarball, AUR)

Todo sale a `dist/` (ignorado por git). Necesitas las dos herramientas de cargo
para los paquetes nativos: `cargo install --locked cargo-deb cargo-generate-rpm`.

Hay **tres** paquetes: `wayvpet` (base, ~3 MB), `wayvpet-vpets` (vpets pesados,
~9 MB) y `wayvpet-update` (helper del aviso, ~0,7 MB). Los dos últimos son
opcionales (`Recommends:` del base).

| Quiero… | Comando |
|---|---|
| Base (tarball + `.deb` + `.rpm`) + `SHA256SUMS` | `make pkg` |
| Solo el tarball reproducible (fuente / AUR) | `make dist` |
| Solo el `.deb` base (Debian/Ubuntu) | `make deb` |
| Solo el `.rpm` base (Fedora) | `make rpm` |
| El pack de vpets pesados | `make deb-vpets` / `make rpm-vpets` |
| El helper de actualización | `make deb-update` / `make rpm-update` |
| **Los cuatro extras** de golpe | `make pkg-extras` |
| Publicar una versión nueva en GitHub | `scripts/release.sh X.Y.Z --publish` |

`make pkg` encadena `release → dist → deb → rpm → checksums`; cada objetivo
intermedio funciona por separado. Release completa: `make pkg && make pkg-extras`.
El paquete de Arch se construye a partir de
[`packaging/PKGBUILD`](packaging/PKGBUILD) (`makepkg`), y el workflow de release
lo sube al AUR.

**Añadir un vpet pesado nuevo:** crea `vpets/<nombre>/` y ya está — los globs del
empaquetado y las rutas de búsqueda lo recogen solos, sin tocar `Cargo.toml` ni
el `Makefile`. Detalles en [`packaging/README.md`](packaging/README.md).

Comprobar el contenido de un paquete: `dpkg -c dist/*.deb` · `rpm -qlp dist/*.rpm`.

Detalles (metadatos, canales de instalación, flujo de release):
[`packaging/README.md`](packaging/README.md).

### Aviso de nueva versión (opcional)

El chequeo de actualizaciones (spec 0015) es un binario **aparte**,
`wayvpet-update-check`, que solo se compila cuando lo pides — así el núcleo no
enlaza el cliente HTTPS (`rustls`) ni pesa de más:

```bash
make update-helper           # compila crates/wayvpet-update con la feature 'net'
make install-update-helper   # lo instala junto a wayvpet
```

Sin él, `check_updates=1` en la config no hace nada (queda apagado en silencio).
Nunca instala una actualización: como mucho descarga el paquete y lo verificas tú.

## Estado

El núcleo ya está reescrito en Rust con paridad funcional y endurecimiento de
seguridad sobre el C original. En curso: interfaz de configuración a pantalla
completa (TUI/GUI) e importación completa de packs de wayland-vpets (GIF,
listado de una instalación local).

## Licencia

MIT — ver [LICENSE](LICENSE).
