//! Lógica compartida entre `bongocat` (overlay) y `bongocatctl` (configuración).
//!
//! Reglas del crate:
//! - Sin E/S, sin Wayland, sin hilos: solo funciones deterministas y tipos.
//! - Sin `unsafe` (lo impone `Cargo.toml`).
//!
//! Portado del árbol C durante la Fase 0.5 (migración a Rust), a paridad 1:1
//! con `include/graphics/paw_frame.h` y `include/platform/scale.h`.

pub mod paw;
pub mod scale;
