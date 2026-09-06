//! Escritor de configuración (spec 0004): modelo del `wayvpet.conf` como lista
//! de líneas que **preserva** comentarios, líneas en blanco y el orden. Cambiar
//! una clave reescribe **solo su línea**; todo lo demás se emite tal cual.
//!
//! Comparte el partidor de líneas con el parser (`super::split_line`,
//! `super::is_comment_or_blank`), así que la clasificación de líneas es idéntica.

use super::{is_comment_or_blank, split_line};

/// Una línea del fichero.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DocLine {
    /// Comentario completo, línea en blanco, o línea sin `=`: se emite igual.
    Verbatim(String),
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

    /// Valor efectivo de `key` (la **última** aparición, como el parser INI).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().rev().find_map(|l| match l {
            DocLine::Kv { key: k, value, .. } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Fija `key = value`. Si ya existe, reescribe la **primera** aparición
    /// (conservando su comentario en línea) y borra las demás, de modo que el
    /// valor efectivo quede sin ambigüedad. Si no existe, añade una línea al
    /// final. No usar con claves de lista (`keyboard_device`, `monitor`…).
    pub fn set(&mut self, key: &str, value: &str) {
        let matches: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, DocLine::Kv { key: k, .. } if k == key))
            .map(|(i, _)| i)
            .collect();

        match matches.split_first() {
            None => self.lines.push(DocLine::Kv {
                key: key.to_string(),
                value: value.to_string(),
                comment: None,
                raw: None,
            }),
            Some((&first, rest)) => {
                if let DocLine::Kv { value: v, raw, .. } = &mut self.lines[first] {
                    *v = value.to_string();
                    *raw = None; // re-renderiza desde las partes
                }
                for &i in rest.iter().rev() {
                    self.lines.remove(i);
                }
            }
        }
    }

    /// Borra todas las líneas `key = …`. Devuelve cuántas quitó.
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
}
