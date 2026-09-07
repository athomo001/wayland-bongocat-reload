//! Disparo del chequeo de nueva versión (spec 0015, hito M3).
//!
//! El supervisor, **una sola vez al arrancar** y después de montar el tray,
//! lanza el helper `wayvpet-update-check` como proceso hijo corto y se olvida.
//! No hay timer, ni cron, ni servicio de fondo, ni re-chequeo mientras corre.
//!
//! `wayvpet` **no** enlaza el cliente HTTPS: vive en el binario aparte
//! `wayvpet-update-check` (crate `wayvpet-update`, paquete `Recommends:`). Si el
//! binario no está instalado, aquí no pasa nada.

use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use wayvpet_common::io;

/// Nombre del helper; se busca en el `PATH`.
const HELPER: &str = "wayvpet-update-check";

/// Suelo anti-abuso: no relanzar el chequeo si el último fue hace menos de esto.
/// **No es un "poll"** (no hay reintento mientras corre): es para no martillear
/// la API de GitHub si wayvpet se reinicia en bucle. Cerrar y volver a abrir
/// wayvpet tras este plazo sí hace un chequeo nuevo.
const FLOOR: Duration = Duration::from_secs(60 * 60);

/// Lanza `wayvpet-update-check` en segundo plano si procede. No bloquea, no
/// espera, y cosecha al hijo en un hilo aparte (sin zombis).
///
/// Cualquier motivo para no lanzarlo es **silencioso**: opción apagada, canal
/// `distro`, un chequeo reciente, o el helper no instalado.
pub fn maybe_spawn(check_updates: bool) {
    if !should_check(
        check_updates,
        io::install_channel_real().as_deref(),
        last_check_age(),
    ) {
        return;
    }
    let spawned = Command::new(HELPER)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    // `Err` = helper ausente (paquete `wayvpet-update` no instalado): sin ruido.
    if let Ok(mut child) = spawned {
        // Cosecha en un hilo: ni zombi ni bloqueo del arranque.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Decisión pura, separada de [`maybe_spawn`] para poder probarla sin tocar el
/// sistema. `last_check_age` = antigüedad del fichero de estado, o `None` si no
/// existe (nunca se comprobó).
fn should_check(
    check_updates: bool,
    channel: Option<&str>,
    last_check_age: Option<Duration>,
) -> bool {
    if !check_updates {
        return false;
    }
    if channel == Some("distro") {
        // Instalado desde los repos de la distro: actualiza el gestor de
        // paquetes; el aviso se calla (spec 0015 §"Marca del canal").
        return false;
    }
    match last_check_age {
        Some(age) => age >= FLOOR,
        None => true,
    }
}

/// Antigüedad del `update-check.json` por su `mtime` (lo reescribe atómico el
/// helper en cada ejecución). Usar el `mtime` evita que `wayvpet` tenga que
/// parsear JSON solo para el suelo anti-abuso. `None` si el fichero no existe.
fn last_check_age() -> Option<Duration> {
    let path = io::update_state_path_real()?;
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    // `mtime` en el futuro (reloj hacia atrás): trátalo como "recién comprobado".
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO),
    )
}

/// Aviso de versión nueva pendiente (spec 0015 M4). Lo produce el helper en el
/// fichero plano `update-notice`; `wayvpet` solo lee dos claves, sin JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Versión disponible, sin la `v` (`0.6.0`).
    pub version: String,
    /// Página del release para "Ver en el navegador". Puede venir vacía.
    pub url: String,
}

/// Lee el aviso pendiente, o `None` si no hay versión nueva. Se relee cada vez
/// (el helper puede haberlo escrito hace un instante, ya con el tray montado).
/// Con canal `distro` nunca hay aviso: actualiza el gestor de paquetes.
pub fn pending_notice() -> Option<Notice> {
    if io::install_channel_real().as_deref() == Some("distro") {
        return None;
    }
    let notice = io::update_state_path_real()?.with_file_name("update-notice");
    parse_notice(&std::fs::read_to_string(&notice).ok()?)
}

/// `latest=` / `url=`, una por línea. `None` si no hay una `latest` no vacía
/// (fichero ausente, a medias, o borrado por el helper porque ya estás al día).
fn parse_notice(text: &str) -> Option<Notice> {
    let mut version = None;
    let mut url = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("latest=") {
            version = Some(v.trim().to_string());
        } else if let Some(u) = line.strip_prefix("url=") {
            url = u.trim().to_string();
        }
    }
    let version = version.filter(|s| !s.is_empty())?;
    Some(Notice { version, url })
}

/// Acción del ítem del tray (spec 0015 M4/M5): abre el diálogo de
/// `wayvpet-config` (`--update-dialog`) si la ventana está instalada; si no, la
/// página del release en el navegador. Sin `url` ni ventana, no hace nada.
pub fn open_notice(url: &str) {
    if crate::tray::config_gui_available() {
        spawn_detached(Command::new("wayvpet-config").arg("--update-dialog"));
    } else if !url.is_empty() {
        spawn_detached(Command::new("xdg-open").arg(url));
    }
}

/// Lanza el comando sin heredar E/S y lo cosecha en un hilo (sin zombis).
fn spawn_detached(cmd: &mut Command) {
    let spawned = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dispara_con_check_updates_apagado() {
        assert!(!should_check(false, None, None));
        assert!(!should_check(
            false,
            Some("deb"),
            Some(Duration::from_secs(0))
        ));
    }

    #[test]
    fn dispara_si_nunca_se_comprobo() {
        assert!(should_check(true, None, None));
        assert!(should_check(true, Some("source"), None));
        assert!(should_check(true, Some("deb"), None));
    }

    #[test]
    fn respeta_el_suelo_de_una_hora() {
        // Chequeo reciente (< 1 h): no.
        assert!(!should_check(
            true,
            None,
            Some(Duration::from_secs(59 * 60))
        ));
        // Justo en el suelo o más viejo: sí.
        assert!(should_check(true, None, Some(FLOOR)));
        assert!(should_check(
            true,
            None,
            Some(Duration::from_secs(3 * 3600))
        ));
    }

    #[test]
    fn canal_distro_nunca_dispara() {
        assert!(!should_check(true, Some("distro"), None));
        assert!(!should_check(
            true,
            Some("distro"),
            Some(Duration::from_secs(999_999))
        ));
        // Otros canales sí.
        assert!(should_check(true, Some("arch"), None));
    }

    #[test]
    fn parse_notice_saca_version_y_url() {
        // Formato que escribe el helper: `clave=valor`, una por línea.
        let n = parse_notice("latest=0.6.0\nurl=https://x/rel\n").unwrap();
        assert_eq!(n.version, "0.6.0");
        assert_eq!(n.url, "https://x/rel");
        // el orden entre líneas no importa; se ignoran líneas en blanco
        let n = parse_notice("\nurl=https://y\nlatest=1.0.0\n").unwrap();
        assert_eq!((n.version.as_str(), n.url.as_str()), ("1.0.0", "https://y"));
        // url ausente → aviso igual, sin enlace
        assert_eq!(parse_notice("latest=2.0.0\n").unwrap().url, "");
    }

    #[test]
    fn parse_notice_none_si_no_hay_latest() {
        assert!(parse_notice("").is_none());
        assert!(parse_notice("url=https://x\n").is_none());
        assert!(
            parse_notice("latest=\n").is_none(),
            "latest vacía no cuenta"
        );
        assert!(parse_notice("basura\n").is_none());
    }
}
