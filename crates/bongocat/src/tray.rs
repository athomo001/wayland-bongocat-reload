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

// `on_overlay_down`/`on_overlay_recovered`/`on_manual_restart` (la máquina
// `Tray`) esperan a que exista supervisión real de la surface (teardown +
// rebuild, spec 0011 M2 pleno) para tener quien las llame; `Normal`/`Hidden`
// ya viven en vivo vía `TrayHandle::set_status` sin pasar por `Tray`.
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

/// Instrucción del hilo del overlay al hilo del tray.
enum ToTray {
    /// Cambia el estado visual del icono (spec 0011 §"Estado del icono", M2).
    SetStatus(TrayStatus),
    Shutdown,
}

/// El objeto `ksni::Tray`. Cada clic de menú manda un [`TrayCommand`] por
/// `cmd_tx` al bucle del overlay; no hace trabajo pesado aquí (spec 0011).
struct SniTray {
    cmd_tx: Sender<TrayCommand>,
    /// Temas instalados para el submenú "Tema"; el activo va marcado.
    themes: Vec<String>,
    active_theme: String,
    /// Icono base (RGBA **recto**, sin premultiplicar) a `icon_size²`; los tres
    /// variantes (Normal/Hidden/Error) se derivan de él en cada consulta —
    /// barato para un icono de bandeja, y evita guardar 3 copias.
    icon_base: Vec<u8>,
    icon_size: u32,
    status: TrayStatus,
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
        vec![tint_icon(&self.icon_base, self.icon_size, self.status)]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let description = match self.status {
            TrayStatus::Normal => "Overlay activo — clic derecho para el menú".to_string(),
            TrayStatus::Hidden => "Oculto — clic derecho para el menú".to_string(),
            TrayStatus::Error => "Overlay caído — usa «Reiniciar overlay»".to_string(),
        };
        ksni::ToolTip {
            title: "Bongo Cat".into(),
            description,
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

    /// Cambia el icono a `status` (Normal/Hidden/Error). No bloqueante: si el
    /// hilo del tray ya no está, no hace nada.
    pub fn set_status(&self, status: TrayStatus) {
        let _ = self.to_tray.send(ToTray::SetStatus(status));
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
    let (icon_base, icon_size) = embedded_icon_base();
    thread::Builder::new()
        .name("tray:sni".into())
        .spawn(move || {
            run(
                cmd_tx,
                themes,
                active_theme,
                icon_base,
                icon_size,
                &from_main,
            )
        })
        .ok()?;
    Some(TrayHandle { to_tray })
}

fn run(
    cmd_tx: Sender<TrayCommand>,
    themes: Vec<String>,
    active_theme: String,
    icon_base: Vec<u8>,
    icon_size: u32,
    from_main: &mpsc::Receiver<ToTray>,
) {
    let tray = SniTray {
        cmd_tx,
        themes,
        active_theme,
        icon_base,
        icon_size,
        status: TrayStatus::Normal,
    };
    // `ksni` con `async-io` gestiona su propio hilo ejecutor; aquí solo hay que
    // llevar el futuro de `spawn()` a término, mantener vivo el `Handle` y
    // reenviarle los cambios de estado que llegan del bucle del overlay.
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
        while let Ok(ToTray::SetStatus(status)) = from_main.recv() {
            handle.update(|t| t.status = status).await;
        }
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

/// Icono base embebido (24×24, RGBA **recto**): el fotograma `both-up` del
/// `classic`, para escritorios sin un icono de tema llamado "bongocat". Recto
/// (no premultiplicado) para que [`tint_icon`] pueda tocar el alfa sin
/// arrastrar color. `(bytes, lado)`; `bytes` vacío si `resvg` fallara.
fn embedded_icon_base() -> (Vec<u8>, u32) {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    const SIZE: u32 = 24;
    let svg = crate::anim::classic_frame_svgs();
    let Ok(tree) = Tree::from_data(svg[0].as_bytes(), &Options::default()) else {
        return (Vec::new(), SIZE);
    };
    let Some(mut pm) = Pixmap::new(SIZE, SIZE) else {
        return (Vec::new(), SIZE);
    };
    let sz = tree.size();
    let t = Transform::from_scale(SIZE as f32 / sz.width(), SIZE as f32 / sz.height());
    resvg::render(&tree, t, &mut pm.as_mut());

    // tiny-skia entrega RGBA premultiplicado; se revierte a recto para poder
    // escalar el alfa (estado `Hidden`) sin manchar los bordes de color.
    let mut data = pm.data().to_vec();
    for px in data.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        if a > 0 {
            let unmul = |c: u8| ((u16::from(c) * 255 + a / 2) / a).min(255) as u8;
            px[0] = unmul(px[0]);
            px[1] = unmul(px[1]);
            px[2] = unmul(px[2]);
        }
    }
    (data, SIZE)
}

/// Deriva de `base` (RGBA recto, `size²`) el icono ARGB32 (orden de red:
/// bytes A,R,G,B) para `status`: `Normal` tal cual; `Hidden` a mitad de alfa;
/// `Error` con un tinte rojo. `Error` no lo dispara nada todavía (M2 pleno
/// necesita supervisión real de la surface) pero el camino ya está listo.
fn tint_icon(base: &[u8], size: u32, status: TrayStatus) -> Icon {
    let mut data = base.to_vec();
    for px in data.chunks_exact_mut(4) {
        match status {
            TrayStatus::Normal => {}
            TrayStatus::Hidden => px[3] = (u16::from(px[3]) * 128 / 255) as u8,
            TrayStatus::Error => {
                px[0] = px[0].saturating_add(140);
                px[1] /= 2;
                px[2] /= 2;
            }
        }
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        px.copy_from_slice(&[a, r, g, b]);
    }
    Icon {
        width: size as i32,
        height: size as i32,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_icon_por_estado() {
        // Un píxel opaco rojo puro, recto (sin premultiplicar).
        let base = [200u8, 20, 20, 255];
        let normal = tint_icon(&base, 1, TrayStatus::Normal);
        assert_eq!(
            normal.data,
            [255, 200, 20, 20],
            "Normal: solo reordena a ARGB"
        );

        let hidden = tint_icon(&base, 1, TrayStatus::Hidden);
        assert_eq!(hidden.data[0], 128, "Hidden: alfa a la mitad");
        assert_eq!(&hidden.data[1..4], &[200, 20, 20], "Hidden: color intacto");

        let error = tint_icon(&base, 1, TrayStatus::Error);
        assert_eq!(error.data[0], 255, "Error: alfa intacto");
        assert!(error.data[1] > 200, "Error: más rojo");
        assert!(
            error.data[2] < 20 || error.data[2] == 10,
            "Error: menos verde"
        );
    }

    #[test]
    fn embedded_icon_base_produce_un_cuadrado_no_vacio() {
        let (data, size) = embedded_icon_base();
        assert_eq!(size, 24);
        assert_eq!(data.len(), (24 * 24 * 4) as usize, "RGBA recto de 24×24");
        assert!(
            data.iter().any(|&b| b != 0),
            "el gato pinta algo, no es todo cero"
        );
    }

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
