//! Puente entre el parser puro y el sistema de ficheros: resolución de rutas
//! XDG, lectura del `wayvpet.conf` y dispositivo de teclado por defecto.
//!
//! Es la única parte de `wayvpet-common` que toca disco. Portado de
//! `config_resolve_path` / `config_parse_file` / `config_set_default_devices`
//! (`src/config/config.c`). El escaneo de `/dev/input` por nombre vive en el
//! binario `wayvpet` (necesita `evdev`).

use crate::config::{parse_ini, Config};
use std::path::{Path, PathBuf};

/// Dispositivo de teclado que usa el C cuando no hay ninguno configurado.
pub const DEFAULT_KEYBOARD_DEVICE: &str = "/dev/input/event4";

/// Resultado de cargar la configuración: la `Config` validada y los avisos
/// (líneas ignoradas, valores ajustados…).
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<String>,
    /// Ruta de la que se leyó, o `None` si se usaron solo los valores por defecto.
    pub path: Option<PathBuf>,
}

/// Busca el `wayvpet.conf` en el orden del C:
/// 1. `$XDG_CONFIG_HOME/wayvpet/wayvpet.conf`
/// 2. `$HOME/.config/wayvpet/wayvpet.conf`
/// 3. `./wayvpet.conf`
///
/// `env` es la función de acceso a variables de entorno (inyectable en tests).
/// `exists` comprueba si una ruta es legible (inyectable en tests).
pub fn resolve_config_path<E, X>(env: E, exists: X) -> Option<PathBuf>
where
    E: Fn(&str) -> Option<String>,
    X: Fn(&Path) -> bool,
{
    if let Some(xdg) = env("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        let p = Path::new(&xdg).join("wayvpet/wayvpet.conf");
        if exists(&p) {
            return Some(p);
        }
    }
    if let Some(home) = env("HOME").filter(|s| !s.is_empty()) {
        let p = Path::new(&home).join(".config/wayvpet/wayvpet.conf");
        if exists(&p) {
            return Some(p);
        }
    }
    let cwd = PathBuf::from("wayvpet.conf");
    if exists(&cwd) {
        return Some(cwd);
    }
    None
}

/// Igual que [`resolve_config_path`] pero usando el entorno y el disco reales.
#[must_use]
pub fn resolve_config_path_real() -> Option<PathBuf> {
    resolve_config_path(|k| std::env::var(k).ok(), |p| std::fs::metadata(p).is_ok())
}

/// Carga la configuración: si `explicit` es `Some`, lee esa ruta; si no, resuelve
/// por XDG; si no hay fichero, devuelve los valores por defecto. Un fichero
/// ilegible es un error; un fichero ausente **no** lo es (como el C).
pub fn load(explicit: Option<&Path>) -> std::io::Result<Loaded> {
    let path = match explicit {
        Some(p) => Some(p.to_path_buf()),
        None => resolve_config_path_real(),
    };

    let Some(path) = path else {
        return Ok(Loaded {
            config: Config::default(),
            warnings: Vec::new(),
            path: None,
        });
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        // Ruta explícita ausente sí es error; el C también falla ahí.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
            return Ok(Loaded {
                config: Config::default(),
                warnings: Vec::new(),
                path: None,
            });
        }
        Err(e) => return Err(e),
    };

    let (mut config, warnings) = parse_ini(&text);
    apply_default_keyboard_device(&mut config);
    Ok(Loaded {
        config,
        warnings,
        path: Some(path),
    })
}

/// Escribe `contents` en `path` de forma **atómica**: fichero temporal en el
/// mismo directorio + `fsync` + `rename` (spec 0004). Preserva los permisos del
/// fichero previo si existía. Un fallo no deja `path` a medias.
///
/// # Errores
/// Errores de E/S al crear el temporal, escribir, sincronizar o renombrar.
pub fn save_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("wayvpet.conf");
    let tmp = dir.join(format!(".{name}.tmp.{}", std::process::id()));

    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Si no se configuró ningún teclado (ni por ruta ni por nombre), añade
/// `/dev/input/event4` como último recurso, igual que `config_set_default_devices`.
pub fn apply_default_keyboard_device(config: &mut Config) {
    if config.keyboard_devices.is_empty() && config.keyboard_names.is_empty() {
        config
            .keyboard_devices
            .push(DEFAULT_KEYBOARD_DEVICE.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn resuelve_xdg_config_home_primero() {
        let env = |k: &str| match k {
            "XDG_CONFIG_HOME" => Some("/xdg".to_string()),
            "HOME" => Some("/home/u".to_string()),
            _ => None,
        };
        let exists = |p: &Path| p == Path::new("/xdg/wayvpet/wayvpet.conf");
        assert_eq!(
            resolve_config_path(env, exists),
            Some(PathBuf::from("/xdg/wayvpet/wayvpet.conf"))
        );
    }

    #[test]
    fn cae_a_home_config_si_no_hay_xdg() {
        let env = |k: &str| (k == "HOME").then(|| "/home/u".to_string());
        let exists = |p: &Path| p == Path::new("/home/u/.config/wayvpet/wayvpet.conf");
        assert_eq!(
            resolve_config_path(env, exists),
            Some(PathBuf::from("/home/u/.config/wayvpet/wayvpet.conf"))
        );
    }

    #[test]
    fn sin_nada_devuelve_none() {
        assert_eq!(resolve_config_path(no_env, |_| false), None);
    }

    #[test]
    fn dispositivo_por_defecto_solo_si_no_hay_ninguno() {
        let mut c = Config::default();
        apply_default_keyboard_device(&mut c);
        assert_eq!(c.keyboard_devices, [DEFAULT_KEYBOARD_DEVICE]);

        let mut c = Config::default();
        c.keyboard_names.push("mi teclado".into());
        apply_default_keyboard_device(&mut c);
        assert!(
            c.keyboard_devices.is_empty(),
            "hay keyboard_name, no se añade"
        );
    }

    #[test]
    fn load_de_ruta_explicita_ausente_es_error() {
        let missing = Path::new("/no/existe/wayvpet.conf");
        assert!(load(Some(missing)).is_err());
    }

    #[test]
    fn load_lee_un_fichero_real() {
        let dir = std::env::temp_dir().join(format!("wayvpet-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("wayvpet.conf");
        std::fs::write(&p, "fps=30\ncat_height=90\n").unwrap();

        let loaded = load(Some(&p)).unwrap();
        assert_eq!(loaded.config.fps, 30);
        assert_eq!(loaded.config.cat_height, 90);
        assert_eq!(loaded.config.keyboard_devices, [DEFAULT_KEYBOARD_DEVICE]);
        assert_eq!(loaded.path.as_deref(), Some(p.as_path()));

        std::fs::remove_dir_all(&dir).ok();
    }
}
