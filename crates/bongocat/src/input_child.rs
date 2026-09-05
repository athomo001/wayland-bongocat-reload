//! Proceso hijo aislado que lee `/dev/input/event*` (spec 0013 §2).
//!
//! Este código corre en un **proceso aparte** del overlay. Su única salida es un
//! byte por pulsación (el bit de pata ya reducido) escrito en una tubería; nunca
//! ve la conexión de Wayland, ni los búferes SHM, ni la configuración, ni la
//! red. Endurecimiento aplicado antes del bucle de lectura:
//!
//!   * `PR_SET_NO_NEW_PRIVS` y `PR_SET_DUMPABLE=0` (sin escalada de privilegios,
//!     sin adjuntar depurador ni volcados de memoria).
//!   * **seccomp-BPF**: lista blanca mínima de llamadas al sistema; cualquier
//!     otra (`execve`, `open*`, `socket`, `ptrace`, `clone`, `prctl`…) mata el
//!     proceso. Así, aunque una entrada evdev maliciosa lograse ejecución de
//!     código, no puede tocar nada del sistema.
//!   * **Nada de logs por pulsación ni de movimiento de ratón**: el keycode y
//!     los deltas del ratón se reducen a 1 bit de pata y se descartan; no se
//!     registra qué tecla fue ni hacia dónde se movió el ratón.
//!
//! Si la arquitectura no está soportada por el filtro, se sigue sin seccomp
//! (el aislamiento por proceso ya vale): se avisa por `stderr`.

#![allow(unsafe_code)] // punto de FFI documentado: prctl + fork-side del hijo

use std::io::Read;
use std::os::fd::RawFd;
use std::thread;

use bongocat_common::config::MousePaw;
use bongocat_common::mouse::mouse_paw_bit;
use bongocat_common::paw::paw_for_keycode;
use evdev::{AbsoluteAxisType, Device, EventType, Key, RelativeAxisType};

/// Punto de entrada del hijo. No regresa: termina con `_exit`.
///
/// `keyboards` y `mice` son rutas ya resueltas por el padre (descubrimiento en
/// el proceso de confianza); aquí solo se abren. `write_fd` es el extremo de
/// escritura de la tubería hacia el padre.
pub fn run(
    keyboards: &[String],
    mice: &[String],
    mouse_paw: MousePaw,
    move_interval_ms: u64,
    write_fd: RawFd,
) -> ! {
    harden();

    // Abrir los dispositivos ANTES de seccomp (después, `open` está prohibido).
    let open_all = |paths: &[String]| -> Vec<(Device, String)> {
        paths
            .iter()
            .filter_map(|p| match Device::open(p) {
                Ok(d) => Some((d, p.clone())),
                Err(e) => {
                    eprintln!("bongocat[input]: no se pudo abrir {p}: {e}");
                    None
                }
            })
            .collect()
    };
    let kbds = open_all(keyboards);
    let mice = open_all(mice);

    if kbds.is_empty() && mice.is_empty() {
        eprintln!("bongocat[input]: ningún dispositivo; el hijo termina");
        exit(0);
    }

    // Un hilo lector por dispositivo (lectura bloqueante). Se crean ANTES de
    // aplicar el filtro para que `clone` pueda quedar prohibido después.
    let mut handles = Vec::new();
    for (dev, path) in kbds {
        handles.push(
            thread::Builder::new()
                .name(format!("kbd:{path}"))
                .spawn(move || keyboard_thread(dev, &path, write_fd))
                .expect("crear hilo de teclado"),
        );
    }
    for (dev, path) in mice {
        handles.push(
            thread::Builder::new()
                .name(format!("mouse:{path}"))
                .spawn(move || mouse_thread(dev, &path, write_fd, mouse_paw, move_interval_ms))
                .expect("crear hilo de ratón"),
        );
    }

    // seccomp para todos los hilos (TSYNC). Best-effort: si falla, se avisa.
    install_seccomp();

    for h in handles {
        let _ = h.join();
    }
    exit(0);
}

/// Escribe 1 byte (el bit de pata) por la tubería. `false` = el padre la cerró.
fn send_bit(write_fd: RawFd, bit: u8) -> bool {
    let buf = [bit];
    // SAFETY: `write` sobre un fd válido; 1 byte < PIPE_BUF, atómico aun con
    // varios hilos escribiendo.
    unsafe { libc::write(write_fd, buf.as_ptr().cast(), 1) == 1 }
}

/// Teclas de captura de pantalla o control del sistema que no deben despertar a
/// la mascota ni considerarse pulsaciones de tecleo (99=SysRq/PrintScreen, 210=Print).
#[must_use]
fn is_system_screenshot_key(code: u16) -> bool {
    matches!(code, 99 | 210)
}

/// Bucle de teclado. Por cada key-down: reduce el keycode a un bit de pata y lo
/// manda por la tubería. Nunca registra la tecla.
fn keyboard_thread(mut dev: Device, path: &str, write_fd: RawFd) {
    loop {
        let batch = match dev.fetch_events() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("bongocat[input]: {path} dejó de leer: {e}");
                return;
            }
        };
        for ev in batch {
            // `| PAW_KEY`: marca el byte como "de teclado" para el contador de
            // teclas/min del padre (`happy_kpm`). El keycode sigue sin salir.
            if ev.event_type() == EventType::KEY && ev.value() == 1 {
                if is_system_screenshot_key(ev.code()) {
                    continue;
                }
                if !send_bit(
                    write_fd,
                    paw_for_keycode(i32::from(ev.code())) | bongocat_common::paw::PAW_KEY,
                ) {
                    return; // el padre cerró la tubería
                }
            }
        }
    }
}

/// Bucle de ratón / touchpad. Botones y rueda → golpecito; movimiento relativo o
/// absoluto del touchpad → actualiza la dirección de mirada (gaze) para que los
/// ojos sigan el cursor.
fn mouse_thread(mut dev: Device, path: &str, write_fd: RawFd, paw: MousePaw, _interval_ms: u64) {
    // PRNG diminuto para `MousePaw::Random` (no hace falta entropía real).
    let mut rng: u64 = u64::from(std::process::id()) ^ 0x9E37_79B9_7F4A_7C15;
    let mut coin = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng & 1 == 1
    };

    let mut gaze_x: i64 = 0;
    let mut gaze_y: i64 = 0;
    let mut last_gaze = bongocat_common::mouse::GazeDirection::Center;
    let mut prev_abs_x: Option<i64> = None;
    let mut prev_abs_y: Option<i64> = None;

    loop {
        let batch = match dev.fetch_events() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("bongocat[input]: {path} dejó de leer: {e}");
                return;
            }
        };
        let mut tap_now = false;
        let mut moved = false;

        for ev in batch {
            match ev.event_type() {
                EventType::KEY => {
                    if ev.value() == 1 {
                        tap_now = true;
                    } else if ev.code() == Key::BTN_TOUCH.0 && ev.value() == 0 {
                        prev_abs_x = None;
                        prev_abs_y = None;
                    }
                }
                EventType::RELATIVE => match RelativeAxisType(ev.code()) {
                    RelativeAxisType::REL_WHEEL | RelativeAxisType::REL_HWHEEL
                        if ev.value() != 0 =>
                    {
                        tap_now = true;
                    }
                    RelativeAxisType::REL_X => {
                        let v = i64::from(ev.value());
                        gaze_x = (gaze_x * 4 / 5 + v).clamp(-120, 120);
                        moved = true;
                    }
                    RelativeAxisType::REL_Y => {
                        let v = i64::from(ev.value());
                        gaze_y = (gaze_y * 4 / 5 + v).clamp(-120, 120);
                        moved = true;
                    }
                    _ => {}
                },
                EventType::ABSOLUTE => match AbsoluteAxisType(ev.code()) {
                    AbsoluteAxisType::ABS_X | AbsoluteAxisType::ABS_MT_POSITION_X => {
                        let curr = i64::from(ev.value());
                        if let Some(prev) = prev_abs_x {
                            let delta = (curr - prev).clamp(-40, 40);
                            gaze_x = (gaze_x * 4 / 5 + delta).clamp(-120, 120);
                            moved = true;
                        }
                        prev_abs_x = Some(curr);
                    }
                    AbsoluteAxisType::ABS_Y | AbsoluteAxisType::ABS_MT_POSITION_Y => {
                        let curr = i64::from(ev.value());
                        if let Some(prev) = prev_abs_y {
                            let delta = (curr - prev).clamp(-40, 40);
                            gaze_y = (gaze_y * 4 / 5 + delta).clamp(-120, 120);
                            moved = true;
                        }
                        prev_abs_y = Some(curr);
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let new_gaze = bongocat_common::mouse::gaze_direction_from_delta(gaze_x, gaze_y, 10);
        if new_gaze != last_gaze || moved {
            last_gaze = new_gaze;
            if !send_bit(write_fd, bongocat_common::mouse::gaze_to_byte(new_gaze)) {
                return;
            }
        }

        if tap_now && !send_bit(write_fd, mouse_paw_bit(paw, coin())) {
            return;
        }
    }
}

/// `PR_SET_NO_NEW_PRIVS` (requisito para seccomp sin privilegios) y
/// `PR_SET_DUMPABLE=0` (sin ptrace del mismo uid, sin core dump).
fn harden() {
    // SAFETY: `prctl` con argumentos constantes; sin efectos sobre memoria.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            eprintln!("bongocat[input]: PR_SET_NO_NEW_PRIVS falló; seccomp no se aplicará");
        }
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
}

/// Termina el proceso hijo sin desenrollar la pila (no debe ejecutar `Drop` de
/// nada del padre heredado por el `fork`).
fn exit(code: i32) -> ! {
    // SAFETY: `_exit` no regresa y es async-signal-safe.
    unsafe { libc::_exit(code) }
}

/// Instala el filtro seccomp-BPF sobre todos los hilos. Best-effort.
fn install_seccomp() {
    use seccompiler::{SeccompAction, SeccompFilter, TargetArch};
    use std::collections::BTreeMap;

    let arch = match TargetArch::try_from(std::env::consts::ARCH) {
        Ok(a) => a,
        Err(_) => {
            eprintln!(
                "bongocat[input]: seccomp no disponible en '{}'; sigo sin filtro de syscalls",
                std::env::consts::ARCH
            );
            return;
        }
    };

    // Lista blanca: solo lo que necesita el bucle de lectura en régimen normal
    // (leer de evdev, escribir el byte, dormir en reintentos, gestión de hilos
    // y del asignador, y abortar). Todo lo demás mata el proceso.
    //
    // Limitaciones conocidas (endurecer más adelante): `mmap` se permite sin
    // filtrar `PROT_EXEC`, e `ioctl` sin filtrar el `request`. Aun así ya no se
    // puede `execve`, abrir ficheros, hacer red, `ptrace` ni crear procesos.
    let allow: &[i64] = &[
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_rt_sigreturn,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigtimedwait,
        libc::SYS_futex,
        libc::SYS_nanosleep,
        libc::SYS_clock_nanosleep,
        libc::SYS_clock_gettime,
        libc::SYS_sched_yield,
        libc::SYS_restart_syscall,
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mprotect,
        libc::SYS_madvise,
        libc::SYS_brk,
        libc::SYS_getrandom,
        libc::SYS_ioctl, // evdev resincroniza con EVIOCGKEY tras SYN_DROPPED
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_tgkill, // abort() -> SIGABRT
        libc::SYS_sigaltstack,
        libc::SYS_rseq,
        libc::SYS_set_robust_list,
        libc::SYS_get_robust_list,
        libc::SYS_membarrier,
    ];

    let rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> =
        allow.iter().map(|&nr| (nr, vec![])).collect();

    let filter = match SeccompFilter::new(
        rules,
        SeccompAction::KillProcess, // syscall fuera de la lista -> muere el proceso
        SeccompAction::Allow,
        arch,
    ) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("bongocat[input]: no se pudo construir el filtro seccomp: {e}");
            return;
        }
    };

    let prog: seccompiler::BpfProgram = match filter.try_into() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bongocat[input]: no se pudo compilar el filtro seccomp: {e}");
            return;
        }
    };

    match seccompiler::apply_filter_all_threads(&prog) {
        Ok(()) => eprintln!(
            "bongocat[input]: seccomp activo ({} syscalls permitidas)",
            allow.len()
        ),
        Err(e) => eprintln!("bongocat[input]: no se pudo aplicar seccomp: {e}"),
    }
}

/// Lee del extremo de la tubería en el proceso padre y reenvía cada bit al
/// `Sender` del bucle de eventos. Corre en un hilo del padre; no toca
/// `/dev/input`, solo lee bytes ya reducidos.
pub fn parent_bridge<R: Read>(mut pipe: R, tx: calloop::channel::Sender<u8>) {
    let mut buf = [0u8; 64];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => {
                eprintln!("bongocat: el proceso lector de input terminó");
                return;
            }
            Ok(n) => {
                for &b in &buf[..n] {
                    if tx.send(b).is_err() {
                        return;
                    }
                }
            }
            Err(e) => {
                eprintln!("bongocat: fallo leyendo del proceso lector: {e}");
                return;
            }
        }
    }
}
