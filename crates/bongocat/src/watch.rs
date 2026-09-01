//! Vigilancia del `bongocat.conf` para `--watch-config`.
//!
//! Un hilo con `inotify` (como el `config_watcher_thread` del C): cuando el
//! fichero cambia, manda un `()` al bucle, que recarga con debounce. Re-arma la
//! vigilancia si el editor reemplaza el fichero por `rename`.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use calloop::channel::Sender;
use inotify::{EventMask, Inotify, WatchMask};

/// Lanza el hilo vigía. `tx` recibe un `()` por cada cambio relevante.
pub fn spawn(path: PathBuf, tx: Sender<()>) {
    let _ = thread::Builder::new()
        .name("watch:config".into())
        .spawn(move || run(&path, &tx));
}

fn run(path: &PathBuf, tx: &Sender<()>) {
    let mask = WatchMask::CLOSE_WRITE
        | WatchMask::MODIFY
        | WatchMask::MOVED_TO
        | WatchMask::ATTRIB
        | WatchMask::MOVE_SELF
        | WatchMask::DELETE_SELF;

    let mut ino = match Inotify::init() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("bongocat: no se pudo iniciar inotify: {e}");
            return;
        }
    };
    if ino.watches().add(path, mask).is_err() {
        eprintln!("bongocat: no se pudo vigilar {}", path.display());
        return;
    }
    eprintln!("bongocat: vigilando {}", path.display());

    let mut buf = [0u8; 4096];
    loop {
        let events = match ino.read_events_blocking(&mut buf) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("bongocat: inotify dejó de leer: {e}");
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
