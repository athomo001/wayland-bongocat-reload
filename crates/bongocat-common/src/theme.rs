//! Metadatos de un **tema** (skin): `theme.ini` + 5 SVG con nombres fijos
//! (spec 0006). El parseo del INI y la relación de aspecto viven aquí (puro y
//! testeable); la resolución de rutas y la lectura de ficheros están en el
//! binario `bongocat` (`src/theme.rs`), que necesita el sistema de ficheros.

use crate::config::split_line;

/// Versión del **formato** de tema que entiende este bongocat. Un tema con
/// `theme_format` mayor se rechaza (fallback al embebido).
///
/// - `1`: 5 SVG con nombres fijos (`classic`).
/// - `2`: reservado.
/// - `3`: sprite sheet PNG en rejilla, estilo wayland-vpets (spec 0014).
pub const THEME_FORMAT_SUPPORTED: u32 = 3;

/// Relación de aspecto de referencia por defecto (la del `classic`).
pub const DEFAULT_ASPECT: (u32, u32) = (500, 277);

/// Nombres de fichero por defecto de los 5 frames, en el orden
/// `FRAME_BOTH_UP..FRAME_SLEEPING` de `paw.rs`.
pub const DEFAULT_FRAME_FILES: [&str; 5] = [
    "both-up.svg",
    "left-down.svg",
    "right-down.svg",
    "both-down.svg",
    "sleeping.svg",
];

/// Metadatos de un tema, ya parseados. No incluye los bytes de los SVG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeMeta {
    pub name: String,
    pub author: String,
    pub license: String,
    pub theme_format: u32,
    pub theme_version: u32,
    /// `(ancho, alto)` de referencia; `cat_width = cat_height * w / h`.
    pub aspect: (u32, u32),
    pub default_cat_height: Option<u32>,
    pub default_cat_align: Option<crate::config::Align>,
    pub default_cat_x_offset: Option<i32>,
    pub default_cat_y_offset: Option<i32>,
    pub can_roam: bool,
    pub roam_speed: Option<u32>,
    /// Nombre de fichero de cada frame (override con `frame_* =` en el INI).
    pub frame_files: [String; 5],
}

impl Default for ThemeMeta {
    fn default() -> Self {
        Self {
            name: String::new(),
            author: String::new(),
            license: String::new(),
            theme_format: 1,
            theme_version: 1,
            aspect: DEFAULT_ASPECT,
            default_cat_height: None,
            default_cat_align: None,
            default_cat_x_offset: None,
            default_cat_y_offset: None,
            can_roam: false,
            roam_speed: None,
            frame_files: DEFAULT_FRAME_FILES.map(String::from),
        }
    }
}

/// Parsea `"W:H"` (o `"W x H"`). Devuelve `None` si no es un par de enteros > 0.
#[must_use]
pub fn parse_aspect(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once([':', 'x', 'X', '/'])?;
    let w: u32 = a.trim().parse().ok()?;
    let h: u32 = b.trim().parse().ok()?;
    if w > 0 && h > 0 {
        Some((w, h))
    } else {
        None
    }
}

/// Parsea el texto de un `theme.ini`. Claves desconocidas se ignoran; si falta
/// todo, se devuelven los valores por defecto (formato 1, aspecto 500:277,
/// nombres de frame estándar). Nunca falla: un `theme.ini` ausente equivale a
/// `parse_theme_ini("")`.
#[must_use]
pub fn parse_theme_ini(text: &str) -> ThemeMeta {
    let mut m = ThemeMeta::default();
    for raw in text.lines() {
        let t = raw.trim_start_matches([' ', '\t']);
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let Some(l) = split_line(raw) else { continue };
        let v = l.value;
        match l.key.as_str() {
            "name" => m.name = v,
            "author" => m.author = v,
            "license" => m.license = v,
            "theme_format" => {
                if let Ok(n) = v.parse() {
                    m.theme_format = n;
                }
            }
            "theme_version" => {
                if let Ok(n) = v.parse() {
                    m.theme_version = n;
                }
            }
            "aspect_ratio" => {
                if let Some(a) = parse_aspect(&v) {
                    m.aspect = a;
                }
            }
            "cat_height" | "default_cat_height" => m.default_cat_height = v.parse().ok(),
            "cat_align" => {
                m.default_cat_align = match v.to_ascii_lowercase().as_str() {
                    "left" => Some(crate::config::Align::Left),
                    "right" => Some(crate::config::Align::Right),
                    "center" => Some(crate::config::Align::Center),
                    _ => None,
                }
            }
            "cat_x_offset" => m.default_cat_x_offset = v.parse().ok(),
            "cat_y_offset" => m.default_cat_y_offset = v.parse().ok(),
            "can_roam" | "enable_roam" => {
                m.can_roam = matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
            }
            "roam_speed" => m.roam_speed = v.parse().ok(),
            "frame_both_up" => m.frame_files[0] = v,
            "frame_left_down" => m.frame_files[1] = v,
            "frame_right_down" => m.frame_files[2] = v,
            "frame_both_down" => m.frame_files[3] = v,
            "frame_sleeping" => m.frame_files[4] = v,
            _ => {}
        }
    }
    m
}

/// ¿Soporta este bongocat el formato de este tema?
#[must_use]
pub fn format_supported(m: &ThemeMeta) -> bool {
    m.theme_format <= THEME_FORMAT_SUPPORTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_vacio_da_valores_por_defecto() {
        let m = parse_theme_ini("");
        assert_eq!(m.theme_format, 1);
        assert_eq!(m.aspect, (500, 277));
        assert_eq!(m.frame_files[0], "both-up.svg");
        assert!(format_supported(&m));
    }

    #[test]
    fn parsea_metadatos_y_overrides() {
        let ini = "\
# comentario
name = Neon Cat
author = fulano
theme_format = 1
aspect_ratio = 400:200
default_cat_height = 120
frame_sleeping = zzz.svg
";
        let m = parse_theme_ini(ini);
        assert_eq!(m.name, "Neon Cat");
        assert_eq!(m.aspect, (400, 200));
        assert_eq!(m.default_cat_height, Some(120));
        assert_eq!(m.frame_files[4], "zzz.svg");
        assert_eq!(
            m.frame_files[1], "left-down.svg",
            "los no-override no cambian"
        );
    }

    #[test]
    fn formato_futuro_no_soportado() {
        let m = parse_theme_ini("theme_format = 99");
        assert!(!format_supported(&m));
    }

    #[test]
    fn aspect_varias_formas() {
        assert_eq!(parse_aspect("500:277"), Some((500, 277)));
        assert_eq!(parse_aspect(" 16 x 9 "), Some((16, 9)));
        assert_eq!(parse_aspect("0:1"), None);
        assert_eq!(parse_aspect("abc"), None);
    }
}
