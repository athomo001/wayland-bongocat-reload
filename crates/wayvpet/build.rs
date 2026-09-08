//! Fija `WAYVPET_VERSION` para `env!()` en el binario: `git describe` si hay
//! repo (`3.0.0-14-gabc1234`, `-dirty` si el árbol tiene cambios), o la versión
//! de `Cargo.toml` como reserva (tarball, Nix, sin `git`). Así `--version` se
//! mueve con cada commit sin tocar `Cargo.toml` a mano.

use std::process::Command;

fn main() {
    // Reconstruir la constante si cambia el HEAD o los tags.
    for p in [
        "../../.git/HEAD",
        "../../.git/refs",
        "../../.git/packed-refs",
    ] {
        println!("cargo:rerun-if-changed={p}");
    }
    let pkg = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let version = git_describe().unwrap_or(pkg);
    println!("cargo:rustc-env=WAYVPET_VERSION={version}");
}

fn git_describe() -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty=-dirty"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        return None;
    }
    // `v3.0.0-14-gabc1234` → `3.0.0-14-gabc1234`. Solo se quita la `v` si va
    // seguida de un dígito (un tag como `vpet-prerelease` se deja tal cual).
    let out = match s.strip_prefix('v') {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest,
        _ => &s,
    };
    Some(out.to_string())
}
