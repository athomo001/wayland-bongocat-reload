//! Lógica compartida entre `wayvpet` (overlay) y `wayvpetctl` (configuración).
//!
//! Reglas del crate:
//! - Sin Wayland, sin hilos, sin procesos: solo lógica.
//! - La única E/S está aislada en el módulo [`io`] (leer el `wayvpet.conf`).
//! - Sin `unsafe` (lo impone `Cargo.toml`).
//!
//! Portado del árbol C durante la Fase 0.5 (migración a Rust), a paridad 1:1
//! con los tests de `tests/test_*.c`.

pub mod config;
pub mod edit;
pub mod field_meta;
pub mod fullscreen;
pub mod io;
pub mod ipc;
pub mod mouse;
pub mod paw;
pub mod scale;
pub mod sheet;
pub mod sleep;
pub mod theme;
pub mod vpet;
