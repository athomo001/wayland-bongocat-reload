//! Aviso pasivo de nueva versión de wayvpet (spec 0015).
//!
//! Este crate NO habla con la red todavía: aquí solo vive
//!
//! - el modelo de `$XDG_STATE_HOME/wayvpet/update-check.json` ([`UpdateState`]),
//! - su lectura acotada desde disco ([`read_state`]),
//! - y la regla "¿la versión remota es más nueva que la instalada?"
//!   ([`is_newer`]).
//!
//! El proceso hijo corto que consulta a `api.github.com` y escribe el fichero
//! (hito M2) será el binario `wayvpet-update-check`, detrás de la feature `net`,
//! para que `ureq`+`rustls` NO se enlacen jamás en el supervisor ni en
//! `wayvpetctl` (spec 0015 §"Marca del canal" / §"Riesgos": peso).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "net")]
pub mod net;

/// Tope de tamaño del `update-check.json` al leerlo. Lo escribe el propio hijo y
/// nunca pasa de unos cientos de bytes; un fichero mayor es corrupción o
/// manipulación y se rechaza **antes** de parsear (spec 0015 §Seguridad:
/// "tamaños acotados").
pub const MAX_STATE_BYTES: usize = 64 * 1024;

/// Un artefacto publicado en el release. La descarga (hito M5) elige uno según
/// la marca `install-channel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// Nombre del fichero (`wayvpet_0.6.0_amd64.deb`, …).
    pub name: String,
    /// URL de descarga directa (HTTPS de `github.com` /
    /// `objects.githubusercontent.com`).
    pub url: String,
    /// SHA-256 en hexadecimal, sacado del `SHA256SUMS` del release. `None` si el
    /// release no lo publicó: sin él la descarga no puede verificarse y M5 la
    /// rechaza en vez de "confiar y seguir".
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Contenido de `$XDG_STATE_HOME/wayvpet/update-check.json`. Lo escribe el hijo
/// de red (M2); lo leen el tray (M4) y `wayvpetctl update` (M6).
///
/// Todos los campos son opcionales a propósito: un chequeo que falla (sin red,
/// rate-limit de GitHub, JSON raro) escribe solo `checked_at` + `error` y
/// termina con código 0 — un problema de red **nunca** molesta al usuario.
/// Los campos desconocidos se ignoran (serde por defecto), así una versión
/// futura del hijo puede añadir claves sin romper a un lector viejo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateState {
    /// Instante del último chequeo, ISO-8601 UTC (`2026-09-06T12:00:00Z`).
    /// Vacío solo cuando el fichero aún no existe (nunca se comprobó).
    pub checked_at: String,
    /// Versión instalada que vio el hijo (`CARGO_PKG_VERSION`), sin la `v`.
    pub installed: Option<String>,
    /// `tag_name` del último release estable, sin la `v`.
    pub latest: Option<String>,
    /// URL de la página del release (para "Ver en el navegador").
    pub url: Option<String>,
    /// Notas del release en texto plano, ya recortadas por el hijo.
    pub notes: Option<String>,
    /// Artefactos del release.
    pub assets: Vec<Asset>,
    /// Mensaje si el último chequeo falló. Cuando está presente, los campos de
    /// versión se ignoran (no hay aviso).
    pub error: Option<String>,
}

impl UpdateState {
    /// `true` si hay una versión estable **estrictamente mayor** que la
    /// instalada y el último chequeo no falló. `installed_fallback` es el
    /// `CARGO_PKG_VERSION` del lector, por si el JSON no trae `installed`.
    #[must_use]
    pub fn update_available(&self, installed_fallback: &str) -> bool {
        if self.error.is_some() {
            return false;
        }
        let Some(latest) = self.latest.as_deref() else {
            return false;
        };
        let installed = self.installed.as_deref().unwrap_or(installed_fallback);
        is_newer(latest, installed)
    }

    /// Línea para `wayvpetctl update` (spec 0015 §"Superficie de CLI"). No toca
    /// la red: solo interpreta el fichero de estado.
    #[must_use]
    pub fn status_line(&self, installed_fallback: &str) -> String {
        if self.checked_at.is_empty() {
            return "sin datos (todavía no se ha comprobado)".to_string();
        }
        if let Some(err) = &self.error {
            return format!("sin datos (el último intento falló: {err})");
        }
        let installed = self.installed.as_deref().unwrap_or(installed_fallback);
        match &self.latest {
            Some(latest) if is_newer(latest, installed) => match self.url.as_deref() {
                Some(url) if !url.is_empty() => format!("v{latest} disponible — {url}"),
                _ => format!("v{latest} disponible"),
            },
            _ => "al día".to_string(),
        }
    }
}

/// Error al leer o parsear el fichero de estado.
#[derive(Debug)]
pub enum StateError {
    /// El fichero supera [`MAX_STATE_BYTES`]; se rechaza sin parsear.
    TooLarge {
        /// Tamaño real encontrado, en bytes.
        size: usize,
    },
    /// El contenido no es un JSON válido para [`UpdateState`].
    Parse(serde_json::Error),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::TooLarge { size } => write!(
                f,
                "update-check.json demasiado grande: {size} B (tope {MAX_STATE_BYTES} B)"
            ),
            StateError::Parse(e) => write!(f, "update-check.json ilegible: {e}"),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StateError::Parse(e) => Some(e),
            StateError::TooLarge { .. } => None,
        }
    }
}

/// Parsea el contenido ya leído a memoria de un `update-check.json`. Rechaza por
/// tamaño **antes** de mirar el JSON.
///
/// # Errores
/// [`StateError::TooLarge`] si `bytes` pasa de [`MAX_STATE_BYTES`];
/// [`StateError::Parse`] si el JSON no cuadra.
pub fn parse_state(bytes: &[u8]) -> Result<UpdateState, StateError> {
    if bytes.len() > MAX_STATE_BYTES {
        return Err(StateError::TooLarge { size: bytes.len() });
    }
    serde_json::from_slice(bytes).map_err(StateError::Parse)
}

/// Serializa el estado como lo escribe el hijo: JSON compacto y un `\n` final.
#[must_use]
pub fn to_json(state: &UpdateState) -> String {
    let mut s = serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string());
    s.push('\n');
    s
}

/// Escribe el estado en `path` de forma **atómica**: temporal en el mismo
/// directorio + `fsync` + `rename` (spec 0015 §Seguridad: "se escribe atómico").
/// Un fallo a media escritura no deja `path` corrupto; el lector siempre ve o el
/// contenido viejo entero o el nuevo entero. Crea el directorio padre si falta.
///
/// # Errores
/// E/S al crear el directorio, el temporal, escribir, sincronizar o renombrar.
pub fn write_state_atomic(path: &Path, state: &UpdateState) -> io::Result<()> {
    use std::io::Write;

    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("update-check.json");
    let tmp = dir.join(format!(".{name}.tmp.{}", std::process::id()));

    let body = to_json(state);
    let write = || -> io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Ruta del fichero de estado: `$XDG_STATE_HOME/wayvpet/update-check.json`, o
/// `$HOME/.local/state/wayvpet/update-check.json` si no hay `XDG_STATE_HOME`.
/// `env` se inyecta en tests.
pub fn state_path(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(xdg) = env("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        return Some(Path::new(&xdg).join("wayvpet/update-check.json"));
    }
    let home = env("HOME").filter(|s| !s.is_empty())?;
    Some(Path::new(&home).join(".local/state/wayvpet/update-check.json"))
}

/// [`state_path`] con el entorno real.
#[must_use]
pub fn state_path_real() -> Option<PathBuf> {
    state_path(|k| std::env::var(k).ok())
}

/// Lee y parsea el fichero de estado.
///
/// - `Ok(None)`: el fichero no existe todavía (nunca se ha comprobado).
/// - `Ok(Some(_))`: estado leído y validado.
/// - `Err(_)`: ilegible, mayor que [`MAX_STATE_BYTES`], o JSON inválido — el
///   llamante lo trata como "sin datos" (nunca aborta por esto).
///
/// # Errores
/// E/S al abrir/leer, o [`StateError`] envuelto en
/// [`io::ErrorKind::InvalidData`].
pub fn read_state(path: &Path) -> io::Result<Option<UpdateState>> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    parse_state(&data)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// `true` si `latest` es una versión semver **estrictamente mayor** que
/// `installed`.
///
/// Reglas (spec 0015: "solo la última release **estable**"):
/// - Se tolera el prefijo `v`/`V` y los espacios.
/// - Un `latest` con parte de pre-release (`0.6.0-rc1`) **no** cuenta: los
///   canales beta/nightly quedan fuera. `installed` sí puede ser pre-release
///   (quien corre una rc verá la estable como más nueva).
/// - Entradas que no son semver → `false`: un tag raro nunca dispara el aviso.
#[must_use]
pub fn is_newer(latest: &str, installed: &str) -> bool {
    let (Some(latest), Some(installed)) = (parse_version(latest), parse_version(installed)) else {
        return false;
    };
    if !latest.pre.is_empty() {
        return false;
    }
    latest > installed
}

/// Parsea `vX.Y.Z` / `X.Y.Z` a [`semver::Version`]; `None` si no cuadra.
fn parse_version(s: &str) -> Option<semver::Version> {
    let s = s.trim();
    let s = s
        .strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s);
    semver::Version::parse(s).ok()
}

#[cfg(test)]
mod tests;
