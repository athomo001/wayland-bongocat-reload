//! Puente entre el parser puro y el sistema de ficheros: resolución de rutas
//! XDG, lectura del `wayvpet.conf` y dispositivo de teclado por defecto.
//!
//! Es la única parte de `wayvpet-common` que toca disco. Portado de
//! `config_resolve_path` / `config_parse_file` / `config_set_default_devices`
//! (`src/config/config.c`). El escaneo de `/dev/input` por nombre vive en el
//! binario `wayvpet` (necesita `evdev`).

use crate::config::{parse_ini_sections, Config, MonitorSection};
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
    /// Secciones `[monitor:NOMBRE]` del fichero (spec 0008 §8.4), sin aplicar.
    /// La instancia que conoce su `--monitor` las aplica con
    /// [`crate::config::apply_monitor_section`].
    pub monitor_sections: Vec<MonitorSection>,
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

    let defaults = || Loaded {
        config: Config::default(),
        warnings: Vec::new(),
        path: None,
        monitor_sections: Vec::new(),
    };

    let Some(path) = path else {
        return Ok(defaults());
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        // Ruta explícita ausente sí es error; el C también falla ahí.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
            return Ok(defaults());
        }
        Err(e) => return Err(e),
    };

    let (mut config, monitor_sections, warnings) = parse_ini_sections(&text);
    apply_default_keyboard_device(&mut config);
    Ok(Loaded {
        config,
        warnings,
        path: Some(path),
        monitor_sections,
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

/// Ruta del fichero de estado del aviso de nueva versión (spec 0015):
/// `$XDG_STATE_HOME/wayvpet/update-check.json`, o
/// `$HOME/.local/state/wayvpet/update-check.json`. `env` se inyecta en tests.
/// El helper `wayvpet-update-check` lo escribe; `wayvpet` (disparo, M3) y
/// `wayvpetctl update` (M6) lo leen.
pub fn update_state_path<E>(env: E) -> Option<PathBuf>
where
    E: Fn(&str) -> Option<String>,
{
    if let Some(xdg) = env("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        return Some(Path::new(&xdg).join("wayvpet/update-check.json"));
    }
    let home = env("HOME").filter(|s| !s.is_empty())?;
    Some(Path::new(&home).join(".local/state/wayvpet/update-check.json"))
}

/// [`update_state_path`] con el entorno real.
#[must_use]
pub fn update_state_path_real() -> Option<PathBuf> {
    update_state_path(|k| std::env::var(k).ok())
}

/// Directorios de datos XDG en orden de prioridad: `$XDG_DATA_HOME` (o
/// `~/.local/share`), luego cada entrada de `$XDG_DATA_DIRS` (por defecto
/// `/usr/local/share:/usr/share`). Mismo criterio que la búsqueda de temas.
fn xdg_data_dirs<E>(env: E) -> Vec<PathBuf>
where
    E: Fn(&str) -> Option<String>,
{
    let mut dirs = Vec::new();
    if let Some(h) = env("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(h));
    } else if let Some(home) = env("HOME").filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    let list = env("XDG_DATA_DIRS")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    for d in list.split(':').filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

/// Marca del canal de instalación (spec 0015 §"Marca del canal"): el contenido
/// de `<datadir>/wayvpet/install-channel` (`source` / `deb` / `rpm` / `arch` /
/// `distro`), o `None` si no hay fichero. Con `distro`, el aviso de nueva
/// versión se calla: actualizar es cosa del gestor de paquetes.
pub fn install_channel<E>(env: E) -> Option<String>
where
    E: Fn(&str) -> Option<String>,
{
    for dir in xdg_data_dirs(&env) {
        let p = dir.join("wayvpet/install-channel");
        if let Ok(s) = std::fs::read_to_string(&p) {
            let word = s.trim();
            if !word.is_empty() {
                return Some(word.to_string());
            }
        }
    }
    None
}

/// [`install_channel`] con el entorno real.
#[must_use]
pub fn install_channel_real() -> Option<String> {
    install_channel(|k| std::env::var(k).ok())
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

    #[test]
    fn update_state_path_prefiere_xdg_state_home() {
        let env = |k: &str| match k {
            "XDG_STATE_HOME" => Some("/st".to_string()),
            "HOME" => Some("/home/u".to_string()),
            _ => None,
        };
        assert_eq!(
            update_state_path(env),
            Some(PathBuf::from("/st/wayvpet/update-check.json"))
        );
        let env = |k: &str| (k == "HOME").then(|| "/home/u".to_string());
        assert_eq!(
            update_state_path(env),
            Some(PathBuf::from(
                "/home/u/.local/state/wayvpet/update-check.json"
            ))
        );
        assert_eq!(update_state_path(no_env), None);
    }

    #[test]
    fn install_channel_lee_la_primera_coincidencia() {
        let dir = std::env::temp_dir().join(format!("wayvpet-ch-{}", std::process::id()));
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(a.join("wayvpet")).unwrap();
        std::fs::create_dir_all(b.join("wayvpet")).unwrap();
        std::fs::write(b.join("wayvpet/install-channel"), "deb\n").unwrap();

        // `a` (XDG_DATA_HOME) no tiene el fichero → cae a `b` (XDG_DATA_DIRS).
        let env = |k: &str| match k {
            "XDG_DATA_HOME" => Some(a.to_string_lossy().into_owned()),
            "XDG_DATA_DIRS" => Some(b.to_string_lossy().into_owned()),
            _ => None,
        };
        assert_eq!(install_channel(env), Some("deb".to_string()));

        // Sin fichero en ningún lado → None.
        let env = |k: &str| (k == "XDG_DATA_HOME").then(|| a.to_string_lossy().into_owned());
        assert_eq!(install_channel(env), None);

        std::fs::remove_dir_all(&dir).ok();
    }
}
