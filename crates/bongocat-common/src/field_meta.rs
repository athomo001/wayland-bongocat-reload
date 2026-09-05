//! Metadatos de cada campo de configuración: la tabla declarativa que alimenta
//! los widgets de `bongocat-config` (spec 0007) y la validación previa de
//! `bongocatctl set` / IPC `SET`.
//!
//! **Fuente única de claves:** [`crate::config::KEYS`]. El test guardián de este
//! módulo comprueba la biyección (`FIELDS` ↔ `KEYS`), que cada rango numérico
//! coincide con el `clamp` real de `config::validate`, y que las opciones de
//! cada enum coinciden con lo que acepta `config::check_kv`. Así no puede haber
//! deriva entre el parser y la interfaz.

use crate::config::{self, ALIGN_VALUES, LAYER_VALUES, MOUSE_PAW_VALUES, POSITION_VALUES};

/// Sección en la que la ventana de configuración agrupa el campo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Posición y tamaño del vpet.
    Position,
    /// Apariencia (opacidad, espejo, capa…).
    Appearance,
    /// Entrada de teclado y ratón.
    Input,
    /// Reposo / dormir.
    Sleep,
    /// Tema (skin) activo.
    Theme,
    /// Ajustes que la mayoría no toca (o legado sin efecto). La ventana los
    /// esconde tras un desplegable "avanzado".
    Advanced,
}

impl Section {
    /// Etiqueta visible de la sección.
    #[must_use]
    pub fn label_es(self) -> &'static str {
        match self {
            Section::Position => "Posición y tamaño",
            Section::Appearance => "Apariencia",
            Section::Input => "Entrada",
            Section::Sleep => "Reposo",
            Section::Theme => "Tema",
            Section::Advanced => "Avanzado",
        }
    }
}

/// Tipo del campo → qué widget lo edita y cómo se valida.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Entero con deslizador en `[min, max]`, paso `step`. `unit` se muestra
    /// junto al número (`""` = sin unidad). El valor del `.conf` es el mismo que
    /// el mostrado.
    Int {
        min: i32,
        max: i32,
        step: i32,
        unit: &'static str,
    },
    /// Entero que **se guarda** en `[store_min, store_max]` pero **se edita** en
    /// `[ui_min, ui_max] unit` (caso `overlay_opacity`: guarda 0–255, se enseña
    /// 0–100 %). La conversión la hace la ventana; la validación previa usa el
    /// rango de guardado.
    IntScaled {
        store_min: i32,
        store_max: i32,
        ui_min: i32,
        ui_max: i32,
        step: i32,
        unit: &'static str,
    },
    /// Booleano 0/1 → casilla.
    Bool,
    /// Una de estas opciones (grafía del `.conf`) → desplegable.
    Enum(&'static [&'static str]),
    /// Hora `HH:MM`.
    Time,
    /// Texto libre (el nombre o ruta del tema).
    Text,
    /// Lista separada por comas o clave repetible (monitores, nombres, rutas de
    /// dispositivo).
    List,
}

impl FieldKind {
    /// Rango en el que el valor se **guarda** en el `.conf`, si el campo es
    /// numérico y acotado. `None` para booleanos, enums, texto, listas y para
    /// los enteros sin `clamp` en `config::validate` (`cat_x_offset` /
    /// `cat_y_offset`: el rango de `Int` es solo el del deslizador).
    #[must_use]
    pub fn store_range(self) -> Option<(i32, i32)> {
        match self {
            FieldKind::IntScaled {
                store_min,
                store_max,
                ..
            } => Some((store_min, store_max)),
            _ => None,
        }
    }
}

/// Una fila de la tabla: todo lo que la ventana y la validación necesitan saber
/// de un campo.
#[derive(Debug, Clone, Copy)]
pub struct FieldMeta {
    /// Clave del `.conf` (igual que `config::KEYS`).
    pub key: &'static str,
    /// Etiqueta visible.
    pub label_es: &'static str,
    /// Sección de la ventana.
    pub section: Section,
    /// Tipo → widget + validación.
    pub kind: FieldKind,
    /// Ayuda de una línea (tooltip).
    pub help_es: &'static str,
}

/// Azúcar para declarar filas `Int` sin unidad.
const fn int(
    key: &'static str,
    label_es: &'static str,
    section: Section,
    min: i32,
    max: i32,
    step: i32,
    help_es: &'static str,
) -> FieldMeta {
    FieldMeta {
        key,
        label_es,
        section,
        kind: FieldKind::Int {
            min,
            max,
            step,
            unit: "",
        },
        help_es,
    }
}

/// Azúcar para declarar filas booleanas.
const fn boolean(
    key: &'static str,
    label_es: &'static str,
    section: Section,
    help_es: &'static str,
) -> FieldMeta {
    FieldMeta {
        key,
        label_es,
        section,
        kind: FieldKind::Bool,
        help_es,
    }
}

/// La tabla. Una fila por cada clave de [`config::KEYS`].
pub const FIELDS: &[FieldMeta] = &[
    // ── Posición y tamaño ──────────────────────────────────────────────────
    FieldMeta {
        key: "cat_align",
        label_es: "Alineación horizontal",
        section: Section::Position,
        kind: FieldKind::Enum(ALIGN_VALUES),
        help_es: "Desde qué borde se mide el desfase X: izquierda, centro o derecha.",
    },
    int(
        "cat_height",
        "Altura del vpet",
        Section::Position,
        10,
        200,
        2,
        "Alto del personaje en píxeles; el ancho sale de la relación de aspecto del tema.",
    ),
    int(
        "cat_x_offset",
        "Desfase horizontal",
        Section::Position,
        -4000,
        4000,
        1,
        "Corrimiento X respecto a la alineación. Se arrastra también con el modo edición.",
    ),
    int(
        "cat_y_offset",
        "Desfase vertical",
        Section::Position,
        -4000,
        4000,
        1,
        "Corrimiento Y. El overlay ocupa toda la pantalla, así que llega a cualquier altura.",
    ),
    FieldMeta {
        key: "monitor",
        label_es: "Monitores",
        section: Section::Position,
        kind: FieldKind::List,
        help_es: "Nombres de salida (p. ej. eDP-1, HDMI-A-1) donde mostrar el vpet. Vacío = donde elija el compositor.",
    },
    // ── Apariencia ─────────────────────────────────────────────────────────
    FieldMeta {
        key: "cat_opacity",
        label_es: "Opacidad del vpet",
        section: Section::Appearance,
        kind: FieldKind::Int {
            min: 0,
            max: 100,
            step: 1,
            unit: "%",
        },
        help_es: "0 = invisible, 100 = opaco. Se aplica al dibujar el personaje.",
    },
    boolean(
        "mirror_x",
        "Espejo horizontal",
        Section::Appearance,
        "Voltea el vpet de izquierda a derecha.",
    ),
    boolean(
        "mirror_y",
        "Espejo vertical",
        Section::Appearance,
        "Voltea el vpet de arriba a abajo.",
    ),
    boolean(
        "enable_antialiasing",
        "Suavizado de bordes",
        Section::Appearance,
        "Antialiasing al rasterizar los temas SVG.",
    ),
    FieldMeta {
        key: "layer",
        label_es: "Capa del overlay",
        section: Section::Appearance,
        kind: FieldKind::Enum(LAYER_VALUES),
        help_es: "Nivel de wlr-layer-shell: sobre qué ventanas queda el vpet.",
    },
    // ── Entrada ────────────────────────────────────────────────────────────
    boolean(
        "enable_mouse",
        "Animar con el ratón",
        Section::Input,
        "El vpet mueve una pata al mover o clicar el ratón físico.",
    ),
    FieldMeta {
        key: "mouse_paw",
        label_es: "Pata del ratón",
        section: Section::Input,
        kind: FieldKind::Enum(MOUSE_PAW_VALUES),
        help_es: "Qué pata responde a la actividad del ratón: izquierda, derecha o al azar.",
    },
    int(
        "mouse_move_interval",
        "Intervalo del ratón",
        Section::Input,
        10,
        2000,
        10,
        "Milisegundos entre golpecitos mientras el ratón se mueve (no uno por evento).",
    ),
    boolean(
        "enable_hand_mapping",
        "Mano según la tecla",
        Section::Input,
        "Elige pata izquierda/derecha según la mitad del teclado (temas 'con manos').",
    ),
    int(
        "keypress_duration",
        "Duración del golpecito",
        Section::Input,
        10,
        5000,
        10,
        "Milisegundos que la pata baja tras cada tecla.",
    ),
    FieldMeta {
        key: "keyboard_device",
        label_es: "Teclados (rutas)",
        section: Section::Advanced,
        kind: FieldKind::List,
        help_es: "Rutas /dev/input/… de teclados. Vacío = autodetección.",
    },
    FieldMeta {
        key: "keyboard_name",
        label_es: "Teclados (nombres)",
        section: Section::Advanced,
        kind: FieldKind::List,
        help_es: "Nombres de teclado a vigilar. Vacío = autodetección.",
    },
    FieldMeta {
        key: "mouse_device",
        label_es: "Ratones (rutas)",
        section: Section::Advanced,
        kind: FieldKind::List,
        help_es: "Rutas /dev/input/… de ratones. Vacío = autodetección.",
    },
    FieldMeta {
        key: "mouse_name",
        label_es: "Ratones (nombres)",
        section: Section::Advanced,
        kind: FieldKind::List,
        help_es: "Nombres de ratón a vigilar. Vacío = autodetección.",
    },
    int(
        "hotplug_scan_interval",
        "Reescaneo de dispositivos",
        Section::Advanced,
        0,
        3600,
        5,
        "Segundos entre reescaneos de /dev/input para detectar teclados/ratones nuevos. 0 = solo al arrancar.",
    ),
    // ── Reposo ─────────────────────────────────────────────────────────────
    int(
        "idle_sleep_timeout",
        "Dormir tras inactividad",
        Section::Sleep,
        0,
        3600,
        5,
        "Segundos sin actividad antes de que el vpet se duerma. 0 = nunca.",
    ),
    boolean(
        "enable_scheduled_sleep",
        "Dormir por horario",
        Section::Sleep,
        "Duerme el vpet dentro de la franja horaria de abajo.",
    ),
    FieldMeta {
        key: "sleep_begin",
        label_es: "Inicio del sueño",
        section: Section::Sleep,
        kind: FieldKind::Time,
        help_es: "Hora HH:MM en que empieza la franja de sueño programado.",
    },
    FieldMeta {
        key: "sleep_end",
        label_es: "Fin del sueño",
        section: Section::Sleep,
        kind: FieldKind::Time,
        help_es: "Hora HH:MM en que termina la franja de sueño programado.",
    },
    int(
        "happy_kpm",
        "Umbral 'contento' (tpm)",
        Section::Sleep,
        0,
        10_000,
        10,
        "Teclas por minuto a partir de las cuales un tema con estado 'happy' lo muestra. 0 = desactivado.",
    ),
    // ── Tema ───────────────────────────────────────────────────────────────
    FieldMeta {
        key: "theme",
        label_es: "Tema (skin)",
        section: Section::Theme,
        kind: FieldKind::Text,
        help_es: "Nombre del tema instalado o ruta a una carpeta. Vacío = el gato embebido (classic).",
    },
    // ── Avanzado / legado ──────────────────────────────────────────────────
    int(
        "overlay_height",
        "Alto del overlay (legado)",
        Section::Advanced,
        20,
        300,
        10,
        "Sin efecto: el overlay pasó a ocupar toda la pantalla. Se conserva por compatibilidad del .conf.",
    ),
    FieldMeta {
        key: "overlay_opacity",
        label_es: "Opacidad del overlay (legado)",
        section: Section::Advanced,
        kind: FieldKind::IntScaled {
            store_min: 0,
            store_max: 255,
            ui_min: 0,
            ui_max: 100,
            step: 1,
            unit: "%",
        },
        help_es: "Sin efecto: el fondo del overlay es siempre transparente. Se guarda 0–255, se muestra 0–100 %.",
    },
    FieldMeta {
        key: "overlay_position",
        label_es: "Borde del overlay (legado)",
        section: Section::Advanced,
        kind: FieldKind::Enum(POSITION_VALUES),
        help_es: "Sin efecto: el overlay ocupa toda la pantalla. Se conserva por compatibilidad.",
    },
    int(
        "fps",
        "FPS (legado)",
        Section::Advanced,
        1,
        120,
        1,
        "Sin efecto: el tick se reprograma a instantes exactos. Se conserva parseado y mostrado.",
    ),
    int(
        "idle_frame",
        "Fotograma en reposo",
        Section::Advanced,
        0,
        4,
        1,
        "Índice del fotograma que se muestra con el vpet quieto (0–4).",
    ),
    int(
        "test_animation_duration",
        "Duración de la animación de prueba",
        Section::Advanced,
        10,
        5000,
        10,
        "Milisegundos que dura cada disparo de la animación de prueba.",
    ),
    int(
        "test_animation_interval",
        "Intervalo de la animación de prueba",
        Section::Advanced,
        0,
        3600,
        1,
        "Segundos entre disparos automáticos de la animación de prueba. 0 = desactivado.",
    ),
    boolean(
        "disable_fullscreen_hide",
        "No ocultar en pantalla completa",
        Section::Advanced,
        "Si se activa, el vpet sigue visible aunque haya una ventana a pantalla completa.",
    ),
    boolean(
        "enable_debug",
        "Registro de depuración",
        Section::Advanced,
        "Logs verbosos. Déjalo desactivado salvo para diagnosticar un problema.",
    ),
    boolean(
        "enable_ipc",
        "Socket de control (IPC)",
        Section::Advanced,
        "Permite que bongocatctl y la ventana hablen con la instancia en marcha. Recomendado dejarlo activo.",
    ),
    boolean(
        "enable_tray",
        "Icono en la bandeja",
        Section::Advanced,
        "Muestra el icono del sistema con el menú (mostrar/ocultar, tema, modo edición…).",
    ),
];

/// Metadatos de una clave, o `None` si no es un campo conocido.
#[must_use]
pub fn field(key: &str) -> Option<&'static FieldMeta> {
    FIELDS.iter().find(|f| f.key == key)
}

/// Validación **previa** para `bongocatctl set` / IPC `SET` / la ventana: primero
/// el tipo (vía [`config::check_kv`]), luego el rango numérico de `field_meta`.
/// Mensajes en español aptos para enseñar al usuario.
///
/// # Errores
/// `Err(msg)` si la clave no existe, el tipo no cuadra, o un entero se sale del
/// rango del campo.
pub fn validate_value(key: &str, value: &str) -> Result<(), String> {
    let Some(meta) = field(key) else {
        // No está en la tabla: que decida el parser (cubre alias como
        // `keyboard_devices` y da el mensaje de "clave desconocida").
        return config::check_kv(key, value);
    };
    config::check_kv(key, value)?;

    let range = match meta.kind {
        FieldKind::Int { min, max, .. } => Some((min, max)),
        FieldKind::IntScaled { .. } => meta.kind.store_range(),
        _ => None,
    };
    if let Some((min, max)) = range {
        let v: i32 = value
            .trim()
            .parse()
            .map_err(|_| format!("'{value}' no es un número entero para {}", meta.label_es))?;
        if v < min || v > max {
            return Err(format!(
                "{} debe estar entre {min} y {max} (diste {v})",
                meta.label_es
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, Config, KEYS};

    /// Un valor de ejemplo **válido** para cada tipo de campo, para poder llamar
    /// a `check_kv` / `set_live` en los cruces.
    fn sample(meta: &FieldMeta) -> String {
        match meta.kind {
            FieldKind::Int { min, max, .. } => {
                // un punto medio del rango (evita min/max por si hay borde)
                (min + (max - min) / 2).to_string()
            }
            FieldKind::IntScaled {
                store_min,
                store_max,
                ..
            } => (store_min + (store_max - store_min) / 2).to_string(),
            FieldKind::Bool => "1".to_string(),
            FieldKind::Enum(opts) => opts[0].to_string(),
            FieldKind::Time => "07:30".to_string(),
            FieldKind::Text => "classic".to_string(),
            FieldKind::List => {
                // `keyboard_device`/`mouse_device` exigen prefijo /dev/input/
                if meta.key.ends_with("_device") {
                    "/dev/input/event0".to_string()
                } else {
                    "eDP-1".to_string()
                }
            }
        }
    }

    #[test]
    fn biyeccion_fields_con_config_keys() {
        let mut fields: Vec<&str> = FIELDS.iter().map(|f| f.key).collect();
        fields.sort_unstable();
        let dups: Vec<_> = fields.windows(2).filter(|w| w[0] == w[1]).collect();
        assert!(dups.is_empty(), "claves duplicadas en FIELDS: {dups:?}");

        let mut keys: Vec<&str> = KEYS.to_vec();
        keys.sort_unstable();

        assert_eq!(
            fields, keys,
            "FIELDS y config::KEYS deben cubrir exactamente las mismas claves"
        );
    }

    #[test]
    fn cada_field_lo_acepta_el_parser() {
        for meta in FIELDS {
            let v = sample(meta);
            assert!(
                config::check_kv(meta.key, &v).is_ok(),
                "check_kv rechazó {}={v} (field_meta dice que es válido)",
                meta.key
            );
        }
    }

    #[test]
    fn rangos_numericos_coinciden_con_el_clamp_del_parser() {
        // Claves cuyo entero NO se acota en `config::validate` (el rango de `Int`
        // es solo el del deslizador de la ventana).
        const SIN_CLAMP: &[&str] = &["cat_x_offset", "cat_y_offset"];

        for meta in FIELDS {
            let (min, max) = match meta.kind {
                FieldKind::Int { min, max, .. } => (min, max),
                FieldKind::IntScaled {
                    store_min,
                    store_max,
                    ..
                } => (store_min, store_max),
                _ => continue,
            };

            let mut c = Config::default();
            let over = set_live_int(&mut c, meta.key, i64::from(max) + 1);
            let under = set_live_int(&mut c, meta.key, i64::from(min) - 1);

            if SIN_CLAMP.contains(&meta.key) {
                assert!(
                    over.is_none() && under.is_none(),
                    "{} está en SIN_CLAMP pero validate lo acotó",
                    meta.key
                );
                continue;
            }

            let (lo_o, hi_o) = over.unwrap_or_else(|| {
                panic!(
                    "{}: field_meta define rango pero validate no acota",
                    meta.key
                )
            });
            assert_eq!(
                (lo_o, hi_o),
                (min, max),
                "{}: rango de field_meta {:?} ≠ clamp de validate {:?}",
                meta.key,
                (min, max),
                (lo_o, hi_o)
            );
            let (lo_u, hi_u) = under.expect("clamp por abajo también avisa");
            assert_eq!(
                (lo_u, hi_u),
                (min, max),
                "{}: rango inconsistente",
                meta.key
            );
        }
    }

    /// Aplica `key = v` a `c` y, si `validate` emitió un aviso de rango
    /// `[lo-hi]`, devuelve `(lo, hi)`. `None` si no hubo recorte.
    fn set_live_int(c: &mut Config, key: &str, v: i64) -> Option<(i32, i32)> {
        let warns = config::set_live(c, key, &v.to_string()).ok()?;
        for w in warns {
            if let Some((lo, hi)) = parse_rango(&w) {
                return Some((lo, hi));
            }
        }
        None
    }

    /// Extrae `(lo, hi)` de un aviso tipo `"… fuera de rango [10-200], …"` o
    /// `"idle_frame 9 fuera de rango [0-4], se pone a 0"`.
    fn parse_rango(w: &str) -> Option<(i32, i32)> {
        let start = w.find('[')?;
        let end = w[start..].find(']')? + start;
        let inner = &w[start + 1..end];
        let (a, b) = inner.split_once('-').or_else(|| {
            // rango con mínimo negativo: "[-100-200]" — parte por el 2º '-'
            let dash = inner[1..].find('-')? + 1;
            Some((&inner[..dash], &inner[dash + 1..]))
        })?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    }

    #[test]
    fn opciones_de_enum_coinciden_con_el_parser() {
        for meta in FIELDS {
            let FieldKind::Enum(opts) = meta.kind else {
                continue;
            };
            assert!(!opts.is_empty(), "{}: enum sin opciones", meta.key);
            for opt in opts {
                assert!(
                    config::check_kv(meta.key, opt).is_ok(),
                    "{}={opt}: field_meta lo lista pero check_kv lo rechaza",
                    meta.key
                );
            }
            assert!(
                config::check_kv(meta.key, "__valor_imposible__").is_err(),
                "{}: check_kv debería rechazar un valor fuera de la lista",
                meta.key
            );
        }
    }

    #[test]
    fn validate_value_rechaza_fuera_de_rango_con_mensaje_util() {
        // dentro de rango
        assert!(validate_value("cat_height", "80").is_ok());
        // fuera de rango: mensaje con la etiqueta y los límites
        let e = validate_value("cat_height", "999").unwrap_err();
        assert!(e.contains("Altura del vpet") && e.contains("200"), "{e}");
        // tipo mal
        assert!(validate_value("cat_height", "alto").is_err());
        // clave inexistente → delega en el parser
        assert!(validate_value("no_existe", "1").is_err());
        // enum válido / inválido
        assert!(validate_value("cat_align", "left").is_ok());
        assert!(validate_value("cat_align", "arriba").is_err());
        // IntScaled valida contra el rango de guardado (0–255), no el 0–100 de UI
        assert!(validate_value("overlay_opacity", "200").is_ok());
        assert!(validate_value("overlay_opacity", "300").is_err());
    }

    #[test]
    fn secciones_tienen_etiqueta() {
        for s in [
            Section::Position,
            Section::Appearance,
            Section::Input,
            Section::Sleep,
            Section::Theme,
            Section::Advanced,
        ] {
            assert!(!s.label_es().is_empty());
        }
    }
}
