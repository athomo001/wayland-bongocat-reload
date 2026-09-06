//! Partidor de líneas `clave = valor  # comentario` del INI.
//!
//! Compartido entre el parser (este crate) y el futuro escritor de
//! configuración (spec 0004): ambos deben partir las líneas **igual**, así que
//! la lógica vive aquí una sola vez. Portado de `config_parse_line`
//! (`src/config/config.c`).

/// Una línea `clave = valor` ya partida y con el comentario en línea separado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Clave sin espacios.
    pub key: String,
    /// Valor sin espacios ni comentario en línea.
    pub value: String,
    /// Texto del comentario en línea (sin el `#`), si lo había.
    pub comment: Option<String>,
}

/// Parte una línea en `clave` / `valor` / `comentario`. Devuelve `None` si la
/// línea no contiene `=` (línea inválida). No filtra líneas de comentario
/// completas ni vacías: de eso se encarga el llamador.
#[must_use]
pub fn split_line(raw: &str) -> Option<Line> {
    let eq = raw.find('=')?;
    let key = raw[..eq].trim().to_string();
    let rest = raw[eq + 1..].trim();

    let (value, comment) = if let Some(stripped) = rest.strip_prefix('#') {
        // El valor era solo un comentario.
        (String::new(), Some(stripped.trim().to_string()))
    } else if let Some(pos) = find_inline_comment(rest) {
        let (v, c) = rest.split_at(pos);
        (v.trim().to_string(), Some(c[1..].trim().to_string()))
    } else {
        (rest.to_string(), None)
    };

    Some(Line {
        key,
        value,
        comment,
    })
}

/// Posición del `#` de un comentario en línea: el primer `#` precedido de espacio
/// o tabulador. Un `#` pegado a un carácter no es comentario (el C busca
/// literalmente `" #"` o `"\t#"`).
fn find_inline_comment(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    (1..b.len()).find(|&i| b[i] == b'#' && (b[i - 1] == b' ' || b[i - 1] == b'\t'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_igual_es_none() {
        assert_eq!(split_line("; comentario con punto y coma"), None);
        assert_eq!(split_line("solo texto"), None);
    }

    #[test]
    fn clave_valor_simple() {
        let l = split_line("  fps = 30  ").unwrap();
        assert_eq!(l.key, "fps");
        assert_eq!(l.value, "30");
        assert_eq!(l.comment, None);
    }

    #[test]
    fn comentario_en_linea() {
        let l = split_line("fps = 30  # comentario en línea").unwrap();
        assert_eq!(l.value, "30");
        assert_eq!(l.comment.as_deref(), Some("comentario en línea"));
    }

    #[test]
    fn valor_es_solo_comentario() {
        let l = split_line("monitor = # nada").unwrap();
        assert_eq!(l.value, "");
        assert_eq!(l.comment.as_deref(), Some("nada"));
    }

    #[test]
    fn almohadilla_pegada_no_es_comentario() {
        let l = split_line("keyboard_name = teclado#1").unwrap();
        assert_eq!(l.value, "teclado#1");
        assert_eq!(l.comment, None);
    }

    #[test]
    fn primer_igual_manda() {
        let l = split_line("a = b = c").unwrap();
        assert_eq!(l.key, "a");
        assert_eq!(l.value, "b = c");
    }
}
