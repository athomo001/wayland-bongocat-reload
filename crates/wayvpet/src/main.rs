//! Punto de entrada del overlay `wayvpet` (reescritura en Rust, Fase 0.5).
//!
//! Portado: CLI, carga de configuración, fichero PID (por salida), `--toggle`,
//! `--watch-config`, `--monitor` + una instancia por salida, el overlay (Wayland
//! con SCTK, bucle `calloop`, rasterizado SVG, HiDPI), el lector de teclado en
//! proceso aislado con seccomp (0013 §2) y el auto-ocultar en pantalla completa
//! (protocolos wlr y COSMIC).

use std::path::PathBuf;
use std::process::ExitCode;

use wayvpet_common::config::{self, Config};
use wayvpet_common::io;

mod anim;
mod cosmic;
mod import_vpets;
mod input;
mod input_child;
mod ipc;
mod kpm;
mod pidfile;
mod png_decode;
mod roam;
mod service;
mod sheet_anim;
mod theme;
mod toggle;
mod tray;
mod update_check;
mod watch;
mod wl;

const VERSION: &str = env!("WAYVPET_VERSION");

#[derive(Debug, Default)]
struct Args {
    config: Option<PathBuf>,
    monitor: Option<String>,
    theme: Option<String>,
    watch_config: bool,
    toggle: bool,
    supervise: bool,
    multi_monitor_child: bool,
    /// Esta instancia es la "primaria" del grupo multi-monitor: monta el tray y
    /// además enlaza `wayvpet.sock` (sin sufijo de monitor).
    multi_primary: bool,
    /// No conectar a ningún protocolo de toplevels (deshabilita el auto-ocultar
    /// en pantalla completa). Escotilla por si el protocolo del compositor da
    /// problemas.
    no_toplevel: bool,
    /// Fuerza `enable_tray=0` para esta ejecución (spec 0011 §5).
    no_tray: bool,
    // Utilidades sin compositor:
    validate: bool,
    print_default_config: bool,
    dry_run: bool,
    install_service: bool,
    uninstall_service: bool,
}

fn print_help(prog: &str) {
    println!(
        "wayvpet — overlay para Wayland (v{VERSION})\n\
         Uso: {prog} [opciones]\n\n\
         Opciones:\n\
         \x20 -h, --help                 Muestra esta ayuda\n\
         \x20 -v, --version              Muestra la versión\n\
         \x20 -c, --config FICHERO       Ruta del wayvpet.conf (auto-detecta si se omite)\n\
         \x20 -T, --theme NOMBRE         Fuerza el tema a cargar (ej: gabumon, miku)\n\
         \x20 -w, --watch-config         Recarga al cambiar la configuración\n\
         \x20 -t, --toggle               Arranca / para\n\
         \x20 -S, --supervise            Relanza el overlay si sale con error\n\
         \x20 -m, --monitor NOMBRE       Fuerza una salida de monitor\n\
         \x20     --validate             Valida la configuración y sale\n\
         \x20     --print-default-config Imprime la configuración por defecto como INI\n\
         \x20     --dry-run              Resuelve config + tema sin abrir Wayland y sale\n\
         \x20     --no-toplevel          No usar protocolos de toplevels (sin auto-ocultar)\n\
         \x20     --no-tray              No arrancar el icono de bandeja\n\
         \x20     --install-service      Instala la unidad systemd de usuario y sale\n\
         \x20     --uninstall-service    Quita la unidad systemd de usuario y sale\n"
    );
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    let prog = it.next().cloned().unwrap_or_else(|| "wayvpet".into());
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help(&prog);
                std::process::exit(0);
            }
            "-v" | "--version" => {
                println!("wayvpet {VERSION}");
                std::process::exit(0);
            }
            "-c" | "--config" => {
                a.config = Some(PathBuf::from(
                    it.next().ok_or("--config necesita una ruta")?,
                ));
            }
            "-T" | "--theme" => {
                a.theme = Some(it.next().ok_or("--theme necesita un nombre")?.clone());
            }
            "-m" | "--monitor" => {
                a.monitor = Some(it.next().ok_or("--monitor necesita un nombre")?.clone());
            }
            "-w" | "--watch-config" => a.watch_config = true,
            "-t" | "--toggle" => a.toggle = true,
            "-S" | "--supervise" => a.supervise = true,
            "--multi-monitor-child" => a.multi_monitor_child = true,
            "--multi-primary" => a.multi_primary = true,
            "--no-toplevel" => a.no_toplevel = true,
            "--no-tray" => a.no_tray = true,
            "--validate" => a.validate = true,
            "--print-default-config" => a.print_default_config = true,
            "--dry-run" => a.dry_run = true,
            "--install-service" => a.install_service = true,
            "--uninstall-service" => a.uninstall_service = true,
            other => return Err(format!("argumento desconocido: {other}")),
        }
    }
    Ok(a)
}

/// Subcomando `wayvpet theme new|check ...` (spec 0006 M7). Se resuelve antes
/// del parseo de flags porque tiene su propia forma.
fn theme_subcommand(argv: &[String]) -> Option<ExitCode> {
    if argv.get(1).map(String::as_str) != Some("theme") {
        return None;
    }
    if argv.get(2).map(String::as_str) == Some("import-vpets") {
        return Some(import_vpets_cmd(&argv[3..]));
    }
    let rc = match (argv.get(2).map(String::as_str), argv.get(3)) {
        (Some("new"), Some(name)) => match theme::scaffold(name) {
            Ok(dir) => {
                println!("wayvpet: tema creado en {}", dir.display());
                println!("Edita los SVG y pruébalo:  wayvpet -c <conf>   (con theme={name})");
                println!("Valídalo:                  wayvpet theme check {name}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("wayvpet: theme new: {e}");
                ExitCode::from(1)
            }
        },
        (Some("check"), Some(spec)) => {
            if theme::check(spec) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        (Some("list"), _) => {
            for n in theme::list() {
                println!("{n}");
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "wayvpet: uso: wayvpet theme new NOMBRE | theme check NOMBRE|RUTA | theme list\n\
                 \x20                  | theme import-vpets ORIGEN [--name N] [--out DIR] [--dry-run]\n\
                 \x20                    [--frame-w W] [--frame-h H]\n\
                 \x20                    [--state NOMBRE]            (APNG suelto → un estado)\n\
                 \x20                    [--state N=row:R,frames:F]  (hoja PNG suelta, repetible)"
            );
            ExitCode::from(2)
        }
    };
    Some(rc)
}

/// `wayvpet theme import-vpets ORIGEN [flags]` (spec 0014 M4). Parsea sus
/// propios flags porque no encajan con el parser global.
fn import_vpets_cmd(rest: &[String]) -> ExitCode {
    let mut source: Option<&str> = None;
    let mut name: Option<&str> = None;
    let mut states: Vec<String> = Vec::new();
    let mut out_dir: Option<std::path::PathBuf> = None;
    let mut dry_run = false;
    let (mut frame_w, mut frame_h) = (None, None);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry-run" => dry_run = true,
            "--name" => name = it.next().map(String::as_str),
            "--state" => {
                if let Some(v) = it.next() {
                    states.push(v.clone());
                }
            }
            "--out" => out_dir = it.next().map(std::path::PathBuf::from),
            "--frame-w" => frame_w = it.next().and_then(|s| s.parse().ok()),
            "--frame-h" => frame_h = it.next().and_then(|s| s.parse().ok()),
            other if other.starts_with('-') => {
                eprintln!("wayvpet: theme import-vpets: flag desconocido: {other}");
                return ExitCode::from(2);
            }
            other => source = Some(other),
        }
    }
    let Some(source) = source else {
        eprintln!("wayvpet: theme import-vpets: falta ORIGEN (carpeta, .conf o .png)");
        return ExitCode::from(2);
    };
    let args = import_vpets::ImportArgs {
        source,
        name,
        out_dir,
        dry_run,
        frame_w,
        frame_h,
        states: &states,
    };
    match import_vpets::run(&args) {
        Ok(dir) => {
            if dry_run {
                println!("wayvpet: revisión completada (no se ha escrito nada)");
            } else {
                println!("wayvpet: tema importado en {}", dir.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("wayvpet: theme import-vpets: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(rc) = theme_subcommand(&argv) {
        return rc;
    }
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("wayvpet: {e}");
            return ExitCode::from(2);
        }
    };

    if args.print_default_config {
        print!("{}", Config::default().to_ini());
        return ExitCode::SUCCESS;
    }
    if args.install_service {
        return service::install();
    }
    if args.uninstall_service {
        return service::uninstall();
    }

    // Carga de configuración (portada; sin escaneo de /dev/input por nombre).
    let mut loaded = match io::load(args.config.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("wayvpet: no se pudo leer la configuración: {e}");
            return ExitCode::from(1);
        }
    };
    if let Some(ref t) = args.theme {
        loaded.config.theme = t.clone();
    }
    for w in &loaded.warnings {
        eprintln!("wayvpet: aviso: {w}");
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
        match theme::resolve(&loaded.config.theme) {
            Some(t) => println!("dry-run: tema {} desde {}", t.describe(), t.dir.display()),
            None => println!("dry-run: tema = embebido (classic)"),
        }
        println!("dry-run: temas disponibles: {:?}", theme::list());
        println!(
            "dry-run: tray = {}",
            if tray::wanted(loaded.config.enable_tray, args.no_tray) {
                "activado"
            } else {
                "desactivado"
            }
        );
        return ExitCode::SUCCESS;
    }

    // --supervise: envoltorio delgado que relanza el overlay si sale con error
    // (autoarranque sin systemd). Ctrl+C / salida limpia (código 0) lo detiene.
    if args.supervise {
        return supervise(&argv);
    }

    // Multi-monitor: una instancia hija por salida (una superficie de layer-shell
    // se ata a una salida de por vida). Se reparte cuando:
    //   - `monitor=` lista ≥2 salidas, o
    //   - `monitor=` está vacío pero el tema activo **pasea** (`can_roam`) y hay
    //     ≥2 pantallas conectadas → el gato podrá cruzar entre ellas (multi-head).
    if args.monitor.is_none() && !args.multi_monitor_child {
        let mut names = loaded.config.output_names.clone();
        if names.is_empty()
            && loaded.config.roam
            && theme::resolve(&loaded.config.theme).is_some_and(|t| t.can_roam())
        {
            let connected = wl::connected_outputs();
            if connected.len() >= 2 {
                eprintln!(
                    "wayvpet: tema con paseo y {} pantallas: una instancia por salida",
                    connected.len()
                );
                names = connected;
            }
        }
        if names.len() > 1 {
            return spawn_per_monitor(&args, &loaded, &names);
        }
    }

    // Salida objetivo: `--monitor` gana; si no, la primera de `monitor=`.
    let target = args
        .monitor
        .clone()
        .or_else(|| loaded.config.output_name.clone());

    // Config por monitor (spec 0008 §8.4): esta instancia conoce su salida, así
    // que aplica `base + [monitor:SU_NOMBRE]` encima.
    if let Some(name) = target.as_deref() {
        for w in config::apply_monitor_section(&mut loaded.config, &loaded.monitor_sections, name) {
            eprintln!("wayvpet: aviso [monitor:{name}]: {w}");
        }
    }

    // --toggle: si hay una instancia (para este monitor), la para y salimos.
    if args.toggle {
        match toggle::run(target.as_deref()) {
            toggle::Outcome::Stopped => return ExitCode::SUCCESS,
            toggle::Outcome::NotRunning => {}
        }
    }

    // Lector de teclado en un proceso aparte con seccomp (spec 0013 §2). Se hace
    // AQUÍ: el proceso todavía es monohilo y aún no se tomó el fichero PID ni se
    // conectó a Wayland, así que el hijo no hereda esos descriptores.
    let input = match input::start(&loaded.config) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("wayvpet: no se pudo arrancar el lector de input: {e}");
            return ExitCode::from(1);
        }
    };

    // Fichero PID: una instancia por salida (`wayvpet[-NOMBRE].pid`).
    let _pid = match pidfile::PidFile::acquire(target.as_deref()) {
        Ok(pidfile::Acquire::Ok(p)) => p,
        Ok(pidfile::Acquire::AlreadyRunning) => {
            eprintln!("wayvpet: ya hay otra instancia corriendo");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("wayvpet: no se pudo crear el fichero PID: {e}");
            return ExitCode::from(1);
        }
    };

    let tray_enabled = tray::wanted(loaded.config.enable_tray, args.no_tray);
    match wl::run_overlay(
        &loaded.config,
        loaded.path.clone(),
        args.watch_config,
        args.no_toplevel,
        tray_enabled,
        input,
        target,
        wl::MultiHead {
            child: args.multi_monitor_child,
            primary: args.multi_primary,
        },
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wayvpet: error de Wayland: {e}");
            ExitCode::from(1)
        }
    }
}

/// Envoltorio de `--supervise`: relanza el binario (con los mismos argumentos,
/// quitando `--supervise`) mientras salga con código ≠ 0. Sale con código 0 (o
/// Ctrl+C, que el overlay traduce a salida limpia) → se detiene. Corta si hay 5
/// fallos rápidos seguidos, para no entrar en bucle (p. ej. un `SIGSYS` de
/// seccomp o un compositor que no arranca).
fn supervise(argv: &[String]) -> ExitCode {
    use std::process::Command;
    use std::time::{Duration, Instant};

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(&argv[0]));
    let child_args: Vec<&str> = argv[1..]
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--supervise" && *a != "-S")
        .collect();

    let mut fast_failures = 0u32;
    let mut backoff = Duration::from_secs(1);
    loop {
        let start = Instant::now();
        let status = match Command::new(&exe).args(&child_args).status() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("wayvpet[supervise]: no se pudo lanzar el overlay: {e}");
                return ExitCode::from(1);
            }
        };
        if status.success() {
            return ExitCode::SUCCESS;
        }

        if start.elapsed() < Duration::from_secs(5) {
            fast_failures += 1;
            if fast_failures >= 5 {
                eprintln!("wayvpet[supervise]: 5 fallos rápidos seguidos ({status}); me rindo");
                return ExitCode::from(1);
            }
            backoff = (backoff * 2).min(Duration::from_secs(30));
        } else {
            fast_failures = 0;
            backoff = Duration::from_secs(1);
        }
        eprintln!(
            "wayvpet[supervise]: el overlay terminó ({status}); reinicio en {}s",
            backoff.as_secs()
        );
        std::thread::sleep(backoff);
    }
}

/// Lanza una instancia hija por cada salida de `names` (con `--monitor NOMBRE`) y
/// espera a que terminen todas. Porta `multi_monitor_launch`.
///
/// La **primera** salida es la "primaria": es la única que monta el icono de la
/// bandeja (a las demás se les pasa `--no-tray`), de modo que multi-monitor
/// muestre un solo icono. El menú controla a todas por IPC.
fn spawn_per_monitor(args: &Args, loaded: &io::Loaded, names: &[String]) -> ExitCode {
    use std::process::Command;

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wayvpet: no se pudo resolver el ejecutable: {e}");
            return ExitCode::from(1);
        }
    };

    let mut kids = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let mut cmd = Command::new(&exe);
        cmd.arg("--multi-monitor-child").arg("--monitor").arg(name);
        if i == 0 {
            // La primaria monta el tray y enlaza `wayvpet.sock`.
            cmd.arg("--multi-primary");
        } else {
            // Las demás, sin icono: un solo icono para el grupo.
            cmd.arg("--no-tray");
        }
        if let Some(p) = &loaded.path {
            cmd.arg("-c").arg(p);
        }
        if args.watch_config {
            cmd.arg("-w");
        }
        if args.no_toplevel {
            cmd.arg("--no-toplevel");
        }
        match cmd.spawn() {
            Ok(c) => {
                eprintln!("wayvpet: instancia para '{name}' (PID {})", c.id());
                kids.push(c);
            }
            Err(e) => eprintln!("wayvpet: no se pudo lanzar la instancia para '{name}': {e}"),
        }
    }

    if kids.is_empty() {
        return ExitCode::from(1);
    }
    for mut k in kids {
        let _ = k.wait();
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Result<Args, String> {
        let mut owned = vec!["wayvpet".to_string()];
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
    fn supervise_y_alias_corto() {
        assert!(args(&["--supervise"]).unwrap().supervise);
        assert!(args(&["-S"]).unwrap().supervise);
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
