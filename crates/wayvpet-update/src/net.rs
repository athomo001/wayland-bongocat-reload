//! Chequeo HTTPS de nueva versión (spec 0015, hito M2). **Solo se compila con
//! `--features net`**: es lo único que enlaza `ureq`+`rustls`, y solo lo hace el
//! binario `wayvpet-update-check`, nunca el supervisor ni `wayvpetctl`.
//!
//! Invariantes de este módulo:
//! - **Una** petición a `releases/latest` (más, si hay, una al `SHA256SUMS`).
//! - Timeouts cortos y tope de bytes en TODAS las respuestas: esto corre al
//!   arrancar y no puede colgar ni tragarse un cuerpo enorme.
//! - La petición no lleva identificador: solo `User-Agent: wayvpet/X.Y.Z`.
//! - Nunca propaga un fallo: [`check`] siempre devuelve un [`UpdateState`], con
//!   el problema en el campo `error`.

use std::collections::HashMap;
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::{Asset, UpdateState};

/// Base de la API de GitHub. Los tests la sustituyen por `http://127.0.0.1:PORT`.
pub const GITHUB_API: &str = "https://api.github.com";

/// Repositorio consultado. Fijo: no es configurable desde el `.conf` (spec 0015
/// §Seguridad: "sin rutas ni comandos dentro").
const REPO: &str = "athomo001/wayvpet";

/// Timeout por operación (conexión / lectura / escritura). Corto a propósito.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Tope del cuerpo de `releases/latest`. Ronda 2–8 KiB; 64 KiB es holgado.
const MAX_BODY: u64 = 64 * 1024;
/// Tope del `SHA256SUMS` (unas pocas líneas).
const MAX_SUMS: u64 = 16 * 1024;
/// Recorte de las notas del release que se guardan en el estado.
const MAX_NOTES_CHARS: usize = 2000;

/// Subconjunto de la respuesta de `GET /repos/{repo}/releases/latest` que
/// necesitamos. Los demás campos se ignoran.
#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    #[serde(default)]
    browser_download_url: String,
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(TIMEOUT)
        .timeout_read(TIMEOUT)
        .timeout_write(TIMEOUT)
        // Sin `id`, sin SO: solo la versión, como pide la spec.
        .user_agent(concat!("wayvpet/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Hace el chequeo y devuelve el estado listo para escribir. `installed` es el
/// `CARGO_PKG_VERSION` del wayvpet que lanzó al hijo. **Nunca falla**: un
/// problema (sin red, rate-limit, JSON raro) queda en `UpdateState::error` y el
/// resto de campos vacíos.
#[must_use]
pub fn check(api_base: &str, installed: &str) -> UpdateState {
    let checked_at = now_iso8601();
    match try_check(&agent(), api_base, installed) {
        Ok(mut st) => {
            st.checked_at = checked_at;
            st
        }
        Err(error) => UpdateState {
            checked_at,
            error: Some(error),
            ..Default::default()
        },
    }
}

fn try_check(agent: &ureq::Agent, api_base: &str, installed: &str) -> Result<UpdateState, String> {
    let url = format!(
        "{}/repos/{REPO}/releases/latest",
        api_base.trim_end_matches('/')
    );
    let resp = agent.get(&url).call().map_err(short_err)?;
    let rel: GhRelease = read_json_capped(resp, MAX_BODY)?;

    let latest = strip_v(rel.tag_name.trim()).to_string();
    if latest.is_empty() {
        return Err("release sin tag_name".to_string());
    }

    // Checksums: si el release publicó `SHA256SUMS`, lo bajamos y mapeamos
    // nombre → hash. Si no está o falla, los assets van sin `sha256` y M5 se
    // niega a dar la descarga por buena.
    let checksums = rel
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .map(|a| fetch_sums(agent, &a.browser_download_url).unwrap_or_default())
        .unwrap_or_default();

    let assets = rel
        .assets
        .iter()
        .filter(|a| a.name != "SHA256SUMS")
        .map(|a| Asset {
            name: a.name.clone(),
            url: a.browser_download_url.clone(),
            sha256: checksums.get(&a.name).cloned(),
        })
        .collect();

    let notes = trim_notes(&rel.body);

    Ok(UpdateState {
        checked_at: String::new(), // lo pone `check`
        installed: Some(installed.to_string()),
        latest: Some(latest),
        date: (!rel.published_at.is_empty()).then(|| rel.published_at.clone()),
        url: (!rel.html_url.is_empty()).then(|| rel.html_url.clone()),
        notes: (!notes.is_empty()).then_some(notes),
        assets,
        error: None,
    })
}

fn fetch_sums(agent: &ureq::Agent, url: &str) -> Result<HashMap<String, String>, String> {
    if url.is_empty() {
        return Err("SHA256SUMS sin URL".to_string());
    }
    let resp = agent.get(url).call().map_err(short_err)?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MAX_SUMS + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("lectura de SHA256SUMS: {e}"))?;
    if buf.len() as u64 > MAX_SUMS {
        return Err(format!("SHA256SUMS pasa de {MAX_SUMS} B"));
    }
    Ok(parse_sha256sums(&String::from_utf8_lossy(&buf)))
}

/// Lee el cuerpo de una respuesta con tope duro y lo deserializa. Si el cuerpo
/// pasa de `cap` bytes, error (nunca se parsea de más).
fn read_json_capped<T: serde::de::DeserializeOwned>(
    resp: ureq::Response,
    cap: u64,
) -> Result<T, String> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(cap + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("lectura: {e}"))?;
    if buf.len() as u64 > cap {
        return Err(format!("respuesta pasa de {cap} B"));
    }
    serde_json::from_slice(&buf).map_err(|e| format!("JSON inesperado: {e}"))
}

/// Parsea el formato de `sha256sum(1)`: líneas `<hex64>  <nombre>`. El nombre
/// puede llevar `*` delante (modo binario). Líneas que no cuadran se ignoran.
fn parse_sha256sums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let Some((hash, name)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let hash = hash.trim();
        let name = name.trim().trim_start_matches('*').trim();
        if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && !name.is_empty() {
            map.insert(name.to_string(), hash.to_ascii_lowercase());
        }
    }
    map
}

/// Notas del release: normaliza saltos y recorta a `MAX_NOTES_CHARS` caracteres
/// (por carácter, no por byte: no parte un UTF-8 a la mitad).
fn trim_notes(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    if body.chars().count() <= MAX_NOTES_CHARS {
        return body.trim().to_string();
    }
    let mut s: String = body.chars().take(MAX_NOTES_CHARS).collect();
    s.push('…');
    s.trim().to_string()
}

fn strip_v(s: &str) -> &str {
    s.strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s)
}

/// Mensaje de error **corto**. `ureq::Error` puede acarrear el cuerpo entero de
/// la respuesta; aquí solo queremos "HTTP 403" o "transporte: …".
pub(crate) fn short_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        ureq::Error::Transport(t) => format!("transporte: {}", t.kind()),
    }
}

/// `SystemTime::now()` como `YYYY-MM-DDTHH:MM:SSZ` (UTC), sin depender de
/// `chrono`/`time` (peso). Si el reloj está antes de 1970, cae a la época.
fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, m, s) = ((secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Días desde 1970-01-01 → `(año, mes, día)` gregoriano. Algoritmo de Howard
/// Hinnant ("chrono-Compatible Low-Level Date Algorithms"), dominio público.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (y + i64::from(m <= 2), m as u32, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_sha256sums_formato_coreutils() {
        // Formato de `sha256sum(1)`: `<hex64>` + separador + nombre. El nombre
        // puede llevar `*` delante (modo binario).
        let text = "\
d2c8f9a1e4b7c0d3f6a9b2c5e8d1f4a7b0c3d6e9f2a5b8c1d4e7f0a3b6c9d2e5  wayvpet_0.6.0_amd64.deb
e5d2c9b6a3f0e7d4c1b8a5f2e9d6c3b0a7f4e1d8c5b2a9f6e3d0c7b4a1f8e5d2 *wayvpet-0.6.0.tar.gz
basura sin hash
0bad  demasiado-corto
";
        let m = parse_sha256sums(text);
        assert_eq!(m.len(), 2);
        assert_eq!(
            m.get("wayvpet_0.6.0_amd64.deb").unwrap(),
            "d2c8f9a1e4b7c0d3f6a9b2c5e8d1f4a7b0c3d6e9f2a5b8c1d4e7f0a3b6c9d2e5"
        );
        assert!(
            m.contains_key("wayvpet-0.6.0.tar.gz"),
            "'*' delante del nombre no estorba"
        );
        assert!(!m.contains_key("demasiado-corto"));
    }

    #[test]
    fn recorta_notas_por_caracter() {
        assert_eq!(trim_notes("  hola\r\nmundo  "), "hola\nmundo");
        let largo = "á".repeat(MAX_NOTES_CHARS + 50);
        let out = trim_notes(&largo);
        assert_eq!(out.chars().count(), MAX_NOTES_CHARS + 1, "recorte + '…'");
        assert!(out.ends_with('…'));
    }

    #[test]
    fn civil_from_days_fechas_conocidas() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(18_993), (2022, 1, 1));
        assert_eq!(civil_from_days(20_454), (2026, 1, 1));
        assert_eq!(civil_from_days(20_564), (2026, 4, 21));
    }

    #[test]
    fn now_iso8601_tiene_forma_valida() {
        let s = now_iso8601();
        assert_eq!(s.len(), 20, "{s}");
        assert!(s.ends_with('Z') && s.as_bytes()[10] == b'T', "{s}");
    }
}
