//! "Modo experto": editar los ficheros `.ini` a mano (`bongocat.conf`, y el
//! `theme.ini` / `vpet.ini` del tema activo). Guardar escribe el texto tal cual
//! y le pide a la instancia que recargue.

use std::path::{Path, PathBuf};

use bongocat_common::{io, ipc};

use crate::model::Model;

/// Un fichero abierto en el editor crudo.
pub struct RawFile {
    /// Nombre corto para la pestaña/cabecera.
    pub label: String,
    pub path: PathBuf,
    /// Texto editable.
    pub text: String,
    /// Contenido en disco la última vez que se leyó (para saber si hay cambios).
    disk: String,
    /// Es un fichero del tema (`theme.ini` / `vpet.ini`): al guardarlo hay que
    /// recargar el tema, no solo la config.
    is_theme: bool,
    /// Aviso de la última acción.
    pub status: String,
}

impl RawFile {
    fn load(label: impl Into<String>, path: PathBuf, is_theme: bool) -> Option<Self> {
        let text = std::fs::read_to_string(&path).ok()?;
        Some(Self {
            label: label.into(),
            path,
            disk: text.clone(),
            text,
            is_theme,
            status: String::new(),
        })
    }

    /// ¿El buffer difiere de lo que hay en disco?
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.text != self.disk
    }

    /// Relee el fichero de disco, descartando los cambios del editor.
    pub fn reload(&mut self) {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => {
                self.text = t.clone();
                self.disk = t;
                self.status = "releído del disco".into();
            }
            Err(e) => self.status = format!("no se pudo leer: {e}"),
        }
    }

    /// Escribe el texto **tal cual** (atómico) y, si hay instancia, le pide que
    /// recargue (`RELOAD`; además `THEME` si es un fichero del tema).
    pub fn save(&mut self, instance: Option<&str>, theme_name: &str) {
        // Un parseo de aviso: si es el `bongocat.conf`, marca las líneas que el
        // parser no entendería (no impide guardar: es modo experto).
        if !self.is_theme {
            let (_, warns) = bongocat_common::config::parse_ini(&self.text);
            if !warns.is_empty() {
                self.status = format!("guardado, pero: {}", warns.join("; "));
            }
        }
        if let Err(e) = io::save_atomic(&self.path, &self.text) {
            self.status = format!("no se pudo escribir: {e}");
            return;
        }
        self.disk = self.text.clone();
        if self.status.is_empty() {
            self.status = "guardado".into();
        }

        // Que la instancia lo recoja.
        if instance_alive(instance) {
            let _ = ipc::send_request(instance, "RELOAD");
            if self.is_theme && !theme_name.is_empty() {
                let _ = ipc::send_request(instance, &format!("THEME {theme_name}"));
            }
        }
    }
}

fn instance_alive(instance: Option<&str>) -> bool {
    matches!(ipc::send_request(instance, "PING").as_deref(), Ok(r) if r.trim() == "PONG")
}

/// Ficheros editables: siempre `bongocat.conf`; y si el tema activo resuelve a
/// una carpeta, su `theme.ini` y `vpet.ini`.
#[must_use]
pub fn gather(model: &Model) -> Vec<RawFile> {
    let mut out = Vec::new();

    if let Some(p) = model.path.clone().or_else(io::resolve_config_path_real) {
        if let Some(f) = RawFile::load("bongocat.conf", p, false) {
            out.push(f);
        }
    }

    let theme = model.cfg.theme.trim();
    if !theme.is_empty() {
        if let Some(dir) = theme_dir(theme) {
            for name in ["theme.ini", "vpet.ini"] {
                if let Some(f) = RawFile::load(name, dir.join(name), true) {
                    out.push(f);
                }
            }
        }
    }
    out
}

/// Localiza la carpeta de un tema por nombre (o la usa tal cual si es una ruta).
/// Mismas rutas que `themes/README.md`.
fn theme_dir(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return p.is_dir().then_some(p);
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        roots.push(PathBuf::from(h));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(Path::new(&home).join(".local/share"));
    }
    if let Some(dirs) = std::env::var_os("XDG_DATA_DIRS").filter(|s| !s.is_empty()) {
        roots.extend(std::env::split_paths(&dirs));
    } else {
        roots.push(PathBuf::from("/usr/local/share"));
        roots.push(PathBuf::from("/usr/share"));
    }
    roots.push(PathBuf::from("themes")); // desarrollo, desde la raíz del repo

    roots
        .into_iter()
        .map(|r| {
            if r.ends_with("themes") {
                r.join(name)
            } else {
                r.join("bongocat/themes").join(name)
            }
        })
        .find(|p| p.is_dir())
}
