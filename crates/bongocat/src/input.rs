//! Lector de teclado.
//!
//! Lee `/dev/input/event*` con `evdev` y, por cada pulsación, **reduce el
//! keycode a un bit de pata (`PAW_LEFT`/`PAW_RIGHT`) y lo descarta**: solo ese
//! bit cruza el canal hacia el bucle de eventos. El identificador de la tecla no
//! se guarda, registra ni transmite (spec 0013 §1).
//!
//! TODO(0013 §2): mover esto a un **proceso** aparte, con seccomp, `zeroize` del
//! búfer y `PR_SET_DUMPABLE(0)`. Por ahora es un hilo por dispositivo para poder
//! iterar; la garantía de "no se sabe qué tecla" ya se cumple.

use std::path::PathBuf;
use std::thread;

use bongocat_common::paw::paw_for_keycode;
use calloop::channel::Sender;
use evdev::{Device, EventType, Key};

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

/// Abre los dispositivos y lanza un hilo lector por cada uno.
///
/// Estrategia: se intentan los de la configuración; si ninguno parece un
/// teclado (o no hay), se usan los detectados automáticamente. Así el gato
/// reacciona aunque el `keyboard_device` del `.conf` esté mal.
pub fn spawn_readers(configured: &[String], tx: &Sender<u8>) {
    let detected = detect_keyboards();
    if detected.is_empty() {
        eprintln!("bongocat: no se detectó ningún teclado en /dev/input (¿grupo 'input'?)");
    } else {
        eprintln!("bongocat: teclados detectados:");
        for (p, n) in &detected {
            eprintln!("           {}  —  {n}", p.display());
        }
    }

    let mut opened_a_keyboard = false;
    for path in configured {
        match Device::open(path) {
            Ok(dev) => {
                let kb = looks_like_keyboard(&dev);
                if kb {
                    opened_a_keyboard = true;
                } else {
                    eprintln!("bongocat: {path} no parece un teclado; se intentará igual");
                }
                start_reader(dev, path.clone(), tx);
                eprintln!("bongocat: escuchando {path}");
            }
            Err(e) => {
                eprintln!("bongocat: no se pudo abrir {path}: {e} (¿estás en el grupo 'input'?)");
            }
        }
    }

    if !opened_a_keyboard {
        for (p, _) in detected {
            let ps = p.display().to_string();
            match Device::open(&p) {
                Ok(dev) => {
                    start_reader(dev, ps.clone(), tx);
                    eprintln!("bongocat: escuchando (auto) {ps}");
                }
                Err(e) => eprintln!("bongocat: no se pudo abrir {ps}: {e}"),
            }
        }
    }
}

fn start_reader(dev: Device, path: String, tx: &Sender<u8>) {
    let tx = tx.clone();
    let _ = thread::Builder::new()
        .name(format!("input:{path}"))
        .spawn(move || read_loop(dev, &path, &tx));
}

fn read_loop(mut dev: Device, path: &str, tx: &Sender<u8>) {
    loop {
        let batch = match dev.fetch_events() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("bongocat: {path} dejó de leer: {e}");
                return;
            }
        };
        for ev in batch {
            // Solo key-down.
            if ev.event_type() == EventType::KEY && ev.value() == 1 {
                // Reducción inmediata e irreversible: keycode -> 1 bit.
                // Nada de logs por pulsación: filtraría el ritmo de tecleo.
                let bit = paw_for_keycode(i32::from(ev.code()));
                if tx.send(bit).is_err() {
                    return; // el bucle de eventos se cerró
                }
            }
        }
    }
}
