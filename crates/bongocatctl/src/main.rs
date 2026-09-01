//! `bongocatctl` — cliente de configuración y control de bongocat.
//!
//! Estado: esqueleto. El grueso (cliente IPC, TUI de configuración con
//! `ratatui`) llega en fases posteriores. Por ahora sirve para inspeccionar la
//! configuración sin arrancar el overlay.

use std::process::ExitCode;

use bongocat_common::config::Config;
use bongocat_common::io;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let cmd = argv.get(1).map(String::as_str);

    match cmd {
        Some("-h") | Some("--help") | None => {
            println!(
                "bongocatctl {VERSION} — configuración de bongocat\n\n\
                 Uso: bongocatctl <orden>\n\n\
                 Órdenes disponibles ahora:\n\
                 \x20 dump           Imprime la configuración efectiva como INI\n\
                 \x20 default        Imprime la configuración por defecto como INI\n\
                 \x20 -v, --version  Versión\n\n\
                 Pendiente (fases posteriores): get/set en vivo, TUI, temas, presets."
            );
            ExitCode::SUCCESS
        }
        Some("-v") | Some("--version") => {
            println!("bongocatctl {VERSION}");
            ExitCode::SUCCESS
        }
        Some("default") => {
            print!("{}", Config::default().to_ini());
            ExitCode::SUCCESS
        }
        Some("dump") => match io::load(None) {
            Ok(l) => {
                for w in &l.warnings {
                    eprintln!("aviso: {w}");
                }
                print!("{}", l.config.to_ini());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("bongocatctl: {e}");
                ExitCode::from(1)
            }
        },
        Some(other) => {
            eprintln!("bongocatctl: orden desconocida '{other}' (prueba --help)");
            ExitCode::from(2)
        }
    }
}
