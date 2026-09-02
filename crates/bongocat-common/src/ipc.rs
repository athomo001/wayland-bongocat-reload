//! Ubicación del socket de control IPC (spec 0003), compartida entre el overlay
//! (servidor) y `bongocatctl` (cliente).
//!
//! El protocolo es de texto, una línea por petición y una por respuesta:
//! `PING` → `PONG`, `STATE` → una línea `clave=valor …`, `QUIT` → `OK`.
//! (`GET`/`SET`/`SAVE`/`RELOAD` llegan en las siguientes rebanadas.)

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// Nombre de instancia saneado para el fichero de socket / PID.
#[must_use]
pub fn instance_slug(target: Option<&str>) -> Option<String> {
    target.map(|name| {
        name.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    })
}

/// `$XDG_RUNTIME_DIR/bongocat[-<slug>].sock` (o bajo `/tmp`). `target` es la
/// salida fijada con `--monitor`: hay un socket por instancia.
#[must_use]
pub fn socket_path(target: Option<&str>) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    match instance_slug(target) {
        Some(slug) => dir.join(format!("bongocat-{slug}.sock")),
        None => dir.join("bongocat.sock"),
    }
}

/// Cliente: conecta al socket de la instancia `target`, manda `req` (una línea)
/// y devuelve la respuesta (una línea, sin el `\n`).
///
/// # Errores
/// Si no hay socket (instancia no corriendo), o hay error de E/S / timeout.
pub fn send_request(target: Option<&str>, req: &str) -> std::io::Result<String> {
    let stream = UnixStream::connect(socket_path(target))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    writeln!(&stream, "{}", req.trim())?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    Ok(reply.trim_end_matches(['\r', '\n']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_sanea_los_no_alfanumericos() {
        assert_eq!(instance_slug(Some("HDMI-A-1")).as_deref(), Some("HDMI_A_1"));
        assert_eq!(instance_slug(None), None);
    }

    #[test]
    fn socket_nombre_por_instancia() {
        // Sin depender del entorno: solo el nombre de fichero.
        assert_eq!(socket_path(None).file_name().unwrap(), "bongocat.sock");
        assert_eq!(
            socket_path(Some("eDP-1")).file_name().unwrap(),
            "bongocat-eDP_1.sock"
        );
    }
}
