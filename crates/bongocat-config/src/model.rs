//! Modelo de la ventana: la `Config` que se está editando, qué claves se han
//! tocado sin guardar, y de dónde salió (instancia viva o fichero).
//!
//! Toda la lógica sin `egui` vive aquí para poder testearla (spec 0007,
//! `T-0007-M3-model`). M2 solo carga; el `set`/`save` por campo llega en M3.

use std::collections::BTreeSet;

use bongocat_common::config::{parse_ini, ConfDoc, Config};
use bongocat_common::{io, ipc};

/// De dónde se cargó la configuración que se está viendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// De una instancia de bongocat en marcha (vía IPC `DUMP`). Los cambios se
    /// aplican en vivo y "Guardar" hace `SAVE`.
    Instance,
    /// Del fichero `bongocat.conf` (no hay instancia). "Guardar" reescribe el
    /// fichero.
    File,
    /// No había ni instancia ni fichero: valores por defecto.
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
    /// Configuración que se está editando.
    pub cfg: Config,
    /// Claves cambiadas en la ventana y aún sin "Guardar".
    pub dirty: BTreeSet<String>,
    /// Origen de `cfg`.
    pub source: Source,
    /// Avisos de la última carga/guardado (rangos recortados, fichero ilegible…).
    pub status: String,
}

impl Model {
    /// Carga desde la instancia `instance` (o la por defecto) si responde;
    /// si no, del fichero; si no, valores por defecto.
    #[must_use]
    pub fn load(instance: Option<&str>) -> Self {
        if let Some((cfg, status)) = load_from_instance(instance) {
            return Self {
                cfg,
                dirty: BTreeSet::new(),
                source: Source::Instance,
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
                status: l.warnings.join("; "),
            },
            Err(e) => Self {
                cfg: Config::default(),
                dirty: BTreeSet::new(),
                source: Source::Defaults,
                status: format!("no se pudo leer la config: {e}"),
            },
        }
    }

    /// ¿Hay cambios sin guardar?
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Valor efectivo de `key` en la config actual, en la grafía del `.conf`
    /// (`ConfDoc` sobre `to_ini`). `None` si la clave no la emite `to_ini`
    /// (p. ej. una lista vacía).
    #[must_use]
    pub fn value(&self, key: &str) -> Option<String> {
        ConfDoc::parse(&self.cfg.to_ini())
            .get(key)
            .map(str::to_owned)
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

    #[test]
    fn source_connected() {
        assert!(Source::Instance.connected());
        assert!(!Source::File.connected());
        assert!(!Source::Defaults.connected());
    }

    #[test]
    fn load_con_instancia_inexistente_no_queda_conectado() {
        // Un slug que nadie usa → no hay socket → cae a fichero/defaults, nunca
        // `Source::Instance`. (M3 prueba el camino "conectado" con un socket
        // falso.)
        let m = Model::load(Some("instancia-que-no-existe-xyzzy"));
        assert_ne!(m.source, Source::Instance);
        assert!(!m.is_dirty());
    }
}
