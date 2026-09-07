//! Escritor de configuración (spec 0004): modelo del `wayvpet.conf` como lista
//! de líneas que **preserva** comentarios, líneas en blanco y el orden. Cambiar
//! una clave reescribe **solo su línea**; todo lo demás se emite tal cual.
//!
//! Comparte el partidor de líneas con el parser (`super::split_line`,
//! `super::is_comment_or_blank`), así que la clasificación de líneas es idéntica.

use super::{is_comment_or_blank, section_header, split_line};

/// Una línea del fichero.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DocLine {
    /// Comentario completo, línea en blanco, o línea sin `=`: se emite igual.
    Verbatim(String),
    /// Cabecera `[monitor:NOMBRE]` (spec 0008 §8.4). `raw` conserva el texto
    /// original; `name` es el nombre de la salida.
    Section { name: String, raw: String },
    /// `clave = valor  # comentario`. `raw` guarda el texto original mientras no
    /// se toque; al modificar el valor se pone a `None` y la línea se re-renderiza.
    Kv {
        key: String,
        value: String,
        comment: Option<String>,
        raw: Option<String>,
    },
}

impl DocLine {
    fn render(&self) -> String {
        match self {
            DocLine::Verbatim(s) => s.clone(),
            DocLine::Section { raw, .. } => raw.clone(),
            DocLine::Kv { raw: Some(r), .. } => r.clone(),
            DocLine::Kv {
                key,
                value,
                comment,
                ..
            } => match comment {
                Some(c) => format!("{key}={value} # {c}"),
                None => format!("{key}={value}"),
            },
        }
    }
}

/// El `wayvpet.conf` como documento editable que conserva el formato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfDoc {
    lines: Vec<DocLine>,
    /// ¿El texto original terminaba en `\n`? (para reproducirlo al renderizar).
    final_newline: bool,
}

impl ConfDoc {
    /// Parte el texto en líneas, clasificándolas como el parser.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let final_newline = text.is_empty() || text.ends_with('\n');
        let lines = text
            .lines()
            .map(|raw| {
                if is_comment_or_blank(raw) {
                    DocLine::Verbatim(raw.to_string())
                } else if let Some(Some(name)) = section_header(raw) {
                    DocLine::Section {
                        name: name.to_string(),
                        raw: raw.to_string(),
                    }
                } else if let Some(l) = split_line(raw) {
                    DocLine::Kv {
                        key: l.key,
                        value: l.value,
                        comment: l.comment,
                        raw: Some(raw.to_string()),
                    }
                } else {
                    // Línea sin `=` que no es comentario: se conserva verbatim.
                    DocLine::Verbatim(raw.to_string())
                }
            })
            .collect();
        Self {
            lines,
            final_newline,
        }
    }

    /// Índice de la primera cabecera `[monitor:…]`, o el nº de líneas si no hay
    /// ninguna. Las claves **base** viven en `0..este_índice`.
    fn base_end(&self) -> usize {
        self.lines
            .iter()
            .position(|l| matches!(l, DocLine::Section { .. }))
            .unwrap_or(self.lines.len())
    }

    /// Rango `[inicio, fin)` del **cuerpo** de la sección `[monitor:name]` (sin
    /// contar su cabecera), o `None` si esa sección no existe.
    fn section_body(&self, name: &str) -> Option<(usize, usize)> {
        let start = self
            .lines
            .iter()
            .position(|l| matches!(l, DocLine::Section { name: n, .. } if n == name))?
            + 1;
        let end = self.lines[start..]
            .iter()
            .position(|l| matches!(l, DocLine::Section { .. }))
            .map_or(self.lines.len(), |off| start + off);
        Some((start, end))
    }

    /// Valor efectivo de `key` en la **base** (última aparición antes de
    /// cualquier sección `[monitor:…]`), como el parser INI.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.get_in(0, self.base_end(), key)
    }

    /// Valor efectivo de `key` dentro de la sección `[monitor:name]`, o `None`
    /// si la sección o la clave no están.
    #[must_use]
    pub fn get_section(&self, name: &str, key: &str) -> Option<&str> {
        let (a, b) = self.section_body(name)?;
        self.get_in(a, b, key)
    }

    fn get_in(&self, lo: usize, hi: usize, key: &str) -> Option<&str> {
        self.lines[lo..hi].iter().rev().find_map(|l| match l {
            DocLine::Kv { key: k, value, .. } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Fija `key = value` en la **base**. Si ya existe, reescribe la primera
    /// aparición (conservando su comentario) y borra las demás; si no, añade una
    /// línea al final de la base. No usar con claves de lista (`keyboard_device`,
    /// `monitor`…).
    pub fn set(&mut self, key: &str, value: &str) {
        let end = self.base_end();
        self.set_in(0, end, key, value);
    }

    /// Fija `key = value` dentro de la sección `[monitor:name]` (spec 0008 §8.4).
    /// Si la sección no existe, la crea al final del fichero. Es lo que usa el
    /// modo edición al persistir en multi-monitor.
    pub fn set_section(&mut self, name: &str, key: &str, value: &str) {
        match self.section_body(name) {
            Some((a, b)) => {
                self.set_in(a, b, key, value);
            }
            None => {
                self.lines.push(DocLine::Section {
                    name: name.to_string(),
                    raw: format!("[monitor:{name}]"),
                });
                self.lines.push(DocLine::Kv {
                    key: key.to_string(),
                    value: value.to_string(),
                    comment: None,
                    raw: None,
                });
            }
        }
    }

    /// Reescribe (o añade) `key=value` dentro de `[lo, hi)`. Colapsa duplicados.
    fn set_in(&mut self, lo: usize, hi: usize, key: &str, value: &str) {
        let matches: Vec<usize> = self.lines[lo..hi]
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, DocLine::Kv { key: k, .. } if k == key))
            .map(|(i, _)| lo + i)
            .collect();

        match matches.split_first() {
            None => self.lines.insert(
                hi,
                DocLine::Kv {
                    key: key.to_string(),
                    value: value.to_string(),
                    comment: None,
                    raw: None,
                },
            ),
            Some((&first, rest)) => {
                if let DocLine::Kv { value: v, raw, .. } = &mut self.lines[first] {
                    *v = value.to_string();
                    *raw = None;
                }
                for &i in rest.iter().rev() {
                    self.lines.remove(i);
                }
            }
        }
    }

    /// Borra todas las líneas `key = …` (en cualquier sección). Devuelve cuántas
    /// quitó.
    pub fn remove(&mut self, key: &str) -> usize {
        let before = self.lines.len();
        self.lines
            .retain(|l| !matches!(l, DocLine::Kv { key: k, .. } if k == key));
        before - self.lines.len()
    }

    /// Reconstruye el texto del fichero.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str(&l.render());
            out.push('\n');
        }
        if !self.final_newline {
            out.pop();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::ConfDoc;

    const SAMPLE: &str = "\
# Configuración de ejemplo
fps = 30            # fotogramas por segundo

cat_height=110
; línea rara sin igual
monitor=eDP-1
";

    #[test]
    fn ida_y_vuelta_conserva_el_texto() {
        assert_eq!(ConfDoc::parse(SAMPLE).render(), SAMPLE);
    }

    #[test]
    fn sin_salto_final_se_respeta() {
        let t = "fps=30\ncat_height=40";
        assert_eq!(ConfDoc::parse(t).render(), t);
    }

    #[test]
    fn get_devuelve_la_ultima_aparicion() {
        let d = ConfDoc::parse("fps=30\nfps=60\n");
        assert_eq!(d.get("fps"), Some("60"));
        assert_eq!(d.get("ausente"), None);
    }

    #[test]
    fn set_reescribe_en_sitio_y_conserva_comentario() {
        let mut d = ConfDoc::parse(SAMPLE);
        d.set("fps", "90");
        let out = d.render();
        assert!(out.contains("fps=90 # fotogramas por segundo"), "{out}");
        // el resto intacto
        assert!(out.contains("# Configuración de ejemplo"));
        assert!(out.contains("cat_height=110"));
        assert!(out.contains("; línea rara sin igual"));
        assert_eq!(d.get("fps"), Some("90"));
    }

    #[test]
    fn set_de_clave_nueva_anade_al_final() {
        let mut d = ConfDoc::parse("fps=30\n");
        d.set("cat_opacity", "50");
        assert_eq!(d.render(), "fps=30\ncat_opacity=50\n");
    }

    #[test]
    fn set_colapsa_duplicados() {
        let mut d = ConfDoc::parse("fps=30\n# medio\nfps=60\n");
        d.set("fps", "45");
        assert_eq!(d.render(), "fps=45\n# medio\n");
        assert_eq!(d.get("fps"), Some("45"));
    }

    #[test]
    fn remove_quita_todas() {
        let mut d = ConfDoc::parse("monitor=A\nmonitor=B\nfps=30\n");
        assert_eq!(d.remove("monitor"), 2);
        assert_eq!(d.render(), "fps=30\n");
    }

    // ── Secciones [monitor:NOMBRE] (spec 0008 §8.4 M2) ─────────────────────

    const SECCIONADO: &str = "\
# base
fps=60
cat_height=100

[monitor:eDP-1]
cat_height=140

[monitor:HDMI-A-1]
cat_x_offset=300
";

    #[test]
    fn ida_y_vuelta_con_secciones() {
        assert_eq!(ConfDoc::parse(SECCIONADO).render(), SECCIONADO);
    }

    #[test]
    fn get_es_solo_de_la_base_get_section_de_la_seccion() {
        let d = ConfDoc::parse(SECCIONADO);
        assert_eq!(
            d.get("cat_height"),
            Some("100"),
            "get = base, no la sección"
        );
        assert_eq!(d.get_section("eDP-1", "cat_height"), Some("140"));
        assert_eq!(d.get_section("HDMI-A-1", "cat_x_offset"), Some("300"));
        assert_eq!(d.get_section("eDP-1", "cat_x_offset"), None);
        assert_eq!(d.get_section("NO-HAY", "fps"), None);
    }

    #[test]
    fn set_toca_la_base_y_no_la_seccion() {
        let mut d = ConfDoc::parse(SECCIONADO);
        d.set("cat_height", "120");
        assert_eq!(d.get("cat_height"), Some("120"));
        assert_eq!(
            d.get_section("eDP-1", "cat_height"),
            Some("140"),
            "la sección intacta"
        );
        // la clave nueva de base entra ANTES de la primera sección
        d.set("mirror_x", "1");
        let out = d.render();
        let base = out.split("[monitor:").next().unwrap();
        assert!(base.contains("mirror_x=1"), "{out}");
    }

    #[test]
    fn set_section_reescribe_en_sitio_y_crea_si_falta() {
        let mut d = ConfDoc::parse(SECCIONADO);
        // reescribe dentro de la sección existente
        d.set_section("eDP-1", "cat_height", "150");
        assert_eq!(d.get_section("eDP-1", "cat_height"), Some("150"));
        assert_eq!(d.get("cat_height"), Some("100"), "la base intacta");
        // clave nueva en sección existente
        d.set_section("HDMI-A-1", "theme", "miku");
        assert_eq!(d.get_section("HDMI-A-1", "theme"), Some("miku"));
        // sección nueva al final
        d.set_section("DP-2", "cat_y_offset", "20");
        assert_eq!(d.get_section("DP-2", "cat_y_offset"), Some("20"));
        assert!(d.render().contains("[monitor:DP-2]\ncat_y_offset=20"));
    }

    #[test]
    fn set_section_desde_cero() {
        let mut d = ConfDoc::parse("fps=30\n");
        d.set_section("eDP-1", "cat_height", "140");
        assert_eq!(d.render(), "fps=30\n[monitor:eDP-1]\ncat_height=140\n");
        assert_eq!(d.get("cat_height"), None);
        assert_eq!(d.get_section("eDP-1", "cat_height"), Some("140"));
    }
}
