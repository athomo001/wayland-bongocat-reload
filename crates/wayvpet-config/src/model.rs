//! Modelo de la ventana: la `Config` que se está editando, qué claves se han
//! tocado sin guardar, y de dónde salió (instancia viva o fichero).
//!
//! Toda la lógica sin `egui` vive aquí para poder testearla (spec 0007,
//! `T-0007-M3-model`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use wayvpet_common::config::{parse_ini, ConfDoc, Config};
use wayvpet_common::field_meta;
use wayvpet_common::{io, ipc};

/// De dónde se cargó la configuración que se está viendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// De una instancia de wayvpet en marcha (vía IPC `DUMP`). Los cambios se
    /// aplican en vivo y "Guardar" hace `SAVE`.
    Instance,
    /// Del fichero `wayvpet.conf` (no hay instancia). "Guardar" reescribe el
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
    /// El vpet activo se pasea solo por la pantalla (`can_roam` del `vpet.ini`).
    /// La ventana esconde los campos de posición: se recoloca arrastrando
    /// (bandeja → "Modo edición"). Solo se sabe con instancia viva.
    pub roaming: bool,
    /// Temas instalados (`THEME list` de la instancia): `["embedded", …]`.
    /// Vacío si no hay instancia.
    pub themes: Vec<String>,
    /// Tamaño lógico de la salida (para el "mapa de pantalla"). Sin instancia,
    /// un valor por defecto razonable.
    pub screen: (i32, i32),
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
                roaming: instance_roaming(instance.as_deref()),
                themes: instance_themes(instance.as_deref()),
                screen: instance_screen(instance.as_deref()),
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
                roaming: false,
                themes: Vec::new(),
                screen: (1920, 1080),
                instance,
                path: l.path.or_else(io::resolve_config_path_real),
                status: l.warnings.join("; "),
            },
            Err(e) => Self {
                cfg: Config::default(),
                dirty: BTreeSet::new(),
                source: Source::Defaults,
                roaming: false,
                themes: Vec::new(),
                screen: (1920, 1080),
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
        let warnings = wayvpet_common::config::set_live(&mut self.cfg, key, value)
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
                    self.status = "no hay un wayvpet.conf que releer".to_owned();
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
        self.roaming = fresh.roaming;
        self.themes = fresh.themes;
        self.screen = fresh.screen;
        self.path = fresh.path;
        self.dirty.clear();
        self.status = "restablecido".to_owned();
    }

    /// Nombre del tema activo tal como se muestra en la galería (`"embedded"` si
    /// `theme` está vacío).
    #[must_use]
    pub fn active_theme(&self) -> &str {
        let t = self.cfg.theme.trim();
        if t.is_empty() {
            "embedded"
        } else {
            t
        }
    }

    /// Cambia el tema activo. `"embedded"` → el vpet embebido (`theme` vacío).
    /// Con instancia lo aplica en vivo (`THEME <n>`); marca `theme` como sucia.
    pub fn set_theme(&mut self, name: &str) {
        let stored = if name == "embedded" { "" } else { name };
        if self.source.connected() {
            match ipc::send_request(self.instance.as_deref(), &format!("THEME {name}")) {
                Ok(r) if r.starts_with("ERR") => {
                    self.status = format!("la instancia no cambió de tema: {r}");
                    return;
                }
                Err(e) => {
                    self.status = format!("no respondió la instancia: {e}");
                    return;
                }
                Ok(_) => {}
            }
            // La instancia ya aplicó y ajustó su `vpet.ini`; recarga para que la
            // ventana (roaming, altura efectiva…) quede al día.
            self.roaming = instance_roaming(self.instance.as_deref());
        }
        self.cfg.theme = stored.to_owned();
        self.dirty.insert("theme".to_owned());
        self.status = format!("tema: {name}");
    }

    /// Vuelve a los **valores de fábrica** (los del `wayvpet.conf.example`
    /// shipped, no lo guardado) todos los campos numéricos / booleanos / enum /
    /// hora — **no** toca el tema, los monitores ni las rutas de dispositivo.
    /// Queda como cambios sin guardar (hay que pulsar "Guardar" para que
    /// persista).
    pub fn factory_reset(&mut self) {
        use wayvpet_common::field_meta::{FieldKind, FIELDS};

        let def = wayvpet_common::config::factory_config();
        let def_doc = ConfDoc::parse(&def.to_ini());
        let cur_doc = ConfDoc::parse(&self.cfg.to_ini());
        let mut n = 0;

        for f in FIELDS {
            if matches!(f.kind, FieldKind::Text | FieldKind::List) {
                continue; // tema, monitores, dispositivos: se dejan como están
            }
            let (Some(want), have) = (def_doc.get(f.key), cur_doc.get(f.key).unwrap_or("")) else {
                continue;
            };
            if want == have {
                continue;
            }
            // Aplica en la config local y, si hay instancia, en vivo.
            let _ = wayvpet_common::config::set_live(&mut self.cfg, f.key, want);
            if self.source.connected() {
                let _ =
                    ipc::send_request(self.instance.as_deref(), &format!("SET {} {want}", f.key));
            }
            self.dirty.insert(f.key.to_owned());
            n += 1;
        }
        self.status = if n == 0 {
            "ya estaban los valores de fábrica".to_owned()
        } else {
            format!("{n} campos a valores de fábrica — pulsa Guardar para que quede")
        };
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
            .ok_or("no sé dónde está el wayvpet.conf")?;

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

/// Lee `STATE` de la instancia y saca `roaming=1`. `false` si no responde.
fn instance_roaming(instance: Option<&str>) -> bool {
    state_kv(instance).get("roaming").is_some_and(|v| v == "1")
}

/// Tamaño **lógico** de la salida donde dibuja la instancia (`STATE`
/// `width`/`height`). `(1920, 1080)` si no responde o no lo trae.
fn instance_screen(instance: Option<&str>) -> (i32, i32) {
    let s = state_kv(instance);
    let w = s.get("width").and_then(|v| v.parse().ok()).unwrap_or(1920);
    let h = s.get("height").and_then(|v| v.parse().ok()).unwrap_or(1080);
    (w, h)
}

/// `STATE` → mapa `clave→valor`. Vacío si la instancia no responde.
fn state_kv(instance: Option<&str>) -> std::collections::HashMap<String, String> {
    ipc::send_request(instance, "STATE")
        .ok()
        .map(|s| {
            s.split_whitespace()
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// Lista de temas instalados (`THEME list` → `"embedded classic …"`). Vacío si
/// no responde.
fn instance_themes(instance: Option<&str>) -> Vec<String> {
    match ipc::send_request(instance, "THEME list") {
        Ok(r) if !r.starts_with("ERR") => r.split_whitespace().map(str::to_owned).collect(),
        _ => Vec::new(),
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
            roaming: false,
            themes: Vec::new(),
            screen: (1920, 1080),
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
        assert!(e.contains("512"), "{e}");
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
        let path = dir.join("wayvpet.conf");
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
