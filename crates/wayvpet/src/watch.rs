//! Vigilancia con `inotify` para `--watch-config`.
//!
//! Un hilo por objetivo (como el `config_watcher_thread` del C): cuando algo
//! cambia, manda un `()` al bucle, que reacciona con debounce. Re-arma la
//! vigilancia si el editor reemplaza el fichero/directorio por `rename`.
//!
//! - [`spawn`]: el `wayvpet.conf`.
//! - [`spawn_dir`]: el **directorio del tema activo** (spec 0006 M6) — editar un
//!   SVG / PNG del tema con `-w` lo recarga de disco en caliente.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use calloop::channel::Sender;
use inotify::{EventMask, Inotify, WatchMask};

/// Lanza el hilo vigía del fichero `path`. `tx` recibe un `()` por cada cambio.
pub fn spawn(path: PathBuf, tx: Sender<()>) {
    let mask = WatchMask::CLOSE_WRITE
        | WatchMask::MODIFY
        | WatchMask::MOVED_TO
        | WatchMask::ATTRIB
        | WatchMask::MOVE_SELF
        | WatchMask::DELETE_SELF;
    let _ = thread::Builder::new()
        .name("watch:config".into())
        .spawn(move || run(&path, mask, &tx));
}

/// Lanza el hilo vigía del **directorio** `dir` (el del tema). Cualquier alta,
/// baja o reescritura de un fichero dentro dispara un `()`.
pub fn spawn_dir(dir: PathBuf, tx: Sender<()>) {
    let mask = WatchMask::CLOSE_WRITE
        | WatchMask::CREATE
        | WatchMask::MOVED_TO
        | WatchMask::MOVED_FROM
        | WatchMask::DELETE
        | WatchMask::MOVE_SELF
        | WatchMask::DELETE_SELF;
    let _ = thread::Builder::new()
        .name("watch:theme".into())
        .spawn(move || run(&dir, mask, &tx));
}

fn run(path: &PathBuf, mask: WatchMask, tx: &Sender<()>) {
    let mut ino = match Inotify::init() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("wayvpet: no se pudo iniciar inotify: {e}");
            return;
        }
    };
    if ino.watches().add(path, mask).is_err() {
        eprintln!("wayvpet: no se pudo vigilar {}", path.display());
        return;
    }
    eprintln!("wayvpet: vigilando {}", path.display());

    let mut buf = [0u8; 4096];
    loop {
        let events = match ino.read_events_blocking(&mut buf) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("wayvpet: inotify dejó de leer: {e}");
                return;
            }
        };

        let mut changed = false;
        let mut lost_watch = false;
        for ev in events {
            changed = true;
            if ev
                .mask
                .intersects(EventMask::MOVE_SELF | EventMask::DELETE_SELF | EventMask::IGNORED)
            {
                lost_watch = true;
            }
        }

        // El editor reemplazó el fichero (write + rename): re-vigilar el inodo nuevo.
        if lost_watch {
            for _ in 0..20 {
                if ino.watches().add(path, mask).is_ok() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }

        if changed && tx.send(()).is_err() {
            return; // el bucle se cerró
        }
    }
}
