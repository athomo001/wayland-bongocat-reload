//! Fichero PID en `$XDG_RUNTIME_DIR/wayvpet.pid` con `flock` exclusivo, para
//! que solo corra una instancia y para que `--toggle` sepa a quién parar.
//! Porta `process_create_pid_file` / `get_pid_file_path` de `src/core/main.c`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use rustix::fs::{flock, FlockOperation};

/// Ruta del fichero PID: `$XDG_RUNTIME_DIR/wayvpet.pid`, o `/tmp/wayvpet.pid`.
///
/// Con `target` (una salida concreta, p. ej. `--monitor HDMI-A-1`) el nombre
/// pasa a `wayvpet-HDMI-A-1.pid`: así puede correr una instancia por monitor
/// sin que se pisen el lock.
#[must_use]
pub fn pid_path(target: Option<&str>) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    match target {
        Some(name) => {
            let slug: String = name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            dir.join(format!("wayvpet-{slug}.pid"))
        }
        None => dir.join("wayvpet.pid"),
    }
}

/// Fichero PID adquirido: mantiene el `flock` mientras viva y borra el fichero
/// al soltarse (`Drop`).
pub struct PidFile {
    path: PathBuf,
    _file: File,
}

/// Resultado de intentar adquirir el fichero PID.
pub enum Acquire {
    /// Adquirido; hay que mantener el valor vivo.
    Ok(PidFile),
    /// Ya hay otra instancia con el lock.
    AlreadyRunning,
}

impl PidFile {
    /// Crea el fichero (`0600`, `O_NOFOLLOW`), toma el `flock` exclusivo no
    /// bloqueante y escribe el PID actual.
    pub fn acquire(target: Option<&str>) -> std::io::Result<Acquire> {
        let path = pid_path(target);
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;

        match flock(file.as_fd(), FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => return Ok(Acquire::AlreadyRunning),
            Err(e) => return Err(e.into()),
        }

        (&file).write_all(format!("{}\n", std::process::id()).as_bytes())?;
        Ok(Acquire::Ok(PidFile { path, _file: file }))
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
