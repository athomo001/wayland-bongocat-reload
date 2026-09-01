# Bongo Cat — overlay para Wayland

[![Licencia: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Versión](https://img.shields.io/badge/version-2.0.2-blue.svg)](https://github.com/saatvik333/wayland-bongocat/releases)

Un overlay para Wayland que muestra un gato bongo animado reaccionando a lo que
escribes.

> 🇬🇧 English: [README.en.md](README.en.md)

![Demo](assets/demo.gif)

## Qué hace

- 🎯 Animación del teclado en tiempo real
- 🔥 Recarga de configuración en caliente
- 🎮 Se auto-oculta en aplicaciones a pantalla completa
- 🖥️ Soporte multi-monitor
- 😴 Modo reposo por inactividad o por horario
- 🎨 Render basado en SVG (nítido a cualquier tamaño)
- ⚡ Ligero (~8 MB de RAM)

## Instalación rápida

### Arch Linux

```bash
yay -S bongocat
```

### Otras distribuciones — compilar desde fuente

```bash
git clone https://github.com/saatvik333/wayland-bongocat.git
cd wayland-bongocat && make
```

**Requisitos:** `wayland-client`, `gcc`/`clang` (C23), `make`.

### Permisos

```bash
sudo usermod -a -G input $USER
# Cierra sesión y vuelve a entrar
```

### Encontrar tu teclado

```bash
bongocat-find-devices   # o ./scripts/find_input_devices.sh
```

### Ejecutar

```bash
bongocat --watch-config
# Opcional: forzar un monitor concreto
bongocat --watch-config --monitor eDP-1
```

## Configuración

Crea `~/.config/bongocat/bongocat.conf` a partir de
[`bongocat.conf.example`](bongocat.conf.example) (cada clave está documentada
ahí).

<details>
<summary>Todas las opciones</summary>

| Opción | Valores | Por defecto | Descripción |
| --- | --- | --- | --- |
| `cat_height` | 10–200 | 40 | Tamaño del gato en píxeles |
| `cat_align` | left/center/right | center | Alineación horizontal |
| `cat_x_offset` | entero | 100 | Desplazamiento horizontal desde la alineación |
| `cat_y_offset` | entero | 10 | Desplazamiento vertical desde el centro |
| `overlay_height` | 20–300 | 50 | Altura de la barra del overlay en píxeles |
| `overlay_opacity` | 0–255 | 150 | Opacidad del fondo (0 = transparente) |
| `overlay_position` | top/bottom | top | Borde de la pantalla |
| `layer` | background/bottom/top/overlay | top | Capa de Wayland |
| `keyboard_device` | ruta `/dev/input/…` | auto | Dispositivo evdev a monitorear |
| `keyboard_name` | texto | — | Casar el dispositivo por nombre (para hotplug) |
| `monitor` | lista separada por comas | auto | Monitores en los que dibujar |
| `fps` | 1–120 | 60 | Fotogramas por segundo de la animación |
| `mirror_x` / `mirror_y` | 0/1 | 0 | Voltear el gato en horizontal / vertical |
| `enable_hand_mapping` | 0/1 | 1 | Mapear teclas a mano izquierda/derecha |
| `keypress_duration` | ms | 100 | Cuánto se mantiene el fotograma de tecla pulsada |
| `idle_frame` | 0–4 | 0 | Fotograma en reposo |
| `idle_sleep_timeout` | segundos | 0 | Dormir tras inactividad (0 = desactivado) |
| `hotplug_scan_interval` | segundos | 30 | Cada cuánto rescanear dispositivos (0 = una vez) |
| `enable_scheduled_sleep` | 0/1 | 0 | Activar reposo por horario |
| `sleep_begin` / `sleep_end` | HH:MM | 00:00 | Inicio / fin del horario de reposo |
| `disable_fullscreen_hide` | 0/1 | 0 | Mantener el overlay visible en pantalla completa |
| `enable_debug` | 0/1 | 0 | Registro de depuración |

Cambiar el número de monitores en marcha requiere reiniciar; el resto se recarga
en caliente con `--watch-config`.

</details>

## Línea de comandos

```
bongocat [OPCIONES]

  -c, --config FICHERO   Ruta del fichero de configuración (auto-detecta si se omite)
  -m, --monitor NOMBRE   Forzar una salida de monitor concreta
  -w, --watch-config     Recargar al cambiar la configuración
  -t, --toggle           Arrancar / parar (toggle)
  -h, --help             Ayuda
  -v, --version          Versión
```

> ⚠️ **Aviso de privacidad**: con `enable_debug=1` la versión actual registra
> pulsaciones. Mantenlo en `0` (por defecto) para uso normal. Este volcado de
> teclas está en camino de eliminarse: bongocat reducirá cada tecla a
> izquierda/derecha y no podrá saber qué tecla pulsaste.

## Solución de problemas

<details>
<summary>Permiso denegado en el dispositivo de entrada</summary>

```bash
sudo usermod -a -G input $USER   # y vuelve a iniciar sesión
```

</details>

<details>
<summary>El gato no responde al teclado</summary>

1. `bongocat-find-devices` para encontrar el dispositivo correcto
2. Actualiza `keyboard_device` en la configuración
3. Reinicia bongocat

</details>

<details>
<summary>No aparece en el monitor correcto</summary>

Pon `monitor=TU_MONITOR` (uno) o `monitor=MON1,MON2` (varios). Los nombres se
ven con `wlr-randr` o `hyprctl monitors`.

</details>

## Compilar

```bash
make          # Build de release
make debug    # Build de depuración (ASan + UBSan)
make test     # Ejecuta los tests
```

## Estado

Este fork está reescribiendo el núcleo a **Rust** y añadiendo: instalación en un
comando, icono en la barra del sistema, mover el gato con el ratón, temas/skins
(incl. compatibilidad con [wayland-vpets](https://github.com/furudbat/wayland-vpets)),
interfaz de configuración, y endurecimiento de seguridad y privacidad.

## Licencia

MIT — ver [LICENSE](LICENSE).
