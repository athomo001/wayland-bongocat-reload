# Temas (skins) de Bongo Cat

Un tema es **una carpeta** con 5 SVG y un `theme.ini`. No hace falta recompilar:
suéltala en una ruta de temas y actívala con `theme = <nombre>` en el
`bongocat.conf` (o `bongocatctl theme set <nombre>`).

## Rutas de búsqueda (por prioridad)

1. `$XDG_DATA_HOME/bongocat/themes/` (por defecto `~/.local/share/bongocat/themes/`)
2. `$XDG_DATA_DIRS/*/bongocat/themes/` (típico: `/usr/local/share`, `/usr/share`)
3. `./themes/` del repositorio (solo en desarrollo)

`theme = /ruta/absoluta/a/mi-tema` también vale.

## Estructura

```
mi-tema/
  theme.ini
  both-up.svg      left-down.svg   right-down.svg
  both-down.svg    sleeping.svg
```

`theme.ini` (mismo formato INI que `bongocat.conf`):

```ini
name = Mi Skin
author = tú <correo>
license = CC-BY-4.0
theme_format = 1              # formato que entiende bongocat (no lo cambies)
theme_version = 1             # versión de tu contenido; súbela al editar
# opcionales:
aspect_ratio = 500:277        # ancho:alto de referencia (por defecto 500:277)
default_cat_height = 110      # sugerencia para el primer uso
# si tu editor exporta con otros nombres, apúntalos aquí en vez de renombrar:
frame_both_up    = up.svg
frame_left_down  = L.svg
frame_right_down = R.svg
frame_both_down  = down.svg
frame_sleeping   = zzz.svg
```

Si falta `theme.ini` pero están los 5 SVG con los nombres por defecto, el tema
carga igual con metadatos vacíos y `theme_format = 1`.

## Contrato de dibujo

- **5 fotogramas**: `both-up` (reposo), `left-down`, `right-down`, `both-down`,
  `sleeping`.
- Lienzo con **fondo transparente** y la misma relación de aspecto en los 5
  (`500:277` por defecto; si usas otra, decláralo en `aspect_ratio`). bongocat
  escala manteniendo esa relación; la altura la pone el usuario (`cat_height`).
- El personaje debe **ocupar el lienzo entero**, sin márgenes: bongocat rasteriza
  el SVG tal cual (a diferencia de `classic`, que aún lleva el recorte del
  editor).
- **Solo trazados/formas vectoriales.** Sin `<text>` (no se rasteriza texto) y
  sin `<image>` externas. Cada SVG, ≤ 2 MiB.

## Empezar uno

Copia `classic/` (la plantilla de referencia) y edita los SVG con Inkscape o Boxy
SVG. Ejemplo ya incluido: `pink/` = `classic` recoloreado.

## `theme_format = 3` — sprite sheet (mascotas animadas)

Formato de rejilla estilo [wayland-vpets](https://github.com/furudbat/wayland-vpets)
(spec `specs/0014-*`): un PNG (o **APNG**) con las poses en filas
(`state_<estado>_row` / `_frames` / `_fps` / `_col`), `frame_w` × `frame_h` por
celda, escalado nearest-neighbor a escala entera (`scale_filter = linear` para
arte no pixel-art). Estados animados con máquina de estados completa
(`idle` / `writing` / `sleep` / `happy` / `boring` + one-shots
`start_writing` / `end_writing` / `wake_up`) y modelo `hands` (poses
izquierda/derecha, autodetectado) vs `activity` (cualquier tecla → `writing`).

- Una hoja global (`sheet = x.png`) y/o una por estado (`sheet_writing = w.apng`).
- `happy_kpm` en el `bongocat.conf` activa el estado `happy` al superar N
  teclas/minuto (si el tema lo trae).
- `anchor = baseline` (default) | `center` (mascotas voladoras).

Ejemplo: `demo/` (`sheet.png` + `writing.apng`, arte propio CC0, regenerable con
`cargo run -p bongocat --example gen_demo_sheet`).

### Importar una mascota de wayland-vpets

```
bongocat theme import-vpets <ORIGEN> [--name N] [--dry-run]
```

`<ORIGEN>` puede ser una carpeta de mascota (`.conf` + hoja), un `.conf` suelto,
una hoja PNG (`--frame-w`/`--frame-h`), una carpeta de PNGs `<estado>_<n>.png`, o
un APNG (`--state`). Traduce las claves `custom_*`, escribe el tema en
`~/.local/share/bongocat/themes/<N>/` y lo valida con `theme check`. Nunca falla
por un estado ausente: usa su reserva y lo informa. Ver
[`COMUNIDAD.md`](COMUNIDAD.md) para packs y **licencias**.

Si algo falla al cargar (falta un SVG, no parsea, formato futuro…), bongocat
**avisa y sigue con el gato embebido** — nunca se queda sin gato.
