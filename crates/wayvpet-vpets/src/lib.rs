//! Paquete **solo datos**: no exporta nada. El contenido real son los vpets
//! pesados de `vpets/`, que el empaquetado (`cargo-deb` / `cargo-generate-rpm`)
//! copia a `/usr/share/wayvpet/themes/`. Ver `packaging/README.md` §"Añadir un
//! vpet".
//!
//! Existe como crate solo para reutilizar las herramientas de empaquetado; no
//! se compila nada útil.
