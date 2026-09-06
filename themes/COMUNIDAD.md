# Mascotas de la comunidad (`theme_format = 3`)

wayvpet lee de forma nativa el formato de sprite sheet de
[**wayland-vpets**](https://github.com/furudbat/wayland-vpets) (furudbat, MIT) —
un fork hermano que generaliza el gato a cientos de mascotas animadas. Con
`wayvpet theme import-vpets <origen>` puedes reutilizarlas.

> **Este repositorio NO incluye arte de terceros.** Solo se empaquetan
> `classic`, `pink` (recoloreado propio) y `demo` (arte propio CC0). Todo lo de
> abajo lo instalas **tú** en tu equipo; wayvpet solo copia desde la ruta local
> que le indicas a `~/.local/share/wayvpet/themes/`.

## Aviso de propiedad intelectual

Muchos packs de wayland-vpets contienen personajes que son **marca y propiedad de
sus dueños**: Pokémon (Nintendo / Game Freak / Creatures), Digimon (Bandai),
Clippy / MS Agent (Microsoft), Neko, etc. Importarlos para uso personal en tu
escritorio es cosa tuya y bajo tu responsabilidad; **no los redistribuyas** ni
subas el tema resultante a ningún sitio público. El `theme.ini` que genera el
importador lleva una `license` de recordatorio: edítala si conoces la real.

## Packs conocidos

| Pack | Contenido | Licencia del **código/formato** | Notas |
|------|-----------|--------------------------------|-------|
| [wayland-vpets](https://github.com/furudbat/wayland-vpets) | Motor + configs de mascotas | MIT (código) | El arte de cada mascota tiene su propia procedencia; revísala. |
| [oneko / neko](https://github.com/tie/oneko) y derivados | El gato/perro clásico de X11 | dominio público / MIT según fork | Arte libre; apto para bundlear si se verifica el fork. |

*(Amplía esta tabla con enlaces y licencias verificadas conforme la comunidad
publique packs con arte libre.)*

## Cómo importar

```sh
# Una carpeta de mascota (sprite sheet + su .conf):
wayvpet theme import-vpets ~/pets/charizard --name charizard

# Solo ver el informe, sin escribir:
wayvpet theme import-vpets ~/pets/charizard --dry-run

# Una hoja PNG suelta:
wayvpet theme import-vpets sheet.png --frame-w 64 --frame-h 64 --name mascota

# Una carpeta de PNGs por estado (idle_0.png, writing_0.png, …):
wayvpet theme import-vpets ~/frames --name mascota

# Un APNG = un estado:
wayvpet theme import-vpets fly.apng --state writing --name dragon
```

El importador:

- traduce `custom_sprite_sheet_filename`, `custom_<estado>_row/_frames`,
  `fps` / `animation_speed` y `row_base` (0/1) al formato nativo;
- mapea los 15 estados de wayland-vpets a los que wayvpet sabe conducir;
  los que no (`working` / `moving`) se ignoran con aviso;
- para un estado canónico ausente usa su **reserva** (`idle`→`writing`,
  `sleep`→`boring`→`idle`, …) y lo dice en el informe;
- detecta el modelo (`hands` si hay poses izquierda/derecha, si no `activity`);
- valida el resultado con `theme check` automáticamente.

## GIF

Todavía no. El importador acepta APNG; para GIF hace falta añadir una crate de
decodificación (pendiente de decisión de suministro). Convierte el GIF a APNG o a
una hoja PNG mientras tanto.
