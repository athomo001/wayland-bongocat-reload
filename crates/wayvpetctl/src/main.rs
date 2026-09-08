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

const VERSION: &str = env!("WAYVPET_VERSION");

const HELP: &str = "\
wayvpetctl — configuración de wayvpet

Uso: wayvpetctl [-c FICHERO] <orden> [args]

Órdenes (fichero):
  get CLAVE          Imprime el valor efectivo de CLAVE del fichero
                     (con -m NOMBRE: el de la sección [monitor:NOMBRE], si tiene)
  set CLAVE VALOR    Fija CLAVE=VALOR (valida el tipo; escritura atómica;
                     conserva comentarios y orden)
                     (con -m NOMBRE: escribe en la sección [monitor:NOMBRE])
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
  profile [list]         Lista los perfiles guardados (marca el activo con *)
  profile save NOMBRE    Guarda la config actual como perfil NOMBRE
  profile switch NOMBRE  Cambia a ese perfil (copia su .conf y recarga)
  get-live CLAVE    Lee un valor de la instancia (config viva)
  set-live CLAVE V  Cambia un valor en caliente (no toca el fichero)
  save              Persiste al .conf lo cambiado con set-live (conserva formato)
  reload            Relee el .conf en la instancia
  snapshot [RUTA]   Guarda el fotograma actual en un PNG (alias: screenshot)
  stop              Le pide a la instancia que se cierre

Órdenes (aviso de nueva versión, spec 0015; necesita el paquete wayvpet-update):
  update            Imprime el estado guardado sin tocar la red
  update --check    Fuerza una consulta a GitHub ahora y luego imprime el estado

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
        ["get", key] => cmd_get(args.config, key, args.monitor.as_deref()),
        ["set", key, value] => cmd_set(args.config, key, value, args.monitor.as_deref()),
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
        ["profile"] | ["profile", "list"] => cmd_ipc(args.monitor.as_deref(), "PROFILE list", ""),
        ["profile", "active"] => cmd_ipc(args.monitor.as_deref(), "PROFILE active", ""),
        ["profile", sub @ ("save" | "switch"), name] => cmd_ipc(
            args.monitor.as_deref(),
            &format!("PROFILE {sub} {name}"),
            "",
        ),
        ["profile", ..] => {
            eprintln!(
                "wayvpetctl: uso: profile [list] | profile save NOMBRE | profile switch NOMBRE"
            );
            ExitCode::from(2)
        }
        ["update"] => cmd_update(&[]),
        ["update", "--check"] | ["update", "check"] => cmd_update(&["--check"]),
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

/// `wayvpetctl update [--check]` (spec 0015 M6). Sin red: delega en el helper
/// `wayvpet-update-check`, que lee el `update-check.json` guardado. `--check`
/// fuerza una consulta nueva a GitHub (proceso hijo corto). Si el helper no
/// está instalado (paquete `wayvpet-update`), lo dice y sale con código 1.
fn cmd_update(extra: &[&str]) -> ExitCode {
    let helper = "wayvpet-update-check";
    let mut cmd = std::process::Command::new(helper);
    if extra.contains(&"--check") {
        // Lanza el chequeo (escribe el estado) y luego imprime cómo quedó.
        match cmd.status() {
            Ok(s) if s.success() => {}
            Ok(_) | Err(_) => {
                eprintln!("wayvpetctl: no pude ejecutar «{helper}» (¿instalado el paquete wayvpet-update?)");
                return ExitCode::from(1);
            }
        }
        return cmd_update(&[]);
    }
    match std::process::Command::new(helper).arg("--status").output() {
        Ok(out) => {
            print!("{}", String::from_utf8_lossy(&out.stdout));
            if out.status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(_) => {
            eprintln!(
                "wayvpetctl: «{helper}» no está instalado; el aviso de nueva versión \
                 viene en el paquete «wayvpet-update»"
            );
            ExitCode::from(1)
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

fn cmd_get(explicit: Option<PathBuf>, key: &str, monitor: Option<&str>) -> ExitCode {
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
    let doc = ConfDoc::parse(&text);
    // `-m NOMBRE`: valor efectivo para esa salida = sección si la tiene, si no la
    // base (spec 0008 §8.4).
    let value = monitor
        .and_then(|m| doc.get_section(m, key))
        .or_else(|| doc.get(key));
    match value {
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

fn cmd_set(explicit: Option<PathBuf>, key: &str, value: &str, monitor: Option<&str>) -> ExitCode {
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
    // `-m NOMBRE` dirige la escritura a la sección `[monitor:NOMBRE]` (spec 0008
    // §8.4); sin `-m`, a la base.
    match monitor {
        Some(name) => doc.set_section(name, key, value),
        None => doc.set(key, value),
    }

    if let Err(e) = io::save_atomic(&path, &doc.render()) {
        eprintln!("wayvpetctl: no se pudo escribir {}: {e}", path.display());
        return ExitCode::from(1);
    }
    let dest = monitor.map_or_else(String::new, |m| format!(" [monitor:{m}]"));
    eprintln!("wayvpetctl: {key}={value}{dest}  →  {}", path.display());
    ExitCode::SUCCESS
}
