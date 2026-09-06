//! Presets (spec 0008 §8.2): un preset es un `.conf` **parcial** — solo las
//! claves que redefine. Aplicarlo = leer sus pares y hacer `SET` de cada uno
//! sobre la config activa (merge por encima). El demonio valida cada clave con
//! `set_live`; aquí solo se **leen** los pares.
//!
//! Rutas (por prioridad): `$XDG_DATA_HOME/wayvpet/presets/`,
//! `$XDG_DATA_DIRS/*/wayvpet/presets/`, `/usr/local|/usr` share, y `./presets/`
//! (desarrollo desde la raíz del repo).

use std::path::PathBuf;

use super::line::split_line;

/// Directorios donde buscar presets, en orden de prioridad.
#[must_use]
pub fn roots() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        out.push(PathBuf::from(h).join("wayvpet/presets"));
    } else if let Some(home) = std::env::var_os("HOME") {
        out.push(PathBuf::from(home).join(".local/share/wayvpet/presets"));
    }
    if let Some(dirs) = std::env::var_os("XDG_DATA_DIRS").filter(|s| !s.is_empty()) {
        out.extend(std::env::split_paths(&dirs).map(|d| d.join("wayvpet/presets")));
    } else {
        out.push(PathBuf::from("/usr/local/share/wayvpet/presets"));
        out.push(PathBuf::from("/usr/share/wayvpet/presets"));
    }
    out.push(PathBuf::from("presets"));
    out
}

/// Nombres de presets disponibles (`.conf` en cualquier raíz), ordenados y sin
/// repetir. El de una raíz de más prioridad tapa al de una de menos.
#[must_use]
pub fn list() -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for root in roots() {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("conf") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    seen.insert(stem.to_owned());
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// Localiza el fichero de un preset por nombre (primera raíz que lo tenga).
#[must_use]
pub fn path(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '.']) {
        return None; // sin rutas ni `..`
    }
    roots()
        .into_iter()
        .map(|r| r.join(format!("{name}.conf")))
        .find(|p| p.is_file())
}

/// Pares `clave=valor` del preset `name`, en orden de aparición. Comentarios y
/// líneas en blanco se ignoran. **No** valida las claves — eso es del que aplica.
///
/// # Errores
/// Si el preset no existe o no se puede leer.
pub fn load(name: &str) -> std::io::Result<Vec<(String, String)>> {
    let p = path(name).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no hay ningún preset '{name}'"),
        )
    })?;
    let text = std::fs::read_to_string(p)?;
    Ok(pairs_from_ini(&text))
}

/// Extrae los pares `clave=valor` de un texto INI parcial.
#[must_use]
pub fn pairs_from_ini(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim_start)
        .filter(|l| !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(split_line)
        .filter(|l| !l.key.is_empty())
        .map(|l| (l.key, l.value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_ignora_comentarios_y_blancos() {
        let src = "\
# stealth: casi invisible
cat_opacity = 8   # semitransparente
layer=overlay

overlay_opacity = 0
";
        let got = pairs_from_ini(src);
        assert_eq!(
            got,
            vec![
                ("cat_opacity".to_owned(), "8".to_owned()),
                ("layer".to_owned(), "overlay".to_owned()),
                ("overlay_opacity".to_owned(), "0".to_owned()),
            ]
        );
    }

    #[test]
    fn path_rechaza_nombres_con_ruta() {
        assert!(path("../x").is_none());
        assert!(path("a/b").is_none());
        assert!(path("").is_none());
    }
}
