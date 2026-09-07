//! Descarga verificada del artefacto del release (spec 0015, hito M5). **Solo
//! con `--features net`.**
//!
//! Invariantes:
//! - Descarga **solo** por HTTPS desde `github.com` /
//!   `objects.githubusercontent.com`.
//! - **Verifica el SHA-256** contra el manifiesto del release ANTES de dejar el
//!   fichero por bueno. Sin checksum en el manifiesto → se niega.
//! - Tope de tamaño y timeout: no se cuelga ni llena el disco.
//! - **Nunca ejecuta lo descargado.** Baja un fichero y devuelve su ruta; punto.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{net, Asset, UpdateState};

/// Tope duro del artefacto. Un `.deb`/`.rpm`/tarball de wayvpet ronda MB, no
/// cientos; 300 MiB es holgadísimo y corta una respuesta disparatada.
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;

/// Timeout de lectura para la descarga (más largo que el del chequeo: aquí sí
/// puede tardar en una conexión lenta).
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Elige el artefacto a bajar. `explicit` (nombre exacto) manda; si no, se elige
/// por la marca del canal de instalación.
///
/// # Errores
/// Si `explicit` no está entre los assets, o si ningún asset encaja con el
/// canal. El mensaje lista los nombres disponibles.
pub fn pick_asset<'a>(
    assets: &'a [Asset],
    channel: Option<&str>,
    explicit: Option<&str>,
) -> Result<&'a Asset, String> {
    if let Some(name) = explicit {
        return assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| format!("no hay ningún asset llamado «{name}» ({})", names(assets)));
    }
    let matches: fn(&str) -> bool = match channel {
        Some("deb") => |n| n.ends_with(".deb"),
        Some("rpm") => |n| n.ends_with(".rpm"),
        Some("arch") => |n| n.ends_with(".pkg.tar.zst") || n.ends_with(".tar.zst"),
        // `source`, `distro` (no debería llegar aquí), None u otro → el tarball.
        _ => |n| n.ends_with(".tar.gz") || n.ends_with(".tgz"),
    };
    let mut found = assets.iter().filter(|a| matches(&a.name));
    // Preferimos el primero; si hay `source` y no hay tarball, caemos a cualquiera.
    found
        .next()
        .or_else(|| assets.first())
        .ok_or_else(|| "el release no publicó ningún artefacto".to_string())
}

fn names(assets: &[Asset]) -> String {
    if assets.is_empty() {
        return "sin assets".to_string();
    }
    format!(
        "disponibles: {}",
        assets
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `true` si la URL es HTTPS y el host es de GitHub (donde se sirven los assets
/// de un release). Cualquier otra cosa se rechaza (spec 0015 §Seguridad).
pub fn is_allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', ':']).next().unwrap_or("");
    host == "github.com"
        || host == "objects.githubusercontent.com"
        || host
            .strip_suffix(".githubusercontent.com")
            .is_some_and(|s| !s.is_empty())
}

/// SHA-256 de un fichero, en hex minúsculas. Se lee a trozos: no carga el
/// artefacto entero en memoria.
///
/// # Errores
/// E/S al abrir o leer.
pub fn sha256_hex(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    Ok(hex(ctx.finish().as_ref()))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Directorio de descargas: `$XDG_DOWNLOAD_DIR`, o `~/Descargas`, o `~/Downloads`,
/// o el directorio actual como último recurso.
#[must_use]
pub fn download_dir() -> PathBuf {
    if let Ok(d) = std::env::var("XDG_DOWNLOAD_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        for cand in ["Descargas", "Downloads"] {
            let p = Path::new(&home).join(cand);
            if p.is_dir() {
                return p;
            }
        }
        return Path::new(&home).join("Descargas");
    }
    PathBuf::from(".")
}

/// Baja el artefacto elegido a [`download_dir`], **verifica su SHA-256** contra
/// el manifiesto, y devuelve la ruta final. `progress(descargado, total)` se
/// llama a medida que llega el cuerpo (`total` es `None` si el servidor no envía
/// `Content-Length`).
///
/// # Errores
/// Sin checksum en el manifiesto, URL no permitida, fallo de red, respuesta
/// mayor que el tope, o **checksum que no cuadra** (el fichero parcial se borra).
pub fn run(
    state: &UpdateState,
    channel: Option<&str>,
    explicit: Option<&str>,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let asset = pick_asset(&state.assets, channel, explicit)?;
    let expected = asset
        .sha256
        .as_deref()
        .filter(|s| s.len() == 64)
        .ok_or_else(|| {
            format!(
                "«{}» no trae SHA-256 en el manifiesto del release; no puedo verificar la descarga",
                asset.name
            )
        })?;
    if !is_allowed_url(&asset.url) {
        return Err(format!("URL de descarga no permitida: {}", asset.url));
    }

    fetch_and_verify(
        &asset.url,
        expected,
        &download_dir(),
        &asset.name,
        &mut progress,
    )
}

/// Baja `url` a `dir/name`, verificando el SHA-256 contra `expected_hex`. Deja
/// el fichero solo si el checksum cuadra; si no, borra el parcial. **No**
/// comprueba el host de `url` (eso lo hace [`run`] antes) — pensado también para
/// tests contra un servidor local.
///
/// # Errores
/// Fallo de red, respuesta mayor que el tope, o checksum que no cuadra.
pub fn fetch_and_verify(
    url: &str,
    expected_hex: &str,
    dir: &Path,
    name: &str,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("no pude crear {}: {e}", dir.display()))?;
    let final_path = dir.join(name);
    let tmp_path = dir.join(format!(".{name}.part.{}", std::process::id()));

    let result = download_to(url, &tmp_path, progress).and_then(|()| {
        let got = sha256_hex(&tmp_path).map_err(|e| format!("no pude leer lo descargado: {e}"))?;
        if got.eq_ignore_ascii_case(expected_hex) {
            Ok(())
        } else {
            Err(format!(
                "el SHA-256 no cuadra:\n  esperado {expected_hex}\n  obtenido {got}"
            ))
        }
    });

    match result {
        Ok(()) => {
            fs::rename(&tmp_path, &final_path)
                .map_err(|e| format!("no pude mover el fichero a su sitio: {e}"))?;
            Ok(final_path)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

fn download_to(
    url: &str,
    tmp_path: &Path,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(READ_TIMEOUT)
        .user_agent(concat!("wayvpet/", env!("CARGO_PKG_VERSION")))
        .build();
    let resp = agent.get(url).call().map_err(net::short_err)?;

    let total: Option<u64> = resp
        .header("Content-Length")
        .and_then(|s| s.parse().ok())
        .filter(|&n| n <= MAX_DOWNLOAD);
    if let Some(t) = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
    {
        if t > MAX_DOWNLOAD {
            return Err(format!(
                "el artefacto dice pesar {t} B (tope {MAX_DOWNLOAD})"
            ));
        }
    }

    let mut file =
        fs::File::create(tmp_path).map_err(|e| format!("no pude crear el temporal: {e}"))?;
    let mut reader = resp.into_reader().take(MAX_DOWNLOAD + 1);
    let mut buf = [0u8; 128 * 1024];
    let mut done: u64 = 0;
    progress(0, total);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("lectura: {e}"))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > MAX_DOWNLOAD {
            return Err(format!("la descarga pasó de {MAX_DOWNLOAD} B"));
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("escritura: {e}"))?;
        progress(done, total);
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, sha: Option<&str>) -> Asset {
        Asset {
            name: name.to_string(),
            url: format!("https://github.com/x/{name}"),
            sha256: sha.map(str::to_string),
        }
    }

    #[test]
    fn pick_asset_por_canal() {
        let assets = vec![
            asset("wayvpet_0.6.0_amd64.deb", Some("a")),
            asset("wayvpet-0.6.0-1.x86_64.rpm", Some("b")),
            asset("wayvpet-0.6.0.tar.gz", Some("c")),
        ];
        assert_eq!(
            pick_asset(&assets, Some("deb"), None).unwrap().name,
            "wayvpet_0.6.0_amd64.deb"
        );
        assert_eq!(
            pick_asset(&assets, Some("rpm"), None).unwrap().name,
            "wayvpet-0.6.0-1.x86_64.rpm"
        );
        assert_eq!(
            pick_asset(&assets, Some("source"), None).unwrap().name,
            "wayvpet-0.6.0.tar.gz"
        );
        assert_eq!(
            pick_asset(&assets, None, None).unwrap().name,
            "wayvpet-0.6.0.tar.gz"
        );
        // nombre explícito
        assert_eq!(
            pick_asset(&assets, Some("deb"), Some("wayvpet-0.6.0.tar.gz"))
                .unwrap()
                .name,
            "wayvpet-0.6.0.tar.gz"
        );
        assert!(pick_asset(&assets, None, Some("no-existe")).is_err());
    }

    #[test]
    fn is_allowed_url_solo_github_https() {
        assert!(is_allowed_url(
            "https://github.com/x/y/releases/download/v1/a.deb"
        ));
        assert!(is_allowed_url("https://objects.githubusercontent.com/abc"));
        assert!(is_allowed_url(
            "https://release-assets.githubusercontent.com/x"
        ));
        assert!(!is_allowed_url("http://github.com/x"), "http no");
        assert!(
            !is_allowed_url("https://evil.com/github.com/x"),
            "host es evil.com"
        );
        assert!(!is_allowed_url("https://notgithub.com/x"));
        assert!(!is_allowed_url("ftp://github.com/x"));
    }

    #[test]
    fn sha256_hex_de_un_fichero_conocido() {
        let dir = std::env::temp_dir().join(format!("wayvpet-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f");
        std::fs::write(&p, b"abc").unwrap();
        // SHA-256("abc")
        assert_eq!(
            sha256_hex(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
