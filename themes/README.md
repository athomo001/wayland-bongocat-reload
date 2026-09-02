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

Si algo falla al cargar (falta un SVG, no parsea, formato futuro…), bongocat
**avisa y sigue con el gato embebido** — nunca se queda sin gato.
