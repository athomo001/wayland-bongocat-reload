//! `wayvpet-update-check` — helper del aviso de nueva versión (spec 0015).
//!
//! Tres modos:
//! - **(por defecto)** consulta la API de releases de GitHub, escribe
//!   `$XDG_STATE_HOME/wayvpet/update-check.json` de forma atómica y el aviso
//!   plano `update-notice`, y termina. Lo lanza el supervisor al arrancar.
//! - **`--print-notice`** imprime versión/fecha/URL/notas del estado ya
//!   guardado, en texto plano, para el diálogo de `wayvpet-config` (M4/M5). Sin
//!   red.
//! - **`--download [NOMBRE]`** baja el artefacto del release a `~/Descargas`,
//!   **verifica su SHA-256** y no ejecuta nada (M5). Imprime líneas
//!   `PROGRESS <hechos> <total>` y termina con `OK <ruta>` o `ERROR <msg>`.
//!
//! El modo por defecto **siempre sale con código 0**: un fallo de red se guarda
//! como `error` y no molesta. `--download` sí devuelve código ≠ 0 si falla.
//!
//! ```text
//! wayvpet-update-check                     # consulta y escribe el estado
//! wayvpet-update-check --print             # además imprime el JSON
//! wayvpet-update-check --print-notice      # texto para el diálogo
//! wayvpet-update-check --download          # baja el asset del canal
//! wayvpet-update-check --download foo.deb  # baja ese asset concreto
//! wayvpet-update-check --channel deb --download
//! wayvpet-update-check --api URL           # base alternativa (pruebas)
//! ```

use std::process::ExitCode;

use wayvpet_update::{
    download, net, read_state, state_path_real, to_json, write_notice, write_state_atomic,
    UpdateState,
};

/// Versión que se compara con la del release. Va a la par con la de `wayvpet`.
/// Versión instalada para la **comparación semver** con el release de GitHub:
/// tiene que ser un semver limpio (`3.0.0`), no un `git describe`.
const INSTALLED: &str = env!("CARGO_PKG_VERSION");
/// Versión para mostrar en `--version` (puede ser `git describe`).
const DISPLAY_VERSION: &str = env!("WAYVPET_VERSION");

const HELP: &str = "\
wayvpet-update-check — aviso de nueva versión de wayvpet (spec 0015)

USO:
    wayvpet-update-check [--print] [--api <URL>]
    wayvpet-update-check --print-notice
    wayvpet-update-check [--channel <c>] --download [<nombre-de-asset>]

OPCIONES:
    --print            Imprime también el estado (JSON) por stdout.
    --status           Una línea con el estado guardado (al día / vX disponible).
    --print-notice     Imprime versión/fecha/URL/notas del estado guardado y sale.
    --download [N]      Baja el artefacto (N, o el que toque por canal), verifica
                       su SHA-256, lo deja en ~/Descargas y NO ejecuta nada.
    --channel <c>      Canal de instalación (deb|rpm|arch|source) para --download.
    --api <URL>        Base de la API (por defecto https://api.github.com).
    -h, --help / -V, --version

No instala ninguna actualización: eso lo hace el usuario con su gestor.";

enum Mode {
    Check,
    Status,
    PrintNotice,
    Download,
}

fn main() -> ExitCode {
    let mut api = net::GITHUB_API.to_string();
    let mut print_json = false;
    let mut mode = Mode::Check;
    let mut channel: Option<String> = None;
    let mut asset: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--print" => print_json = true,
            "--status" => mode = Mode::Status,
            "--print-notice" => mode = Mode::PrintNotice,
            "--download" => mode = Mode::Download,
            "--channel" => match args.next() {
                Some(v) => channel = Some(v),
                None => return fail("--channel necesita un valor"),
            },
            "--api" => match args.next() {
                Some(v) => api = v,
                None => return fail("--api necesita una URL"),
            },
            "-h" | "--help" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("wayvpet-update-check {DISPLAY_VERSION}");
                return ExitCode::SUCCESS;
            }
            // El primer positional tras `--download` es el nombre del asset.
            other
                if !other.starts_with('-') && matches!(mode, Mode::Download) && asset.is_none() =>
            {
                asset = Some(other.to_string());
            }
            other => return fail(&format!("argumento desconocido: {other}")),
        }
    }

    match mode {
        Mode::Check => run_check(&api, print_json),
        Mode::Status => run_status(),
        Mode::PrintNotice => run_print_notice(),
        Mode::Download => run_download(channel.as_deref(), asset.as_deref()),
    }
}

/// Una línea con el estado del `update-check.json`, sin tocar la red. Para
/// `wayvpetctl update` y scripts. Nunca falla: sin fichero → "sin datos".
fn run_status() -> ExitCode {
    let state = state_path_real()
        .and_then(|p| read_state(&p).ok().flatten())
        .unwrap_or_default();
    println!("{}", state.status_line(INSTALLED));
    ExitCode::SUCCESS
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("wayvpet-update-check: {msg}");
    ExitCode::FAILURE
}

fn run_check(api: &str, print_json: bool) -> ExitCode {
    let state = net::check(api, INSTALLED);
    if print_json {
        print!("{}", to_json(&state));
    }
    let Some(path) = state_path_real() else {
        return fail("sin $HOME ni $XDG_STATE_HOME; no escribo el estado");
    };
    if let Err(e) = write_state_atomic(&path, &state) {
        eprintln!(
            "wayvpet-update-check: no pude escribir {}: {e}",
            path.display()
        );
    }
    if let Err(e) = write_notice(&path, &state, INSTALLED) {
        eprintln!("wayvpet-update-check: no pude escribir el aviso: {e}");
    }
    ExitCode::SUCCESS
}

fn load_state() -> Result<UpdateState, ExitCode> {
    let path = state_path_real().ok_or_else(|| fail("sin $HOME ni $XDG_STATE_HOME"))?;
    match read_state(&path) {
        Ok(Some(s)) => Ok(s),
        Ok(None) => Err(fail("no hay estado todavía; corre el chequeo primero")),
        Err(e) => Err(fail(&format!("estado ilegible: {e}"))),
    }
}

fn run_print_notice() -> ExitCode {
    let state = match load_state() {
        Ok(s) => s,
        Err(code) => return code,
    };
    if !state.update_available(INSTALLED) {
        println!("al día");
        return ExitCode::SUCCESS;
    }
    // Formato simple para el diálogo: cabecera `clave=valor`, luego `---`, luego
    // las notas en crudo (ya recortadas por el chequeo).
    println!("version={}", state.latest.as_deref().unwrap_or(""));
    println!("date={}", state.date.as_deref().unwrap_or(""));
    println!("url={}", state.url.as_deref().unwrap_or(""));
    println!("---");
    if let Some(notes) = &state.notes {
        print!("{notes}");
        if !notes.ends_with('\n') {
            println!();
        }
    }
    ExitCode::SUCCESS
}

fn run_download(channel: Option<&str>, asset: Option<&str>) -> ExitCode {
    let state = match load_state() {
        Ok(s) => s,
        Err(code) => return code,
    };
    // Progreso por stdout: el diálogo lo parsea para la barra.
    let on_progress = |done: u64, total: Option<u64>| {
        println!("PROGRESS {done} {}", total.unwrap_or(0));
    };
    match download::run(&state, channel, asset, on_progress) {
        Ok(path) => {
            println!("OK {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            // Una línea por si el diálogo la enseña; multilinea al stderr.
            println!("ERROR {}", e.lines().next().unwrap_or("falló la descarga"));
            eprintln!("wayvpet-update-check: {e}");
            ExitCode::FAILURE
        }
    }
}
