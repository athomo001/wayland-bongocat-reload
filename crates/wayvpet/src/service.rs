//! `--install-service` / `--uninstall-service`: gestiona la unidad **systemd de
//! usuario** que arranca el supervisor (`wayvpet --supervise`) con la sesión
//! gráfica (spec 0002 M2). No necesita `root`.
//!
//! La unidad lleva un endurecimiento básico (sin red, sin `/tmp` compartido, sin
//! escalada de privilegios); el endurecimiento completo con `systemd-analyze
//! security` es 0013 M9.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

const UNIT_NAME: &str = "wayvpet.service";

/// `~/.config/systemd/user/wayvpet.service` (respeta `$XDG_CONFIG_HOME`).
fn unit_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("systemd/user").join(UNIT_NAME)
}

fn unit_contents(exec: &str) -> String {
    format!(
        "[Unit]\n\
         Description=wayvpet — overlay animado para Wayland\n\
         Documentation=https://github.com/athomo001/wayland-wayvpet-reload\n\
         PartOf=graphical-session.target\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec} --supervise\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         # Endurecimiento básico (0013 M9 lo amplía):\n\
         NoNewPrivileges=true\n\
         PrivateTmp=true\n\
         RestrictAddressFamilies=AF_UNIX\n\
         IPAddressDeny=any\n\
         ProtectKernelTunables=true\n\
         ProtectControlGroups=true\n\
         RestrictNamespaces=true\n\
         \n\
         [Install]\n\
         WantedBy=graphical-session.target\n"
    )
}

fn systemctl(args: &[&str]) {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!(
            "wayvpet: `systemctl --user {}` salió con {s}",
            args.join(" ")
        ),
        Err(e) => eprintln!("wayvpet: no se pudo ejecutar systemctl: {e}"),
    }
}

/// Escribe la unidad apuntando al ejecutable actual y recarga systemd.
pub fn install() -> ExitCode {
    let exec = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wayvpet: no se pudo resolver el ejecutable: {e}");
            return ExitCode::from(1);
        }
    };
    let path = unit_path();
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("wayvpet: no se pudo crear {}: {e}", dir.display());
            return ExitCode::from(1);
        }
    }
    if let Err(e) = std::fs::write(&path, unit_contents(&exec.to_string_lossy())) {
        eprintln!("wayvpet: no se pudo escribir {}: {e}", path.display());
        return ExitCode::from(1);
    }
    println!("wayvpet: unidad instalada en {}", path.display());
    systemctl(&["daemon-reload"]);
    println!("Actívala con:  systemctl --user enable --now {UNIT_NAME}");
    ExitCode::SUCCESS
}

/// Para, deshabilita y borra la unidad. No toca la configuración del usuario.
pub fn uninstall() -> ExitCode {
    systemctl(&["disable", "--now", UNIT_NAME]);
    let path = unit_path();
    match std::fs::remove_file(&path) {
        Ok(()) => println!("wayvpet: unidad borrada ({})", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("wayvpet: no había unidad instalada");
        }
        Err(e) => {
            eprintln!("wayvpet: no se pudo borrar {}: {e}", path.display());
            return ExitCode::from(1);
        }
    }
    systemctl(&["daemon-reload"]);
    ExitCode::SUCCESS
}
