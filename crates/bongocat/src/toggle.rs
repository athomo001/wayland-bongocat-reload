//! `--toggle`: si hay una instancia corriendo, la para; si no, indica que se
//! siga con el arranque normal. Porta `process_handle_toggle` de
//! `src/core/main.c`.

use std::thread::sleep;
use std::time::Duration;

use rustix::process::{kill_process, Pid, Signal};

use crate::pidfile::pid_path;

/// Qué hacer tras `--toggle`.
pub enum Outcome {
    /// Se paró la instancia que había: `main` debe salir con éxito.
    Stopped,
    /// No había ninguna: `main` debe continuar con el arranque normal.
    NotRunning,
}

/// Lee el PID guardado, comprueba que sea un `bongocat` vivo y le manda
/// `SIGTERM` (y `SIGKILL` si no muere en 5 s).
#[must_use]
pub fn run() -> Outcome {
    let path = pid_path();
    let Some(pid) = read_running_pid(&path) else {
        return Outcome::NotRunning;
    };
    let Some(target) = Pid::from_raw(pid) else {
        return Outcome::NotRunning;
    };

    eprintln!("bongocat: parando la instancia PID {pid}");
    if kill_process(target, Signal::Term).is_err() {
        return Outcome::NotRunning;
    }
    for _ in 0..50 {
        if !proc_alive(pid) {
            eprintln!("bongocat: detenido");
            return Outcome::Stopped;
        }
        sleep(Duration::from_millis(100));
    }
    eprintln!("bongocat: no responde, SIGKILL");
    let _ = kill_process(target, Signal::Kill);
    Outcome::Stopped
}

/// PID del fichero si apunta a un proceso vivo llamado `bongocat`. Limpia el
/// fichero si está obsoleto.
fn read_running_pid(path: &std::path::Path) -> Option<i32> {
    let txt = std::fs::read_to_string(path).ok()?;
    let pid: i32 = txt.trim().parse().ok().filter(|&p| p > 1)?;
    if !proc_alive(pid) || comm(pid).as_deref() != Some("bongocat") {
        let _ = std::fs::remove_file(path);
        return None;
    }
    Some(pid)
}

fn proc_alive(pid: i32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

fn comm(pid: i32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
}
