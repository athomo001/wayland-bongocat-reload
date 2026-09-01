//! Descubrimiento de teclados y arranque del **proceso lector aislado**.
//!
//! El descubrimiento (`evdev::enumerate`) ocurre aquí, en el proceso de
//! confianza. La lectura de `/dev/input/event*` se hace en un proceso hijo
//! separado con seccomp (ver [`crate::input_child`]): así, aunque una entrada
//! evdev maliciosa comprometiese al lector, no puede tocar Wayland, la config,
//! la red ni ejecutar nada (spec 0013 §2).
//!
//! Al hijo solo le cruza **1 byte por pulsación** (el bit de pata ya reducido);
//! el identificador de la tecla no se guarda, registra ni transmite.

#![allow(unsafe_code)] // punto de FFI documentado: pipe2 + fork

use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::thread;

use evdev::{Device, Key};

/// ¿Este dispositivo parece un teclado? (tiene las letras y Enter).
fn looks_like_keyboard(dev: &Device) -> bool {
    dev.supported_keys().is_some_and(|k| {
        k.contains(Key::KEY_A) && k.contains(Key::KEY_Z) && k.contains(Key::KEY_ENTER)
    })
}

/// Escanea `/dev/input` y devuelve `(ruta, nombre)` de lo que parece un teclado.
#[must_use]
pub fn detect_keyboards() -> Vec<(PathBuf, String)> {
    evdev::enumerate()
        .filter(|(_, d)| looks_like_keyboard(d))
        .map(|(p, d)| (p, d.name().unwrap_or("(sin nombre)").to_string()))
        .collect()
}

/// Decide qué dispositivos leerá el hijo. Estrategia: los de la configuración
/// que existan y parezcan teclado; si ninguno lo parece (o la lista está vacía),
/// se usan los detectados automáticamente. Así el gato reacciona aunque el
/// `keyboard_device` del `.conf` esté mal (p. ej. apuntando a un botón rfkill).
#[must_use]
pub fn resolve_devices(configured: &[String]) -> Vec<String> {
    let detected = detect_keyboards();
    if detected.is_empty() {
        eprintln!("bongocat: no se detectó ningún teclado en /dev/input (¿grupo 'input'?)");
    } else {
        eprintln!("bongocat: teclados detectados:");
        for (p, n) in &detected {
            eprintln!("           {}  —  {n}", p.display());
        }
    }

    let mut chosen = Vec::new();
    for path in configured {
        match Device::open(path) {
            Ok(dev) if looks_like_keyboard(&dev) => chosen.push(path.clone()),
            Ok(_) => eprintln!("bongocat: {path} no parece un teclado; se ignora"),
            Err(e) => eprintln!("bongocat: no se pudo abrir {path}: {e} (¿grupo 'input'?)"),
        }
    }

    if chosen.is_empty() {
        chosen = detected
            .into_iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        if !chosen.is_empty() {
            eprintln!("bongocat: uso los teclados detectados automáticamente");
        }
    }
    chosen
}

/// Extremo de padre del lector aislado.
pub struct Isolated {
    /// Extremo de lectura de la tubería: bytes = bits de pata.
    pub read: OwnedFd,
}

/// Lanza el proceso lector aislado. **Debe llamarse mientras el proceso es
/// monohilo** (antes de conectar a Wayland o de crear cualquier hilo) y antes de
/// tomar el fichero PID, para que el hijo no herede esos descriptores.
///
/// El hijo hace `fork` sin `exec`: ejecuta [`crate::input_child::run`], que
/// aplica el endurecimiento y no regresa.
pub fn start(configured: &[String]) -> io::Result<Isolated> {
    let devices = resolve_devices(configured);

    // Tubería: ambos extremos con O_CLOEXEC. El hijo no hace `exec`, así que su
    // extremo sigue válido; en un `exec` futuro (no hay) se cerrarían solos.
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `pipe2` con un array de 2 enteros y flags constantes.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let (rd, wr) = (fds[0], fds[1]);

    // SAFETY: el proceso es monohilo aquí (contrato de la función). El hijo solo
    // llama a `input_child::run`, que termina con `_exit` sin desenrollar.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => {
            let e = io::Error::last_os_error();
            // SAFETY: fds válidos recién creados.
            unsafe {
                libc::close(rd);
                libc::close(wr);
            }
            Err(e)
        }
        0 => {
            // Hijo.
            // SAFETY: cerramos el extremo que no usa; `rd` es válido.
            unsafe { libc::close(rd) };
            crate::input_child::run(&devices, wr);
        }
        _ => {
            // Padre.
            // SAFETY: cerramos el extremo de escritura; `wr` es válido.
            unsafe { libc::close(wr) };
            // SAFETY: `rd` es un fd válido del que somos dueños en exclusiva.
            let read = unsafe { OwnedFd::from_raw_fd(rd) };
            reap_in_background(pid);
            Ok(Isolated { read })
        }
    }
}

/// Recolecta al hijo cuando termine (evita el zombi) sin bloquear el bucle.
fn reap_in_background(pid: libc::pid_t) {
    let _ = thread::Builder::new()
        .name("input:reaper".into())
        .spawn(move || {
            let mut status = 0;
            // SAFETY: `waitpid` sobre nuestro propio hijo; puntero a un i32 local.
            unsafe { libc::waitpid(pid, &mut status, 0) };
            eprintln!("bongocat: el proceso lector de input ({pid}) terminó");
        });
}
