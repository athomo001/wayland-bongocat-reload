//! Descubrimiento de teclados/ratones y arranque del **proceso lector aislado**.
//!
//! El descubrimiento (`evdev::enumerate`) ocurre aquí, en el proceso de
//! confianza. La lectura de `/dev/input/event*` se hace en un proceso hijo
//! separado con seccomp (ver [`crate::input_child`]): así, aunque una entrada
//! evdev maliciosa comprometiese al lector, no puede tocar Wayland, la config,
//! la red ni ejecutar nada (spec 0013 §2).
//!
//! Al padre solo le cruza **1 byte por pulsación / golpecito de ratón** (el bit
//! de pata ya reducido); ni la tecla ni la posición del ratón se guardan,
//! registran ni transmiten.

#![allow(unsafe_code)] // punto de FFI documentado: pipe2 + fork

use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::thread;

use bongocat_common::config::Config;
use evdev::{AbsoluteAxisType, Device, Key, RelativeAxisType};

/// ¿Este dispositivo parece un teclado? (tiene las letras y Enter).
fn looks_like_keyboard(dev: &Device) -> bool {
    dev.supported_keys().is_some_and(|k| {
        k.contains(Key::KEY_A) && k.contains(Key::KEY_Z) && k.contains(Key::KEY_ENTER)
    })
}

/// ¿Este dispositivo parece un ratón o touchpad? (eje relativo/absoluto X/Y o botón).
fn looks_like_mouse(dev: &Device) -> bool {
    dev.supported_relative_axes()
        .is_some_and(|a| a.contains(RelativeAxisType::REL_X) || a.contains(RelativeAxisType::REL_Y))
        || dev.supported_absolute_axes().is_some_and(|a| {
            a.contains(AbsoluteAxisType::ABS_X) || a.contains(AbsoluteAxisType::ABS_MT_POSITION_X)
        })
        || dev
            .supported_keys()
            .is_some_and(|k| k.contains(Key::BTN_LEFT) || k.contains(Key::BTN_TOUCH))
}

fn detect(pred: fn(&Device) -> bool) -> Vec<(PathBuf, String)> {
    evdev::enumerate()
        .filter(|(_, d)| pred(d))
        .map(|(p, d)| (p, d.name().unwrap_or("(sin nombre)").to_string()))
        .collect()
}

/// Resuelve qué dispositivos leerá el hijo para un rol. Estrategia: los de la
/// configuración que existan y casen con `pred`; si ninguno casa (o la lista
/// está vacía), se usan los detectados automáticamente. `rol` es solo para los
/// mensajes.
fn resolve(configured: &[String], pred: fn(&Device) -> bool, rol: &str) -> Vec<String> {
    let detected = detect(pred);
    if detected.is_empty() {
        eprintln!("bongocat: no se detectó ningún {rol} en /dev/input (¿grupo 'input'?)");
    } else {
        eprintln!("bongocat: {rol}s detectados:");
        for (p, n) in &detected {
            eprintln!("           {}  —  {n}", p.display());
        }
    }

    let mut chosen = Vec::new();
    for path in configured {
        match Device::open(path) {
            Ok(dev) if pred(&dev) => chosen.push(path.clone()),
            Ok(_) => eprintln!("bongocat: {path} no parece un {rol}; se ignora"),
            Err(e) => eprintln!("bongocat: no se pudo abrir {path}: {e} (¿grupo 'input'?)"),
        }
    }

    if chosen.is_empty() {
        chosen = detected
            .into_iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        if !chosen.is_empty() {
            eprintln!("bongocat: uso los {rol}s detectados automáticamente");
        }
    }
    chosen
}

/// Teclados a leer (config o autodetección). Ver [`resolve`].
#[must_use]
pub fn resolve_devices(configured: &[String]) -> Vec<String> {
    resolve(configured, looks_like_keyboard, "teclado")
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
///
/// # Errores
/// Errores de E/S al crear la tubería o al hacer `fork`.
pub fn start(cfg: &Config) -> io::Result<Isolated> {
    let keyboards = resolve_devices(&cfg.keyboard_devices);
    let mice = if cfg.enable_mouse {
        resolve(&cfg.mouse_devices, looks_like_mouse, "ratón")
    } else {
        eprintln!("bongocat: enable_mouse=0; el ratón se ignora");
        Vec::new()
    };
    let mouse_paw = cfg.mouse_paw;
    let move_interval_ms = cfg.mouse_move_interval.max(1) as u64;

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
            crate::input_child::run(&keyboards, &mice, mouse_paw, move_interval_ms, wr);
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
