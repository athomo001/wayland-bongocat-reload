//! Icono de bandeja (StatusNotifierItem, spec 0011).
//!
//! Dos partes:
//! - **Pura y testeable**: mapeo ítem-de-menú → [`TrayCommand`] (`T-0011-M1-map`),
//!   máquina de estado del icono [`Tray`] / [`TrayStatus`] (`T-0011-status`), y la
//!   decisión de si arrancar el tray ([`wanted`]).
//! - **Frontend** ([`spawn`]): un hilo dedicado corre el servicio `ksni` (D-Bus
//!   puro Rust vía `zbus`/`async-io`, sin `libdbus`). Los clics del menú viajan
//!   por un `calloop::channel::Sender<TrayCommand>` al bucle del overlay, que los
//!   ejecuta con la **misma** lógica que los verbos IPC (spec 0011: el tray es
//!   otro cliente, no un camino paralelo).
//!
//! Sin host SNI en el escritorio, `spawn` avisa por stderr y no molesta; el
//! usuario sigue teniendo `bongocatctl` (`show`/`hide`/`reload`/`restart`/`stop`).

// Algunos ganchos de la máquina de estado (`on_overlay_down`, …) los consume la
// rebanada M2 (icono Normal/Hidden/Error en vivo); hoy el frontend solo usa
// `TrayCommand` / `wanted`.
#![allow(dead_code)]

use std::sync::mpsc;
use std::thread;

use calloop::channel::Sender;
use ksni::menu::{StandardItem, SubMenu};
use ksni::{Icon, MenuItem, TrayMethods};

/// ¿Hay que arrancar el tray? `enable_tray` de la config, salvo `--no-tray`.
#[must_use]
pub fn wanted(enable_tray: bool, no_tray_flag: bool) -> bool {
    enable_tray && !no_tray_flag
}

/// Acción que un ítem del menú del tray pide al supervisor (spec 0011
/// §"Comandos y su efecto"). Reutiliza la misma lógica que los verbos IPC: el
/// tray es otro cliente de esas acciones, no un camino paralelo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    /// Mostrar / Ocultar (alterna visibilidad, sin cerrar).
    ToggleVisibility,
    /// Reconstruir las surfaces de overlay sin salir del proceso.
    RestartOverlays,
    /// Lanzar `bongocatctl` / la GUI.
    LaunchConfig,
    /// Submenú Tema ▸ *n*.
    SetTheme(String),
    /// Recargar la configuración desde disco.
    Reload,
    /// Versión + enlace al repo.
    About,
    /// Salida limpia de todo.
    Quit,
}

impl TrayCommand {
    /// Traduce el **id estable** de un ítem de menú (no su etiqueta traducible)
    /// a un comando. El submenú de temas usa `theme:<nombre>`. `None` = ítem
    /// desconocido.
    #[must_use]
    pub fn from_menu_id(id: &str) -> Option<Self> {
        Some(match id {
            "toggle" => Self::ToggleVisibility,
            "restart" => Self::RestartOverlays,
            "configure" => Self::LaunchConfig,
            "reload" => Self::Reload,
            "about" => Self::About,
            "quit" => Self::Quit,
            other => match other.strip_prefix("theme:") {
                Some(name) if !name.is_empty() => Self::SetTheme(name.to_string()),
                _ => return None,
            },
        })
    }
}

/// Estado visual del icono (spec 0011 §"Estado del icono").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayStatus {
    /// Icono a color.
    #[default]
    Normal,
    /// Icono atenuado (el gato está oculto).
    Hidden,
    /// Overlay caído y sin poder reconstruirse.
    Error,
}

/// Reintentos de reconstrucción antes de declarar `Error` (spec 0011 §M2: 0–3).
pub const MAX_RESTART_ATTEMPTS: u32 = 3;

/// Máquina de estado del icono, alimentada por eventos del supervisor.
#[derive(Debug, Default)]
pub struct Tray {
    status: TrayStatus,
    /// Fallos de overlay consumidos en la racha actual.
    restart_attempts: u32,
}

impl Tray {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn status(&self) -> TrayStatus {
        self.status
    }

    /// El overlay pasó a oculto / visible. Un `Error` (overlay caído) manda
    /// sobre la visibilidad: no se pisa hasta recuperarse.
    pub fn set_hidden(&mut self, hidden: bool) {
        if self.status == TrayStatus::Error {
            return;
        }
        self.status = if hidden {
            TrayStatus::Hidden
        } else {
            TrayStatus::Normal
        };
    }

    /// Una `Overlay` se cayó y se va a intentar reconstruir. Mientras queden
    /// reintentos el icono no cambia; agotados → `Error`.
    pub fn on_overlay_down(&mut self) {
        self.restart_attempts += 1;
        if self.restart_attempts > MAX_RESTART_ATTEMPTS {
            self.status = TrayStatus::Error;
        }
    }

    /// La `Overlay` se reconstruyó: limpia el contador y sale de `Error`. La
    /// visibilidad se re-empuja aparte con [`Tray::set_hidden`].
    pub fn on_overlay_recovered(&mut self) {
        self.restart_attempts = 0;
        if self.status == TrayStatus::Error {
            self.status = TrayStatus::Normal;
        }
    }

    /// `RestartOverlays` manual: el usuario pide un intento limpio.
    pub fn on_manual_restart(&mut self) {
        self.on_overlay_recovered();
    }

    /// Texto del tooltip según el estado y el nº de monitores.
    #[must_use]
    pub fn tooltip(&self, monitors: usize) -> String {
        match self.status {
            TrayStatus::Normal => format!("bongocat — {monitors} monitor(es)"),
            TrayStatus::Hidden => "bongocat — oculto".to_string(),
            TrayStatus::Error => "bongocat — overlay caído (clic para reiniciar)".to_string(),
        }
    }
}

// --- Frontend StatusNotifierItem (ksni) -----------------------------------

/// Instrucción del hilo del overlay al hilo del tray (por ahora solo apagarlo
/// al salir; los cambios de icono Normal/Hidden/Error son de M2).
enum ToTray {
    Shutdown,
}

/// El objeto `ksni::Tray`. Cada clic de menú manda un [`TrayCommand`] por
/// `cmd_tx` al bucle del overlay; no hace trabajo pesado aquí (spec 0011).
struct SniTray {
    cmd_tx: Sender<TrayCommand>,
    /// Temas instalados para el submenú "Tema"; el activo va marcado.
    themes: Vec<String>,
    active_theme: String,
    icon: Vec<Icon>,
}

impl SniTray {
    fn send(&self, cmd: TrayCommand) {
        // `calloop::channel::Sender` es no bloqueante; si el bucle ya cerró, da igual.
        let _ = self.cmd_tx.send(cmd);
    }
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "bongocat".into()
    }
    fn title(&self) -> String {
        "Bongo Cat".into()
    }
    fn icon_name(&self) -> String {
        // Si el tema del panel tiene un icono llamado "bongocat", lo usa; si no,
        // cae al pixmap embebido de abajo.
        "bongocat".into()
    }
    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Bongo Cat".into(),
            description: "Overlay activo — clic derecho para el menú".into(),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let item = |label: &str, cmd: TrayCommand| {
            MenuItem::Standard(StandardItem {
                label: label.into(),
                activate: Box::new(move |t: &mut Self| t.send(cmd.clone())),
                ..Default::default()
            })
        };
        let temas: Vec<MenuItem<Self>> = self
            .themes
            .iter()
            .cloned()
            .map(|name| {
                let activo = name == self.active_theme;
                let label = if activo {
                    format!("● {name}")
                } else {
                    format!("   {name}")
                };
                let cmd = TrayCommand::SetTheme(name);
                MenuItem::Standard(StandardItem {
                    label,
                    activate: Box::new(move |t: &mut Self| t.send(cmd.clone())),
                    ..Default::default()
                })
            })
            .collect();

        vec![
            item("Mostrar / Ocultar", TrayCommand::ToggleVisibility),
            item("Reiniciar overlay", TrayCommand::RestartOverlays),
            item("Recargar configuración", TrayCommand::Reload),
            item("Configurar…", TrayCommand::LaunchConfig),
            MenuItem::SubMenu(SubMenu {
                label: "Tema".into(),
                submenu: temas,
                ..Default::default()
            }),
            MenuItem::Separator,
            item("Acerca de", TrayCommand::About),
            item("Cerrar", TrayCommand::Quit),
        ]
    }
}

/// Handle del hilo del tray: al soltarlo (o llamar [`TrayHandle::stop`]) el
/// servicio SNI se apaga y el icono desaparece.
pub struct TrayHandle {
    to_tray: mpsc::Sender<ToTray>,
}

impl TrayHandle {
    /// Apaga el servicio del tray (se llama al salir del overlay).
    pub fn stop(&self) {
        let _ = self.to_tray.send(ToTray::Shutdown);
    }
}

/// Arranca el servicio del tray en un hilo dedicado. `cmd_tx` recibe un
/// [`TrayCommand`] por cada clic de menú. Devuelve `None` si no se pudo crear el
/// hilo; que **no haya host SNI** no es un error aquí (se avisa por stderr).
#[must_use]
pub fn spawn(
    cmd_tx: Sender<TrayCommand>,
    themes: Vec<String>,
    active_theme: String,
) -> Option<TrayHandle> {
    let (to_tray, from_main) = mpsc::channel::<ToTray>();
    let icon = embedded_icon();
    thread::Builder::new()
        .name("tray:sni".into())
        .spawn(move || run(cmd_tx, themes, active_theme, icon, &from_main))
        .ok()?;
    Some(TrayHandle { to_tray })
}

fn run(
    cmd_tx: Sender<TrayCommand>,
    themes: Vec<String>,
    active_theme: String,
    icon: Vec<Icon>,
    from_main: &mpsc::Receiver<ToTray>,
) {
    let tray = SniTray {
        cmd_tx,
        themes,
        active_theme,
        icon,
    };
    // `ksni` con `async-io` gestiona su propio hilo ejecutor; aquí solo hay que
    // llevar el futuro de `spawn()` a término y mantener vivo el `Handle`.
    futures_lite::future::block_on(async move {
        let handle = match tray.spawn().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!(
                    "bongocat: no hay icono de bandeja ({e}); usa `bongocatctl` \
                     (show/hide/reload/restart/stop)"
                );
                return;
            }
        };
        eprintln!("bongocat: icono de bandeja activo");
        // Bloquea hasta que el overlay pida apagar (o cierre el canal al salir).
        let _ = from_main.recv();
        handle.shutdown();
    });
}

/// "Configurar…": lanza una GUI dedicada si existe, si no `bongocatctl` en un
/// terminal conocido. Lista fija de binarios, **sin shell** ni interpolar nada
/// (spec 0011 §Seguridad).
pub fn launch_config() {
    use std::process::Command;
    if Command::new("bongocat-config").spawn().is_ok() {
        return;
    }
    for term in [
        "x-terminal-emulator",
        "foot",
        "kitty",
        "alacritty",
        "wezterm",
        "konsole",
        "gnome-terminal",
    ] {
        if Command::new(term)
            .arg("-e")
            .arg("bongocatctl")
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    eprintln!(
        "bongocat: no encuentro un terminal para 'Configurar…'; ejecuta `bongocatctl` a mano"
    );
}

/// Icono ARGB32 embebido (24×24): el fotograma `both-up` del `classic`, para
/// escritorios sin un icono de tema llamado "bongocat".
fn embedded_icon() -> Vec<Icon> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    const SIZE: u32 = 24;
    let svg = crate::anim::classic_frame_svgs();
    let Ok(tree) = Tree::from_data(svg[0].as_bytes(), &Options::default()) else {
        return Vec::new();
    };
    let Some(mut pm) = Pixmap::new(SIZE, SIZE) else {
        return Vec::new();
    };
    let sz = tree.size();
    let t = Transform::from_scale(SIZE as f32 / sz.width(), SIZE as f32 / sz.height());
    resvg::render(&tree, t, &mut pm.as_mut());

    // tiny-skia entrega RGBA premultiplicado (bytes R,G,B,A); ksni quiere ARGB32
    // en orden de red (bytes A,R,G,B).
    let mut data = pm.data().to_vec();
    for px in data.chunks_exact_mut(4) {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        px.copy_from_slice(&[a, r, g, b]);
    }
    vec![Icon {
        width: SIZE as i32,
        height: SIZE as i32,
        data,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanted_respeta_config_y_flag() {
        assert!(wanted(true, false));
        assert!(!wanted(true, true), "--no-tray gana");
        assert!(!wanted(false, false), "enable_tray=0");
        assert!(!wanted(false, true));
    }

    #[test]
    fn mapeo_item_a_comando() {
        assert_eq!(
            TrayCommand::from_menu_id("toggle"),
            Some(TrayCommand::ToggleVisibility)
        );
        assert_eq!(TrayCommand::from_menu_id("quit"), Some(TrayCommand::Quit));
        assert_eq!(
            TrayCommand::from_menu_id("theme:pink"),
            Some(TrayCommand::SetTheme("pink".into()))
        );
        assert_eq!(TrayCommand::from_menu_id("theme:"), None, "tema sin nombre");
        assert_eq!(TrayCommand::from_menu_id("no_existe"), None);
    }

    #[test]
    fn status_reintentos_y_error() {
        let mut t = Tray::new();
        assert_eq!(t.status(), TrayStatus::Normal);

        // 3 caídas: aún se reintenta, sin `Error`.
        for _ in 0..MAX_RESTART_ATTEMPTS {
            t.on_overlay_down();
        }
        assert_eq!(t.status(), TrayStatus::Normal);

        // La 4ª agota los reintentos → `Error`.
        t.on_overlay_down();
        assert_eq!(t.status(), TrayStatus::Error);

        // Recuperación → `Normal` y contador a cero.
        t.on_overlay_recovered();
        assert_eq!(t.status(), TrayStatus::Normal);
        for _ in 0..MAX_RESTART_ATTEMPTS {
            t.on_overlay_down();
        }
        assert_eq!(t.status(), TrayStatus::Normal, "el contador se reinició");
    }

    #[test]
    fn status_error_manda_sobre_visibilidad() {
        let mut t = Tray::new();
        t.set_hidden(true);
        assert_eq!(t.status(), TrayStatus::Hidden);
        t.set_hidden(false);
        assert_eq!(t.status(), TrayStatus::Normal);

        // En `Error`, `set_hidden` no hace nada hasta recuperarse.
        for _ in 0..=MAX_RESTART_ATTEMPTS {
            t.on_overlay_down();
        }
        assert_eq!(t.status(), TrayStatus::Error);
        t.set_hidden(true);
        assert_eq!(t.status(), TrayStatus::Error, "overlay caído manda");
        t.on_manual_restart();
        assert_eq!(t.status(), TrayStatus::Normal);
    }

    #[test]
    fn tooltip_por_estado() {
        let mut t = Tray::new();
        assert!(t.tooltip(2).contains('2'));
        t.set_hidden(true);
        assert!(t.tooltip(2).contains("oculto"));
    }
}
