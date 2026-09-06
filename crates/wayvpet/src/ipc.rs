//! Servidor del socket de control IPC (spec 0003 M1).
//!
//! El socket Unix de la instancia se registra en `calloop` como fuente
//! `Generic`; cada conexión trae **una línea** de petición y recibe **una línea**
//! de respuesta. `SO_PEERCRED`: solo se atiende a conexiones del **mismo uid**.
//! El fichero del socket se borra al soltar `SocketGuard`.

use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

/// Borra el fichero del socket cuando se suelta (el `UnixListener` lo posee la
/// fuente de `calloop`, así que la limpieza del path va aparte).
pub struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Enlaza el socket de la instancia (`target` = salida de `--monitor`). Limpia
/// un socket huérfano previo, lo deja en modo `0600` y no bloqueante.
///
/// # Errores
/// Errores de E/S al enlazar, configurar el modo o el no-bloqueo.
pub fn bind(target: Option<&str>) -> std::io::Result<(UnixListener, SocketGuard)> {
    let path = wayvpet_common::ipc::socket_path(target);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    eprintln!("wayvpet: IPC escuchando en {}", path.display());
    Ok((listener, SocketGuard(path)))
}

/// ¿La conexión viene de un proceso del mismo uid? (defensa en profundidad
/// sobre el modo `0600`; spec 0013). Usa `SO_PEERCRED` vía `rustix`.
#[must_use]
pub fn same_uid(stream: &UnixStream) -> bool {
    rustix::net::sockopt::get_socket_peercred(stream)
        .map(|c| c.uid == rustix::process::getuid())
        .unwrap_or(false)
}

/// Lee una línea de petición (con tope de tiempo, para no colgar el bucle si el
/// cliente no manda nada). Devuelve la línea sin el `\n`.
///
/// # Errores
/// Errores de E/S o de tiempo de espera al leer.
pub fn read_request(stream: &UnixStream) -> std::io::Result<String> {
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}
