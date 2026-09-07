//! `T-0015-M2-fetch` (spec 0015): el hijo de red contra un servidor HTTP local
//! de mentira. Comprueba que lee `tag_name`/`assets`, cruza el `SHA256SUMS`,
//! respeta el tope de 64 KiB, y que un fallo se convierte en `{error}` sin
//! reventar. Habla HTTP plano por `127.0.0.1`: no ejercita TLS (eso es cosa de
//! `ureq`/`rustls`), sí toda la lógica de parseo, topes y escritura.
#![cfg(feature = "net")]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wayvpet_update::net;

/// Servidor de un solo uso. Se enlaza el puerto ANTES de construir el JSON del
/// release para poder meter en él las URLs de descarga (`<base>/dl/...`). El
/// hilo de `accept` para al soltar el `Drop`.
struct FakeGithub {
    base: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FakeGithub {
    fn start(make_release: impl FnOnce(&str) -> String, sums: String, oversize: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let release = make_release(&base);

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let handle = thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &release, &sums, oversize),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        FakeGithub {
            base,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for FakeGithub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve(mut stream: TcpStream, release: &str, sums: &str, oversize: bool) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    // Descartar el resto de cabeceras hasta la línea en blanco.
    loop {
        let mut h = String::new();
        let n = reader.read_line(&mut h).unwrap_or(0);
        if n == 0 || h == "\r\n" || h == "\n" {
            break;
        }
    }

    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (status, body): (&str, Vec<u8>) = if path.ends_with("/releases/latest") {
        if oversize {
            // JSON válido pero por encima del tope de 64 KiB.
            let filler = "x".repeat(70 * 1024);
            let json = format!("{{\"tag_name\":\"v9.9.9\",\"body\":\"{filler}\"}}");
            ("200 OK", json.into_bytes())
        } else {
            ("200 OK", release.as_bytes().to_vec())
        }
    } else if path.ends_with("/sums") {
        ("200 OK", sums.as_bytes().to_vec())
    } else {
        ("404 Not Found", b"nope".to_vec())
    };

    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

const HASH_DEB: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const HASH_TGZ: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn release_with_sums(base: &str) -> String {
    serde_json::json!({
        "tag_name": "v0.6.0",
        "html_url": "https://github.com/athomo001/wayvpet/releases/tag/v0.6.0",
        "body": "## Novedades\r\n- cosas\r\n",
        "assets": [
            { "name": "wayvpet_0.6.0_amd64.deb", "browser_download_url": format!("{base}/dl/deb") },
            { "name": "wayvpet-0.6.0.tar.gz", "browser_download_url": format!("{base}/dl/tgz") },
            { "name": "SHA256SUMS", "browser_download_url": format!("{base}/sums") },
        ]
    })
    .to_string()
}

#[test]
fn lee_release_y_cruza_checksums() {
    let sums = format!("{HASH_DEB}  wayvpet_0.6.0_amd64.deb\n{HASH_TGZ}  wayvpet-0.6.0.tar.gz\n");
    let srv = FakeGithub::start(release_with_sums, sums, false);

    let st = net::check(&srv.base, "0.5.0");

    assert!(st.error.is_none(), "sin error: {:?}", st.error);
    assert_eq!(st.latest.as_deref(), Some("0.6.0"));
    assert_eq!(st.installed.as_deref(), Some("0.5.0"));
    assert!(!st.checked_at.is_empty());
    assert_eq!(
        st.url.as_deref(),
        Some("https://github.com/athomo001/wayvpet/releases/tag/v0.6.0")
    );
    assert!(st
        .notes
        .as_deref()
        .unwrap_or_default()
        .contains("Novedades"));

    // `SHA256SUMS` no es un asset descargable; los otros dos sí, con su hash.
    assert_eq!(st.assets.len(), 2);
    let deb = st.assets.iter().find(|a| a.name.ends_with(".deb")).unwrap();
    assert_eq!(deb.sha256.as_deref(), Some(HASH_DEB));
    let tgz = st
        .assets
        .iter()
        .find(|a| a.name.ends_with(".tar.gz"))
        .unwrap();
    assert_eq!(tgz.sha256.as_deref(), Some(HASH_TGZ));

    assert!(st.update_available("0.5.0"), "0.6.0 > 0.5.0");
}

#[test]
fn respeta_el_tope_de_64_kib() {
    let srv = FakeGithub::start(|_| String::new(), String::new(), true);
    let st = net::check(&srv.base, "0.5.0");
    assert!(
        st.error.as_deref().unwrap_or_default().contains("pasa de"),
        "un cuerpo enorme da error de tope, no se parsea: {st:?}"
    );
    assert!(st.latest.is_none());
}

#[test]
fn sin_sha256sums_los_assets_van_sin_hash() {
    let make = |base: &str| {
        serde_json::json!({
            "tag_name": "v0.6.0",
            "assets": [
                { "name": "wayvpet-0.6.0.tar.gz", "browser_download_url": format!("{base}/dl/tgz") },
            ]
        })
        .to_string()
    };
    let srv = FakeGithub::start(make, String::new(), false);
    let st = net::check(&srv.base, "0.5.0");
    assert!(st.error.is_none(), "{:?}", st.error);
    assert_eq!(st.assets.len(), 1);
    assert_eq!(st.assets[0].sha256, None);
}

#[test]
fn servidor_caido_da_error_no_panico() {
    // Puerto 1: nadie escucha.
    let st = net::check("http://127.0.0.1:1", "0.5.0");
    assert!(
        st.error.is_some(),
        "un fallo de conexión se guarda como error"
    );
    assert!(st.latest.is_none());
    assert!(!st.checked_at.is_empty(), "checked_at se rellena igual");
}
