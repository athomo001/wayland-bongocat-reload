//! Núcleo del icono de bandeja (spec 0011). Aquí vive la parte **pura y
//! testeable**: el mapeo ítem-de-menú → [`TrayCommand`] (`T-0011-M1-map`), la
//! máquina de estado del icono [`Tray`] / [`TrayStatus`] (`T-0011-status`), y la
//! decisión de si arrancar el tray ([`wanted`]).
//!
//! El *frontend* StatusNotifierItem (hilo `ksni` ↔ bucle del supervisor por un
//! `calloop::channel`) es una rebanada aparte, a la espera de decidir la
//! dependencia D-Bus (`ksni 0.2` con `libdbus` del sistema vs `ksni 0.3` con
//! `async-io` y MSRV 1.80). Sin ese frontend el usuario ya maneja todo por
//! `bongocatctl` (`show`/`hide`/`toggle`/`reload`/`restart`/`stop`).
#![allow(dead_code)] // el consumidor (frontend SNI) es una rebanada futura

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
