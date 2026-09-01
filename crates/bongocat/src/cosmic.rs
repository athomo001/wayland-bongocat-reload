//! Cliente del protocolo `zcosmic_toplevel_info_v1` (COSMIC), versión 2.
//!
//! COSMIC (y GNOME) no exponen `zwlr_foreign_toplevel_management_v1`, así que en
//! esos escritorios el overlay no sabría si hay una ventana a pantalla completa.
//!
//! El camino de versión 1 de este protocolo (evento `toplevel` autónomo) hace
//! que cosmic-comp saque a las ventanas de pantalla completa a los ~2 s, así que
//! usamos el puente documentado de la versión 2: la lista de toplevels llega por
//! `ext-foreign-toplevel-list-v1` (crate `wayland-protocols`) y para cada uno se
//! pide `get_cosmic_toplevel` para poder leer su evento `state` (nos importan
//! `activated` y `fullscreen`, igual que en el protocolo wlr).
//!
//! El cliente se genera con `wayland-scanner` a partir del XML (MIT) copiado en
//! `protocols/cosmic-toplevel-info-unstable-v1.xml`. No se toca `unsafe`.

#![allow(clippy::empty_docs)]

// `generate_client_code!` genera código que resuelve `super::wayland_client`;
// este `use` es imprescindible aunque Clippy lo crea redundante.
#[allow(clippy::single_component_path_imports)]
use wayland_client;
use wayland_client::protocol::*;
// `get_cosmic_toplevel` referencia `ext_foreign_toplevel_handle_v1`: su módulo
// tiene que estar en el ámbito para que el código generado resuelva el tipo.
use wayland_protocols::ext::foreign_toplevel_list::v1::client::*;

pub mod __interfaces {
    use wayland_client::backend as wayland_backend;
    use wayland_client::protocol::__interfaces::*;
    use wayland_protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
    wayland_scanner::generate_interfaces!("../../protocols/cosmic-toplevel-info-unstable-v1.xml");
}
use self::__interfaces::*;

wayland_scanner::generate_client_code!("../../protocols/cosmic-toplevel-info-unstable-v1.xml");
