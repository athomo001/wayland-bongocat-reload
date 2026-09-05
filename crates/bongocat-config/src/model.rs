//! Modelo de la ventana: la `Config` que se está editando, qué claves se han
//! tocado sin guardar, y de dónde salió (instancia viva o fichero).
//!
//! Toda la lógica sin `egui` vive aquí para poder testearla (spec 0007,
//! `T-0007-M3-model`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use bongocat_common::config::{parse_ini, ConfDoc, Config};
use bongocat_common::field_meta;
use bongocat_common::{io, ipc};

/// De dónde se cargó la configuración que se está viendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// De una instancia de bongocat en marcha (vía IPC `DUMP`). Los cambios se
    /// aplican en vivo y "Guardar" hace `SAVE`.
    Instance,
    /// Del fichero `bongocat.conf` (no hay instancia). "Guardar" reescribe el
    /// fichero conservando comentarios.
    File,
    /// No había ni instancia ni fichero: valores por defecto. "Guardar" crea el
    /// fichero.
    Defaults,
}

impl Source {
    /// ¿Hay una instancia viva detrás?
    #[must_use]
    pub fn connected(self) -> bool {
        self == Source::Instance
    }
}

/// Estado editable de la ventana.
pub struct Model {
    /// Configuración que se está editando (siempre validada/recortada).
    pub cfg: Config,
    /// Claves cambiadas en la ventana y aún sin "Guardar".
    pub dirty: BTreeSet<String>,
    /// Origen de `cfg`.
    pub source: Source,
    /// Instancia elegida (`--monitor`); `None` = la por defecto.
    pub instance: Option<String>,
    /// Ruta del `.conf` (para "Guardar" sin instancia). `None` si no se resolvió.
    pub path: Option<PathBuf>,
    /// Aviso de la última acción (rango recortado, error de E/S…).
    pub status: String,
}

impl Model {
    /// Carga desde la instancia `instance` (o la por defecto) si responde;
    /// si no, del fichero; si no, valores por defecto.
    #[must_use]
    pub fn load(instance: Option<&str>) -> Self {
        let instance = instance.map(str::to_owned);

        if let Some((cfg, status)) = load_from_instance(instance.as_deref()) {
            return Self {
                cfg,
                dirty: BTreeSet::new(),
                source: Source::Instance,
                instance,
                path: io::resolve_config_path_real(),
                status,
            };
        }
        match io::load(None) {
            Ok(l) => Self {
                cfg: l.config,
                dirty: BTreeSet::new(),
                source: if l.path.is_some() {
                    Source::File
                } else {
                    Source::Defaults
                },
                instance,
                path: l.path.or_else(io::resolve_config_path_real),
                status: l.warnings.join("; "),
            },
            Err(e) => Self {
                cfg: Config::default(),
                dirty: BTreeSet::new(),
                source: Source::Defaults,
                instance,
                path: io::resolve_config_path_real(),
                status: format!("no se pudo leer la config: {e}"),
            },
        }
    }

    /// ¿Hay cambios sin guardar?
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Valor efectivo de `key` en la config actual, en la grafía del `.conf`.
    /// `None` si la clave no la emite `to_ini` (p. ej. una lista vacía).
    #[must_use]
    pub fn value(&self, key: &str) -> Option<String> {
        ConfDoc::parse(&self.cfg.to_ini())
            .get(key)
            .map(str::to_owned)
    }

    /// Aplica `key = value`: valida (tipo + rango de `field_meta`), actualiza la
    /// `Config` en memoria y, si hay instancia, manda `SET` en vivo. Marca la
    /// clave como sucia.
    ///
    /// # Errores
    /// `Err(msg)` en español si el valor no es válido; en ese caso no cambia nada.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        field_meta::validate_value(key, value)?;

        // La config viva del proceso es la referencia: `set_live` valida el tipo
        // y recorta a rango igual que al cargar del fichero.
        let warnings = bongocat_common::config::set_live(&mut self.cfg, key, value)
            .map_err(|e| e.to_string())?;

        self.dirty.insert(key.to_owned());
        self.status = warnings.join("; ");

        if self.source.connected() {
            match ipc::send_request(self.instance.as_deref(), &format!("SET {key} {value}")) {
                Ok(reply) if reply.starts_with("ERR") => {
                    self.status = format!("la instancia rechazó {key}: {reply}");
                }
                Ok(_) => {}
                Err(e) => {
                    // Se perdió la instancia a mitad de sesión: seguimos en modo
                    // fichero para no tirar los cambios.
                    self.source = Source::File;
                    self.status =
                        format!("se perdió la instancia ({e}); ahora se guardará al fichero");
                }
            }
        }
        Ok(())
    }

    /// Descarta los cambios sin guardar: con instancia → le pide `RELOAD`
    /// (relee su `.conf`, deshaciendo los `SET`) y vuelve a cargar; sin
    /// instancia → recarga del fichero.
    pub fn reset(&mut self) {
        if self.source.connected() {
            match ipc::send_request(self.instance.as_deref(), "RELOAD") {
                Ok(r) if r.starts_with("ERR") => {
                    self.status = "no hay un bongocat.conf que releer".to_owned();
                    return;
                }
                Err(e) => {
                    self.status = format!("no respondió la instancia: {e}");
                    return;
                }
                Ok(_) => {}
            }
        }
        let fresh = Model::load(self.instance.as_deref());
        self.cfg = fresh.cfg;
        self.source = fresh.source;
        self.path = fresh.path;
        self.dirty.clear();
        self.status = "restablecido".to_owned();
    }

    /// Persiste los cambios: con instancia → `SAVE`; sin instancia → reescribe el
    /// `.conf` **conservando comentarios** (`ConfDoc`), o lo crea si no existía.
    ///
    /// # Errores
    /// `Err(msg)` si falla el IPC o la escritura del fichero.
    pub fn save(&mut self) -> Result<(), String> {
        if self.dirty.is_empty() {
            return Ok(());
        }

        if self.source.connected() {
            let reply = ipc::send_request(self.instance.as_deref(), "SAVE")
                .map_err(|e| format!("no respondió la instancia: {e}"))?;
            if reply.starts_with("ERR") {
                return Err(format!("la instancia no pudo guardar: {reply}"));
            }
            self.dirty.clear();
            self.status = "guardado en la instancia".to_owned();
            return Ok(());
        }

        let path = self
            .path
            .clone()
            .or_else(io::resolve_config_path_real)
            .ok_or("no sé dónde está el bongocat.conf")?;

        let base = std::fs::read_to_string(&path).unwrap_or_default();
        let mut doc = ConfDoc::parse(&base);
        for key in &self.dirty {
            if let Some(v) = self.value(key) {
                doc.set(key, &v);
            }
        }
        io::save_atomic(&path, &doc.render())
            .map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))?;

        self.path = Some(path);
        self.dirty.clear();
        self.status = "guardado en el fichero".to_owned();
        Ok(())
    }
}

/// Pide `DUMP` a la instancia y parsea la respuesta. `None` si no hay instancia
/// o la respuesta no es una config.
fn load_from_instance(instance: Option<&str>) -> Option<(Config, String)> {
    let dump = ipc::send_request_full(instance, "DUMP").ok()?;
    let trimmed = dump.trim();
    if trimmed.is_empty() || trimmed.starts_with("ERR") {
        return None;
    }
    let (cfg, warnings) = parse_ini(&dump);
    Some((cfg, warnings.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_de_defaults() -> Model {
        Model {
            cfg: Config::default(),
            dirty: BTreeSet::new(),
            source: Source::Defaults,
            instance: None,
            path: None,
            status: String::new(),
        }
    }

    #[test]
    fn source_connected() {
        assert!(Source::Instance.connected());
        assert!(!Source::File.connected());
        assert!(!Source::Defaults.connected());
    }

    #[test]
    fn load_con_instancia_inexistente_no_queda_conectado() {
        let m = Model::load(Some("instancia-que-no-existe-xyzzy"));
        assert_ne!(m.source, Source::Instance);
        assert!(!m.is_dirty());
    }

    #[test]
    fn set_valido_actualiza_cfg_y_marca_sucio() {
        let mut m = model_de_defaults();
        assert!(m.set("cat_height", "120").is_ok());
        assert_eq!(m.cfg.cat_height, 120);
        assert_eq!(m.value("cat_height").as_deref(), Some("120"));
        assert!(m.dirty.contains("cat_height"));
    }

    #[test]
    fn set_fuera_de_rango_no_cambia_nada() {
        let mut m = model_de_defaults();
        let antes = m.cfg.cat_height;
        let e = m.set("cat_height", "9999").unwrap_err();
        assert!(e.contains("200"), "{e}");
        assert_eq!(m.cfg.cat_height, antes);
        assert!(m.dirty.is_empty());
    }

    #[test]
    fn set_tipo_invalido_falla() {
        let mut m = model_de_defaults();
        assert!(m.set("cat_align", "arriba").is_err());
        assert!(m.set("mirror_x", "quizas").is_err());
        assert!(m.dirty.is_empty());
    }

    #[test]
    fn save_sin_cambios_es_noop() {
        let mut m = model_de_defaults();
        assert!(m.save().is_ok());
    }

    #[test]
    fn save_a_fichero_conserva_comentarios() {
        let dir = std::env::temp_dir().join(format!("bcfg-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bongocat.conf");
        std::fs::write(&path, "# mi gato\ncat_height = 40  # tamaño\nfps = 60\n").unwrap();

        let mut m = model_de_defaults();
        m.path = Some(path.clone());
        m.source = Source::File;
        m.set("cat_height", "120").unwrap();
        m.save().unwrap();

        let out = std::fs::read_to_string(&path).unwrap();
        assert!(
            out.contains("# mi gato"),
            "conserva el comentario de cabecera:\n{out}"
        );
        assert!(
            out.contains("cat_height=120") || out.contains("cat_height = 120"),
            "{out}"
        );
        assert!(out.contains("fps = 60"), "no toca las demás líneas:\n{out}");
        assert!(!m.is_dirty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
