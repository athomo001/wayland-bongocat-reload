//! Regla pura de "¿este toplevel a pantalla completa nos importa?".
//!
//! El resto de la detección de pantalla completa (protocolo
//! `zwlr_foreign_toplevel`, fallbacks de Hyprland/KDE) vive en el binario
//! `bongocat`; aquí solo la decisión testeable. Portado de
//! `fullscreen_toplevel_relevant` (`include/platform/fullscreen.h`).

/// Un toplevel es relevante para ocultar el gato si está **activado** (es la
/// ventana con foco) y, o bien el compositor no envía eventos de salida por
/// toplevel (no podemos saber en qué monitor está → asumimos que sí), o bien
/// está en **nuestra** salida.
#[must_use]
pub fn toplevel_relevant(has_output_events: bool, is_on_output: bool, is_activated: bool) -> bool {
    is_activated && (!has_output_events || is_on_output)
}

#[cfg(test)]
mod tests {
    // Casos portados verbatim de `tests/test_fullscreen_state.c`.
    use super::toplevel_relevant;

    #[test]
    fn tabla_de_relevancia() {
        assert!(toplevel_relevant(true, true, true));
        assert!(!toplevel_relevant(true, true, false));
        assert!(!toplevel_relevant(true, false, true));
        assert!(toplevel_relevant(false, false, true));
        assert!(!toplevel_relevant(false, false, false));
    }
}
