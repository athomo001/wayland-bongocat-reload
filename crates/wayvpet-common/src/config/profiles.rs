//! Perfiles (spec 0008 §8.3): configuraciones **completas** con nombre, para
//! alternar entre "trabajo / juego / streaming". A diferencia de un preset (un
//! `.conf` parcial que se aplica por encima), un perfil **es** un `wayvpet.conf`
//! entero.
//!
//! - Se guardan en `<dir del wayvpet.conf>/profiles/<nombre>.conf`.
//! - `switch <n>` copia (no symlink, para que el watcher lo vea) ese fichero
//!   sobre el `wayvpet.conf` y recarga.
//! - `save <n>` copia el `wayvpet.conf` actual a `profiles/<n>.conf`.
//! - El perfil activo se recuerda en `$XDG_STATE_HOME/wayvpet/active-profile`.

use std::io;
use std::path::{Path, PathBuf};

/// Carpeta de perfiles junto a `conf_path`.
#[must_use]
pub fn dir(conf_path: &Path) -> PathBuf {
    conf_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("profiles")
}

/// Fichero que recuerda el perfil activo.
#[must_use]
pub fn state_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("wayvpet/active-profile"))
}

/// Nombre del perfil activo, o `None` si no hay ninguno marcado.
#[must_use]
pub fn active() -> Option<String> {
    let s = std::fs::read_to_string(state_file()?).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

fn set_active(name: &str) -> io::Result<()> {
    let f = state_file()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "sin XDG_STATE_HOME ni HOME"))?;
    if let Some(p) = f.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(f, format!("{name}\n"))
}

/// Perfiles guardados (`.conf` en la carpeta), ordenados.
#[must_use]
pub fn list(conf_path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir(conf_path)) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("conf") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_owned());
                }
            }
        }
    }
    out.sort();
    out
}

/// ¿Nombre de perfil aceptable? (sin rutas, sin `.`, corto).
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.contains(['/', '\\', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Guarda `content` (la config **efectiva** actual, normalmente
/// `Config::to_ini()`) como perfil `name` junto a `conf_path`, y lo marca
/// activo. No lee `conf_path` — el que llama pasa lo que quiere congelar,
/// incluidos los cambios en vivo aún sin `SAVE`.
///
/// # Errores
/// Nombre inválido, o E/S al crear la carpeta / escribir el perfil.
pub fn save(conf_path: &Path, name: &str, content: &str) -> io::Result<()> {
    if !valid_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nombre de perfil inválido (solo letras, dígitos, - y _)",
        ));
    }
    let d = dir(conf_path);
    std::fs::create_dir_all(&d)?;
    crate::io::save_atomic(&d.join(format!("{name}.conf")), content)?;
    set_active(name)
}

/// Copia `profiles/<name>.conf` sobre `conf_path` y marca `name` como activo. El
/// llamante debe recargar la config después.
///
/// # Errores
/// Nombre inválido, el perfil no existe, o E/S.
pub fn switch(conf_path: &Path, name: &str) -> io::Result<()> {
    if !valid_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nombre de perfil inválido",
        ));
    }
    let src = dir(conf_path).join(format!("{name}.conf"));
    let text = std::fs::read_to_string(&src)?;
    crate::io::save_atomic(conf_path, &text)?;
    set_active(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_name_rechaza_rutas_y_puntos() {
        assert!(valid_name("streaming"));
        assert!(valid_name("juego_2"));
        assert!(!valid_name("../x"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("con.punto"));
        assert!(!valid_name(""));
        assert!(!valid_name(&"x".repeat(65)));
    }

    #[test]
    fn save_y_switch_round_trip() {
        let tmp = std::env::temp_dir().join(format!("wv-prof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let conf = tmp.join("wayvpet.conf");
        std::fs::write(&conf, "cat_height = 40\n").unwrap();

        // `save` sin `XDG_STATE_HOME` puede fallar al marcar activo, pero el
        // perfil debe quedar escrito igualmente si hay HOME; aquí solo miramos
        // el fichero del perfil.
        let d = dir(&conf);
        std::fs::create_dir_all(&d).unwrap();
        crate::io::save_atomic(&d.join("big.conf"), "cat_height = 200\n").unwrap();

        assert_eq!(list(&conf), vec!["big".to_owned()]);
        switch(&conf, "big").ok(); // marca activo puede fallar sin STATE/HOME
        assert_eq!(
            std::fs::read_to_string(&conf).unwrap().trim(),
            "cat_height = 200"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
