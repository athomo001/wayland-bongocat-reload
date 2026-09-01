//! Punto de entrada del overlay `bongocat` (reescritura en Rust, Fase 0.5).
//!
//! Portado: CLI, carga de configuración, fichero PID, `--toggle`, y el overlay
//! (Wayland con SCTK, bucle `calloop`, rasterizado SVG, lector de teclado).
//! Pendiente: HiDPI, multi-monitor, auto-ocultar en fullscreen, `--watch-config`.

use std::path::PathBuf;
use std::process::ExitCode;

use bongocat_common::config::Config;
use bongocat_common::io;

mod anim;
mod input;
mod pidfile;
mod toggle;
mod wl;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default)]
struct Args {
    config: Option<PathBuf>,
    monitor: Option<String>,
    watch_config: bool,
    toggle: bool,
    multi_monitor_child: bool,
    // Utilidades sin compositor:
    validate: bool,
    print_default_config: bool,
    dry_run: bool,
}

fn print_help(prog: &str) {
    println!(
        "Bongo Cat — overlay para Wayland (v{VERSION})\n\
         Uso: {prog} [opciones]\n\n\
         Opciones:\n\
         \x20 -h, --help                 Muestra esta ayuda\n\
         \x20 -v, --version              Muestra la versión\n\
         \x20 -c, --config FICHERO       Ruta del bongocat.conf (auto-detecta si se omite)\n\
         \x20 -w, --watch-config         Recarga al cambiar la configuración\n\
         \x20 -t, --toggle               Arranca / para\n\
         \x20 -m, --monitor NOMBRE       Fuerza una salida de monitor\n\
         \x20     --validate             Valida la configuración y sale\n\
         \x20     --print-default-config Imprime la configuración por defecto como INI\n\
         \x20     --dry-run              Resuelve config + tema sin abrir Wayland y sale\n"
    );
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    let prog = it.next().cloned().unwrap_or_else(|| "bongocat".into());
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help(&prog);
                std::process::exit(0);
            }
            "-v" | "--version" => {
                println!("bongocat {VERSION}");
                std::process::exit(0);
            }
            "-c" | "--config" => {
                a.config = Some(PathBuf::from(
                    it.next().ok_or("--config necesita una ruta")?,
                ));
            }
            "-m" | "--monitor" => {
                a.monitor = Some(it.next().ok_or("--monitor necesita un nombre")?.clone());
            }
            "-w" | "--watch-config" => a.watch_config = true,
            "-t" | "--toggle" => a.toggle = true,
            "--multi-monitor-child" => a.multi_monitor_child = true,
            "--validate" => a.validate = true,
            "--print-default-config" => a.print_default_config = true,
            "--dry-run" => a.dry_run = true,
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bongocat: {e}");
            return ExitCode::from(2);
        }
    };

    if args.print_default_config {
        print!("{}", Config::default().to_ini());
        return ExitCode::SUCCESS;
    }

    // Carga de configuración (portada; sin escaneo de /dev/input por nombre).
    let loaded = match io::load(args.config.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bongocat: no se pudo leer la configuración: {e}");
            return ExitCode::from(1);
        }
    };
    for w in &loaded.warnings {
        eprintln!("bongocat: aviso: {w}");
    }

    if args.validate {
        match &loaded.path {
            Some(p) => println!("configuración válida: {}", p.display()),
            None => println!("sin fichero de configuración; se usarían los valores por defecto"),
        }
        return ExitCode::SUCCESS;
    }

    if args.dry_run {
        println!(
            "dry-run: overlay {}x{}, gato alto {}, fps {}, teclados {:?}",
            loaded.config.screen_width,
            loaded.config.overlay_height,
            loaded.config.cat_height,
            loaded.config.fps,
            loaded.config.keyboard_devices,
        );
        return ExitCode::SUCCESS;
    }

    // --toggle: si hay una instancia, la para y salimos; si no, seguimos.
    if args.toggle {
        match toggle::run() {
            toggle::Outcome::Stopped => return ExitCode::SUCCESS,
            toggle::Outcome::NotRunning => {}
        }
    }

    // Fichero PID: garantiza una sola instancia. Vive hasta el final de `main`.
    let _pid = match pidfile::PidFile::acquire() {
        Ok(pidfile::Acquire::Ok(p)) => p,
        Ok(pidfile::Acquire::AlreadyRunning) => {
            eprintln!("bongocat: ya hay otra instancia corriendo");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("bongocat: no se pudo crear el fichero PID: {e}");
            return ExitCode::from(1);
        }
    };

    // Pendiente (F19+): multi-monitor y --watch-config.
    let _ = (args.watch_config, args.monitor, args.multi_monitor_child);
    match wl::run_overlay(&loaded.config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bongocat: error de Wayland: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Result<Args, String> {
        let mut owned = vec!["bongocat".to_string()];
        owned.extend(v.iter().map(|s| s.to_string()));
        parse_args(&owned)
    }

    #[test]
    fn parsea_config_y_monitor() {
        let a = args(&["-c", "/tmp/x.conf", "--monitor", "eDP-1", "-w"]).unwrap();
        assert_eq!(a.config, Some(PathBuf::from("/tmp/x.conf")));
        assert_eq!(a.monitor.as_deref(), Some("eDP-1"));
        assert!(a.watch_config);
    }

    #[test]
    fn flags_de_utilidad() {
        assert!(args(&["--validate"]).unwrap().validate);
        assert!(args(&["--dry-run"]).unwrap().dry_run);
        assert!(
            args(&["--print-default-config"])
                .unwrap()
                .print_default_config
        );
    }

    #[test]
    fn argumento_desconocido_es_error() {
        assert!(args(&["--nope"]).is_err());
        assert!(args(&["-c"]).is_err(), "-c sin valor");
    }
}
