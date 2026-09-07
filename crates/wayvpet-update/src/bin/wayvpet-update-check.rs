//! `wayvpet-update-check` — proceso hijo corto del aviso de nueva versión
//! (spec 0015). Hace UNA consulta a la API de releases de GitHub, escribe
//! `$XDG_STATE_HOME/wayvpet/update-check.json` de forma atómica y termina. El
//! supervisor lo lanza al arrancar (si `check_updates=1`) y no lo espera.
//!
//! **Siempre sale con código 0 en el camino normal**: un fallo de red o de
//! parseo se guarda como `error` en el fichero y no molesta a nadie. Solo un
//! argumento inválido o la falta total de `$HOME`/`$XDG_STATE_HOME` dan código
//! ≠ 0 (son errores de invocación, no de red).
//!
//! Uso interno; se puede correr a mano para depurar:
//! ```text
//! wayvpet-update-check              # consulta api.github.com y escribe el estado
//! wayvpet-update-check --print      # además imprime el estado por stdout
//! wayvpet-update-check --api URL    # apunta a otra base (pruebas)
//! ```

use std::process::ExitCode;

use wayvpet_update::{net, state_path_real, to_json, write_state_atomic};

/// Versión que se compara con la del release. Es la del workspace, que va a la
/// par con la de `wayvpet`.
const INSTALLED: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
wayvpet-update-check — comprueba si hay una versión nueva de wayvpet (spec 0015)

USO:
    wayvpet-update-check [--print] [--api <URL>]

OPCIONES:
    --print        Imprime también el estado resultante (JSON) por stdout.
    --api <URL>    Base de la API a consultar (por defecto https://api.github.com).
                   Pensado para pruebas contra un servidor local.
    -h, --help     Esta ayuda.
    -V, --version  Versión.

Escribe $XDG_STATE_HOME/wayvpet/update-check.json y termina. No instala nada.";

fn main() -> ExitCode {
    let mut api = net::GITHUB_API.to_string();
    let mut print = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--print" => print = true,
            "--api" => match args.next() {
                Some(v) => api = v,
                None => {
                    eprintln!("wayvpet-update-check: --api necesita una URL");
                    return ExitCode::FAILURE;
                }
            },
            "-h" | "--help" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("wayvpet-update-check {INSTALLED}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("wayvpet-update-check: argumento desconocido: {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let state = net::check(&api, INSTALLED);
    if print {
        print!("{}", to_json(&state));
    }

    let Some(path) = state_path_real() else {
        eprintln!("wayvpet-update-check: sin $HOME ni $XDG_STATE_HOME; no escribo el estado");
        return ExitCode::FAILURE;
    };
    if let Err(e) = write_state_atomic(&path, &state) {
        // El estado sí se calculó; solo no se pudo persistir. Aviso y salgo 0:
        // el supervisor no debe tratar esto como algo urgente.
        eprintln!(
            "wayvpet-update-check: no pude escribir {}: {e}",
            path.display()
        );
    }
    ExitCode::SUCCESS
}
