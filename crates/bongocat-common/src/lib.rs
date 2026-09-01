//! Lógica compartida entre `bongocat` (overlay) y `bongocatctl` (configuración).
//!
//! Reglas del crate:
//! - Sin Wayland, sin hilos, sin procesos: solo lógica.
//! - La única E/S está aislada en el módulo [`io`] (leer el `bongocat.conf`).
//! - Sin `unsafe` (lo impone `Cargo.toml`).
//!
//! Portado del árbol C durante la Fase 0.5 (migración a Rust), a paridad 1:1
//! con los tests de `tests/test_*.c`.

pub mod config;
pub mod fullscreen;
pub mod io;
pub mod paw;
pub mod scale;
pub mod sleep;
