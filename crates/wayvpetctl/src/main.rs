//! `wayvpetctl` — cliente de configuración y control de wayvpet por línea de
//! órdenes: lee/escribe el `wayvpet.conf` conservando el formato (spec 0004) y
//! habla con la instancia en marcha por el socket IPC (spec 0003).
//!
//! Es la **plomería** para scripts y para la ventana gráfica `wayvpet-config`
//! (spec 0007); la interfaz para personas es esa ventana, no esto.

use std::path::PathBuf;
use std::process::ExitCode;

use wayvpet_common::config::{ConfDoc, Config};
use wayvpet_common::io;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
wayvpetctl — configuración de wayvpet

Uso: wayvpetctl [-c FICHERO] <orden> [args]

Órdenes (fichero):
  get CLAVE          Imprime el valor efectivo de CLAVE del fichero
  set CLAVE VALOR    Fija CLAVE=VALOR (valida el tipo; escritura atómica;
                     conserva comentarios y orden)
  dump              Imprime la configuración efectiva (ya validada) como INI
  default           Imprime la configuración por defecto como INI

Órdenes (instancia en marcha, vía socket IPC):
  ping              Comprueba que la instancia responde
  state             Imprime el estado de la instancia
  show / hide / toggle   Muestra u oculta el gato a mano
  edit [on|off|toggle]   Modo edición: arrastra el gato / rueda = tamaño; guarda al salir
  theme [list]           Lista los temas disponibles
  theme next             Pasa al siguiente tema
  theme set NOMBRE       Cambia de tema en caliente (NOMBRE o 'embedded')
  preset [list]          Lista los presets disponibles
  preset apply NOMBRE    Aplica un preset (.conf parcial) sobre la config activa
  get-live CLAVE    Lee un valor de la instancia (config viva)
  set-live CLAVE V  Cambia un valor en caliente (no toca el fichero)
  save              Persiste al .conf lo cambiado con set-live (conserva formato)
  reload            Relee el .conf en la instancia
  snapshot [RUTA]   Guarda el fotograma actual en un PNG (alias: screenshot)
  stop              Le pide a la instancia que se cierre

Opciones:
  -c, --config FICHERO   Ruta del wayvpet.conf (por defecto: autodetección XDG)
  -m, --monitor NOMBRE   Instancia de esa salida (para las órdenes IPC)
  -v, --version          Versión

La configuración visual (deslizadores, vista previa) es la ventana
`wayvpet-config` (spec 0007). Pendiente: presets y perfiles.";

struct Args {
    config: Option<PathBuf>,
    monitor: Option<String>,
    rest: Vec<String>,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut config = None;
    let mut monitor = None;
    let mut rest = Vec::new();
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-c" | "--config" => {
                config = Some(PathBuf::from(
                    it.next().ok_or("-c necesita una ruta")?.clone(),
                ));
            }
            "-m" | "--monitor" => {
                monitor = Some(it.next().ok_or("-m necesita un nombre")?.clone());
            }
            _ => rest.push(a.clone()),
        }
    }
    Ok(Args {
        config,
        monitor,
        rest,
    })
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("wayvpetctl: {e}");
            return ExitCode::from(2);
        }
    };

    match args.rest.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] | ["-h"] | ["--help"] => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        ["-v" | "--version"] => {
            println!("wayvpetctl {VERSION}");
            ExitCode::SUCCESS
        }
        ["default"] => {
            print!("{}", Config::default().to_ini());
            ExitCode::SUCCESS
        }
        ["dump"] => match io::load(args.config.as_deref()) {
            Ok(l) => {
                for w in &l.warnings {
                    eprintln!("aviso: {w}");
                }
                print!("{}", l.config.to_ini());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("wayvpetctl: {e}");
                ExitCode::from(1)
            }
        },
        ["get", key] => cmd_get(args.config, key),
        ["set", key, value] => cmd_set(args.config, key, value),
        ["ping"] => cmd_ipc(args.monitor.as_deref(), "PING", "PONG"),
        ["state"] => cmd_ipc(args.monitor.as_deref(), "STATE", ""),
        ["stop"] => cmd_ipc(args.monitor.as_deref(), "QUIT", "OK"),
        ["show"] => cmd_ipc(args.monitor.as_deref(), "SHOW", ""),
        ["hide"] => cmd_ipc(args.monitor.as_deref(), "HIDE", ""),
        ["toggle"] => cmd_ipc(args.monitor.as_deref(), "TOGGLE", ""),
        ["edit"] => cmd_ipc(args.monitor.as_deref(), "EDIT toggle", ""),
        ["edit", m @ ("on" | "off" | "toggle")] => {
            cmd_ipc(args.monitor.as_deref(), &format!("EDIT {m}"), "")
        }
        ["theme"] | ["theme", "list"] => cmd_ipc(args.monitor.as_deref(), "THEME list", ""),
        ["theme", "next"] => cmd_ipc(args.monitor.as_deref(), "THEME next", ""),
        ["theme", "set", name] => cmd_ipc(args.monitor.as_deref(), &format!("THEME {name}"), ""),
        ["theme", ..] => {
            eprintln!("wayvpetctl: uso: theme [list] | theme next | theme set NOMBRE");
            ExitCode::from(2)
        }
        ["preset"] | ["preset", "list"] => cmd_ipc(args.monitor.as_deref(), "PRESET list", ""),
        ["preset", "apply", name] => {
            cmd_ipc(args.monitor.as_deref(), &format!("PRESET {name}"), "")
        }
        ["preset", ..] => {
            eprintln!("wayvpetctl: uso: preset [list] | preset apply NOMBRE");
            ExitCode::from(2)
        }
        ["reload"] => cmd_ipc(args.monitor.as_deref(), "RELOAD", "OK"),
        ["snapshot"] | ["screenshot"] => cmd_ipc(args.monitor.as_deref(), "SNAPSHOT", "OK"),
        ["snapshot", path] | ["screenshot", path] => {
            cmd_ipc(args.monitor.as_deref(), &format!("SNAPSHOT {path}"), "OK")
        }
        ["save"] => cmd_ipc(args.monitor.as_deref(), "SAVE", ""),
        ["get-live", key] => cmd_ipc(args.monitor.as_deref(), &format!("GET {key}"), ""),
        ["set-live", key, value] => {
            cmd_ipc(args.monitor.as_deref(), &format!("SET {key} {value}"), "")
        }
        ["get-live"] | ["set-live"] | ["set-live", _] => {
            eprintln!("wayvpetctl: faltan argumentos (prueba --help)");
            ExitCode::from(2)
        }
        ["get"] | ["set"] | ["set", _] => {
            eprintln!("wayvpetctl: faltan argumentos (prueba --help)");
            ExitCode::from(2)
        }
        _ => {
            eprintln!("wayvpetctl: orden desconocida (prueba --help)");
            ExitCode::from(2)
        }
    }
}

/// Órdenes que hablan con la instancia en marcha por el socket IPC. Si
/// `expect` no está vacío, el código de salida refleja si la respuesta coincide.
fn cmd_ipc(monitor: Option<&str>, req: &str, expect: &str) -> ExitCode {
    match wayvpet_common::ipc::send_request(monitor, req) {
        Ok(reply) => {
            println!("{reply}");
            if reply.starts_with("ERR") {
                ExitCode::from(1)
            } else if expect.is_empty() || reply == expect {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("wayvpetctl: no hay respuesta de la instancia ({e})");
            ExitCode::from(1)
        }
    }
}

/// Resuelve la ruta del `.conf`: `-c`, o autodetección XDG.
fn resolve_path(explicit: Option<PathBuf>) -> Result<PathBuf, ()> {
    explicit
        .or_else(io::resolve_config_path_real)
        .ok_or_else(|| eprintln!("wayvpetctl: no encuentro wayvpet.conf; usa -c FICHERO"))
}

fn cmd_get(explicit: Option<PathBuf>, key: &str) -> ExitCode {
    let Ok(path) = resolve_path(explicit) else {
        return ExitCode::from(1);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("wayvpetctl: no se pudo leer {}: {e}", path.display());
            return ExitCode::from(1);
        }
    };
    match ConfDoc::parse(&text).get(key) {
        Some(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("wayvpetctl: '{key}' no está en {}", path.display());
            ExitCode::from(1)
        }
    }
}

fn cmd_set(explicit: Option<PathBuf>, key: &str, value: &str) -> ExitCode {
    // 1. validar tipo **y rango** antes de tocar el disco (misma tabla
    //    `field_meta` que usa la ventana `wayvpet-config`, spec 0007).
    if let Err(msg) = wayvpet_common::field_meta::validate_value(key, value) {
        eprintln!("wayvpetctl: {msg}");
        return ExitCode::from(2);
    }
    let Ok(path) = resolve_path(explicit) else {
        return ExitCode::from(1);
    };

    // 2. leer (o partir de vacío si no existe), fijar, escribir atómico.
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!("wayvpetctl: no se pudo leer {}: {e}", path.display());
            return ExitCode::from(1);
        }
    };
    let mut doc = ConfDoc::parse(&text);
    doc.set(key, value);

    if let Err(e) = io::save_atomic(&path, &doc.render()) {
        eprintln!("wayvpetctl: no se pudo escribir {}: {e}", path.display());
        return ExitCode::from(1);
    }
    eprintln!("wayvpetctl: {key}={value}  →  {}", path.display());
    ExitCode::SUCCESS
}
