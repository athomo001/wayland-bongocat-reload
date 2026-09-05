//! Parseo y validación de `bongocat.conf` (formato INI propio).
//!
//! `parse_ini` es **puro**: recibe el texto y devuelve una [`Config`] ya
//! validada más la lista de avisos. La lectura de fichero, la resolución de
//! rutas XDG y el escaneo de `/dev/input` viven en los binarios (`bongocat` /
//! `bongocatctl`), no aquí.
//!
//! Portado de `src/config/config.c` a paridad con `tests/test_config.c`.

mod doc;
mod line;

pub use doc::ConfDoc;
pub use line::{split_line, Line};

// ── Rangos de validación (de `src/config/config.c`) ──────────────────────────
const MIN_CAT_HEIGHT: i32 = 10;
const MAX_CAT_HEIGHT: i32 = 200;
const MIN_OVERLAY_HEIGHT: i32 = 20;
const MAX_OVERLAY_HEIGHT: i32 = 300;
const MIN_FPS: i32 = 1;
const MAX_FPS: i32 = 120;
const MIN_DURATION: i32 = 10;
const MAX_DURATION: i32 = 5000;
const MAX_INTERVAL: i32 = 3600;
const NUM_FRAMES: i32 = 5;
const DEFAULT_SCREEN_WIDTH: i32 = 1920;

/// Borde de la pantalla al que se ancla el overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Top,
    Bottom,
}

/// Capa de wlr-layer-shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer {
    Background,
    Bottom,
    #[default]
    Top,
    Overlay,
}

/// Alineación horizontal del gato dentro de la barra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

/// Qué pata usa la actividad del ratón (`enable_mouse`, spec 0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MousePaw {
    Left,
    #[default]
    Right,
    /// Una pata al azar por cada golpecito.
    Random,
}

/// Hora del día para el reposo programado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Time {
    pub hour: i32,
    pub min: i32,
}

impl Time {
    /// Minutos desde medianoche (`hour * 60 + min`).
    #[must_use]
    pub fn minutes(self) -> i32 {
        self.hour * 60 + self.min
    }
}

/// Configuración completa de bongocat. `Config::default()` reproduce
/// `config_set_defaults` del código C.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    // Pantalla / overlay
    pub screen_width: i32,
    pub output_name: Option<String>,
    pub output_names: Vec<String>,
    pub overlay_height: i32,
    pub overlay_opacity: i32,
    pub layer: Layer,
    pub overlay_position: Position,

    // Gato
    pub cat_x_offset: i32,
    pub cat_y_offset: i32,
    pub cat_height: i32,
    /// Opacidad del **gato** en % (0 = invisible, 100 = opaco). Se aplica en el
    /// blit, aparte de `overlay_opacity` (que es el fondo de la barra).
    pub cat_opacity: i32,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub enable_antialiasing: bool,
    pub cat_align: Align,
    /// Tema (skin): nombre en las rutas de temas, o ruta a un directorio.
    /// Vacío = el gato embebido (`classic`). Spec 0006.
    pub theme: String,

    // Animación
    pub idle_frame: i32,
    /// Teclas/minuto a partir de las cuales un tema de sprite sheet con estado
    /// `happy` lo muestra (spec 0014 §5.7 M6). 0 = desactivado. Se cuenta como
    /// un contador de eventos en ventana deslizante, sin identidad de tecla
    /// (spec 0013).
    pub happy_kpm: i32,
    pub keypress_duration: i32,
    pub test_animation_duration: i32,
    pub test_animation_interval: i32,
    /// Antes gobernaba el ritmo de sondeo del bucle de animación; desde que
    /// `bongocat/src/wl.rs::State::next_wake` reprograma el tick a un instante
    /// exacto (patas sueltas / sprite sheet en curso / antirrebote de pantalla
    /// completa, con un tope de reposo aparte), ya no tiene efecto en tiempo de
    /// ejecución. Se conserva parseado/clampado/mostrado por compatibilidad del
    /// `.conf`.
    pub fps: i32,
    pub enable_hand_mapping: bool,

    // Entrada
    pub keyboard_devices: Vec<String>,
    pub keyboard_names: Vec<String>,
    pub hotplug_scan_interval: i32,
    /// Animar una pata con la actividad del ratón físico (spec 0012).
    pub enable_mouse: bool,
    pub mouse_paw: MousePaw,
    /// ms entre golpecitos mientras se mueve el ratón (no uno por evento).
    pub mouse_move_interval: i32,
    pub mouse_devices: Vec<String>,
    pub mouse_names: Vec<String>,

    // Reposo
    pub enable_scheduled_sleep: bool,
    pub sleep_begin: Time,
    pub sleep_end: Time,
    pub idle_sleep_timeout_sec: i32,

    // Pantalla completa / depuración
    pub disable_fullscreen_hide: bool,
    pub enable_debug: bool,
    /// Socket de control IPC (spec 0003). Por defecto activo; `0` lo desactiva.
    pub enable_ipc: bool,
    /// Icono de bandeja (spec 0011, `ksni`+`async-io`). Por defecto activo;
    /// `--no-tray` lo fuerza a 0 para esa ejecución. Sin host SNI en el
    /// escritorio, no molesta (aviso por stderr).
    pub enable_tray: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            screen_width: DEFAULT_SCREEN_WIDTH,
            output_name: None,
            output_names: Vec::new(),
            overlay_height: 50,
            overlay_opacity: 150,
            layer: Layer::Top,
            overlay_position: Position::Top,
            cat_x_offset: 100,
            cat_y_offset: 10,
            cat_height: 40,
            cat_opacity: 100,
            mirror_x: false,
            mirror_y: false,
            enable_antialiasing: true,
            cat_align: Align::Center,
            theme: String::new(),
            idle_frame: 0,
            happy_kpm: 0,
            keypress_duration: 100,
            test_animation_duration: 200,
            test_animation_interval: 0,
            fps: 60,
            enable_hand_mapping: true,
            keyboard_devices: Vec::new(),
            keyboard_names: Vec::new(),
            hotplug_scan_interval: 30,
            enable_mouse: true,
            mouse_paw: MousePaw::Right,
            mouse_move_interval: 50,
            mouse_devices: Vec::new(),
            mouse_names: Vec::new(),
            enable_scheduled_sleep: false,
            sleep_begin: Time { hour: 0, min: 0 },
            sleep_end: Time { hour: 0, min: 0 },
            idle_sleep_timeout_sec: 0,
            disable_fullscreen_hide: false,
            enable_debug: false,
            enable_ipc: true,
            enable_tray: true,
        }
    }
}

/// Parsea el texto de un `bongocat.conf`. Las claves inválidas o desconocidas se
/// **descartan con un aviso**; la configuración resultante siempre es usable.
/// Equivale a `load_config` sin la parte de fichero/dispositivos.
#[must_use]
pub fn parse_ini(input: &str) -> (Config, Vec<String>) {
    let mut cfg = Config::default();
    let mut warnings = Vec::new();

    for (n, raw) in input.lines().enumerate() {
        let lineno = n + 1;
        if is_comment_or_blank(raw) {
            continue;
        }
        let Some(Line { key, value, .. }) = split_line(raw) else {
            warnings.push(format!("línea {lineno} inválida: {}", raw.trim()));
            continue;
        };
        if key.is_empty() {
            warnings.push(format!("línea {lineno} sin clave: {}", raw.trim()));
            continue;
        }
        if let Err(msg) = apply_kv(&mut cfg, &key, &value) {
            warnings.push(format!("línea {lineno}: {msg}"));
        }
    }

    validate(&mut cfg, &mut warnings);
    (cfg, warnings)
}

impl std::str::FromStr for Config {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(parse_ini(s).0)
    }
}

/// `config_is_comment_or_empty`: espacios/tabs iniciales y luego `#` o fin.
/// (`;` NO cuenta como comentario — el C lo trata como línea inválida.)
fn is_comment_or_blank(line: &str) -> bool {
    let t = line.trim_start_matches([' ', '\t']);
    t.is_empty() || t.starts_with('#')
}

fn parse_int(s: &str) -> Option<i32> {
    let t = s.trim();
    // strtol del C rechaza basura final ("60junk"): `i64::from_str` también.
    let v: i64 = t.parse().ok()?;
    if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&v) {
        Some(v as i32)
    } else {
        None
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match parse_int(s)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn parse_time(s: &str) -> Option<Time> {
    let b = s.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return None;
    }
    if !(b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4].is_ascii_digit())
    {
        return None;
    }
    let hour = i32::from(b[0] - b'0') * 10 + i32::from(b[1] - b'0');
    let min = i32::from(b[3] - b'0') * 10 + i32::from(b[4] - b'0');
    if (0..=23).contains(&hour) && (0..=59).contains(&min) {
        Some(Time { hour, min })
    } else {
        None
    }
}

/// Valida un par clave=valor aislado (para el escritor de config, spec 0004:
/// rechazar antes de escribir claves desconocidas o valores mal tipados). No
/// aplica el `clamp` de rango: eso lo hace `validate` al cargar.
///
/// # Errores
/// `Err(msg)` si la clave es desconocida o el valor no tiene el tipo esperado.
pub fn check_kv(key: &str, value: &str) -> Result<(), String> {
    apply_kv(&mut Config::default(), key, value)
}

/// Aplica una clave a una `Config` **viva** (IPC `SET`, spec 0003): valida el
/// tipo y luego recorta a rango igual que al cargar del fichero.
///
/// # Errores
/// `Err(msg)` si la clave es desconocida o el valor no tiene el tipo esperado;
/// en ese caso `cfg` no cambia. `Ok(avisos)` con los mensajes de recorte (vacío
/// si el valor ya estaba en rango).
pub fn set_live(cfg: &mut Config, key: &str, value: &str) -> Result<Vec<String>, String> {
    apply_kv(cfg, key, value)?;
    let mut warnings = Vec::new();
    validate(cfg, &mut warnings);
    Ok(warnings)
}

/// Aplica un par clave=valor ya partido. `Err(msg)` si la clave es desconocida o
/// el valor inválido; en ese caso el campo conserva su valor previo.
fn apply_kv(c: &mut Config, key: &str, value: &str) -> Result<(), String> {
    // Helper: entero validado o error con contexto.
    let int =
        || parse_int(value).ok_or_else(|| format!("valor entero inválido '{value}' para '{key}'"));
    // Helper: booleano 0/1 o error con contexto.
    let boolean = || {
        parse_bool(value).ok_or_else(|| format!("valor booleano inválido '{value}' para '{key}'"))
    };

    match key {
        // ── Enteros con signo (el rango se acota luego en `validate`) ──
        "cat_x_offset" => c.cat_x_offset = int()?,
        "cat_y_offset" => c.cat_y_offset = int()?,
        "cat_height" => c.cat_height = int()?,
        "cat_opacity" => c.cat_opacity = int()?,
        "overlay_height" => c.overlay_height = int()?,
        "overlay_opacity" => c.overlay_opacity = int()?,
        "idle_frame" => c.idle_frame = int()?,
        "happy_kpm" => c.happy_kpm = int()?,
        "keypress_duration" => c.keypress_duration = int()?,
        "test_animation_duration" => c.test_animation_duration = int()?,
        "test_animation_interval" => c.test_animation_interval = int()?,
        "fps" => c.fps = int()?,
        "hotplug_scan_interval" => c.hotplug_scan_interval = int()?,
        "idle_sleep_timeout" => c.idle_sleep_timeout_sec = int()?,
        "mouse_move_interval" => c.mouse_move_interval = int()?,

        // ── Booleanos (0/1) ──
        "mirror_x" => c.mirror_x = boolean()?,
        "mirror_y" => c.mirror_y = boolean()?,
        "enable_antialiasing" => c.enable_antialiasing = boolean()?,
        "enable_hand_mapping" => c.enable_hand_mapping = boolean()?,
        "enable_debug" => c.enable_debug = boolean()?,
        "enable_ipc" => c.enable_ipc = boolean()?,
        "enable_tray" => c.enable_tray = boolean()?,
        "enable_scheduled_sleep" => c.enable_scheduled_sleep = boolean()?,
        "disable_fullscreen_hide" => c.disable_fullscreen_hide = boolean()?,

        // ── Enums (valor desconocido → error; el campo conserva el default) ──
        "layer" => {
            c.layer = match value {
                "background" => Layer::Background,
                "bottom" => Layer::Bottom,
                "top" => Layer::Top,
                "overlay" => Layer::Overlay,
                _ => return Err(format!("layer '{value}' inválido, se usa 'top'")),
            }
        }
        "overlay_position" => {
            c.overlay_position = match value {
                "top" => Position::Top,
                "bottom" => Position::Bottom,
                _ => return Err(format!("overlay_position '{value}' inválido, se usa 'top'")),
            }
        }
        "cat_align" => {
            c.cat_align = match value {
                "left" => Align::Left,
                "center" => Align::Center,
                "right" => Align::Right,
                _ => return Err(format!("cat_align '{value}' inválido, se usa 'center'")),
            }
        }
        "theme" => {
            if value.contains("..") {
                return Err(format!("path traversal en theme: {value}"));
            }
            c.theme = value.to_string();
        }

        // ── Horas HH:MM ──
        "sleep_begin" => {
            c.sleep_begin = parse_time(value)
                .ok_or_else(|| format!("hora inválida '{value}' para '{key}', se espera HH:MM"))?;
        }
        "sleep_end" => {
            c.sleep_end = parse_time(value)
                .ok_or_else(|| format!("hora inválida '{value}' para '{key}', se espera HH:MM"))?;
        }

        // ── Cadenas / listas ──
        "monitor" => {
            c.output_names = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            c.output_name = c.output_names.first().cloned();
        }
        "keyboard_name" => c.keyboard_names.push(value.to_string()),
        "keyboard_device" | "keyboard_devices" => {
            validate_input_path(value)?;
            c.keyboard_devices.push(value.to_string());
        }
        "mouse_name" => c.mouse_names.push(value.to_string()),
        "mouse_device" | "mouse_devices" => {
            validate_input_path(value)?;
            c.mouse_devices.push(value.to_string());
        }
        "enable_mouse" => c.enable_mouse = boolean()?,
        "mouse_paw" => {
            c.mouse_paw = match value {
                "left" => MousePaw::Left,
                "right" => MousePaw::Right,
                "random" => MousePaw::Random,
                _ => return Err(format!("mouse_paw '{value}' inválido (left|right|random)")),
            }
        }

        _ => return Err(format!("clave desconocida '{key}'")),
    }
    Ok(())
}

/// Valida una ruta de `/dev/input/` (teclado o ratón): prefijo obligatorio y sin
/// `..` (spec 0013: nadie mete rutas raras por la config).
fn validate_input_path(value: &str) -> Result<(), String> {
    if !value.starts_with("/dev/input/") {
        return Err(format!(
            "la ruta de dispositivo debe empezar por /dev/input/: {value}"
        ));
    }
    if value.contains("..") {
        return Err(format!("path traversal en la ruta de dispositivo: {value}"));
    }
    Ok(())
}

fn clamp(v: &mut i32, lo: i32, hi: i32, name: &str, warnings: &mut Vec<String>) {
    if *v < lo || *v > hi {
        warnings.push(format!("{name} {v} fuera de rango [{lo}-{hi}], se ajusta"));
        *v = (*v).clamp(lo, hi);
    }
}

/// `config_validate`: clamps de rango y normalizaciones.
fn validate(c: &mut Config, warnings: &mut Vec<String>) {
    clamp(
        &mut c.cat_height,
        MIN_CAT_HEIGHT,
        MAX_CAT_HEIGHT,
        "cat_height",
        warnings,
    );
    clamp(
        &mut c.overlay_height,
        MIN_OVERLAY_HEIGHT,
        MAX_OVERLAY_HEIGHT,
        "overlay_height",
        warnings,
    );
    clamp(&mut c.fps, MIN_FPS, MAX_FPS, "fps", warnings);
    clamp(
        &mut c.keypress_duration,
        MIN_DURATION,
        MAX_DURATION,
        "keypress_duration",
        warnings,
    );
    clamp(
        &mut c.test_animation_duration,
        MIN_DURATION,
        MAX_DURATION,
        "test_animation_duration",
        warnings,
    );
    clamp(
        &mut c.test_animation_interval,
        0,
        MAX_INTERVAL,
        "test_animation_interval",
        warnings,
    );
    clamp(
        &mut c.hotplug_scan_interval,
        0,
        MAX_INTERVAL,
        "hotplug_scan_interval",
        warnings,
    );
    clamp(
        &mut c.idle_sleep_timeout_sec,
        0,
        MAX_INTERVAL,
        "idle_sleep_timeout",
        warnings,
    );
    clamp(
        &mut c.mouse_move_interval,
        10,
        2000,
        "mouse_move_interval",
        warnings,
    );
    clamp(&mut c.overlay_opacity, 0, 255, "overlay_opacity", warnings);
    clamp(&mut c.cat_opacity, 0, 100, "cat_opacity", warnings);
    clamp(&mut c.happy_kpm, 0, 10_000, "happy_kpm", warnings);

    if c.idle_frame < 0 || c.idle_frame >= NUM_FRAMES {
        warnings.push(format!(
            "idle_frame {} fuera de rango [0-{}], se pone a 0",
            c.idle_frame,
            NUM_FRAMES - 1
        ));
        c.idle_frame = 0;
    }

    // Reposo programado con inicio == fin: se desactiva (como el C).
    if c.enable_scheduled_sleep && c.sleep_begin.minutes() == c.sleep_end.minutes() {
        warnings
            .push("reposo programado activado pero sleep_begin == sleep_end; se desactiva".into());
        c.enable_scheduled_sleep = false;
    }
}

impl Layer {
    fn as_str(self) -> &'static str {
        match self {
            Layer::Background => "background",
            Layer::Bottom => "bottom",
            Layer::Top => "top",
            Layer::Overlay => "overlay",
        }
    }
}

impl Position {
    fn as_str(self) -> &'static str {
        match self {
            Position::Top => "top",
            Position::Bottom => "bottom",
        }
    }
}

impl Align {
    fn as_str(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

impl MousePaw {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MousePaw::Left => "left",
            MousePaw::Right => "right",
            MousePaw::Random => "random",
        }
    }
}

impl Config {
    /// Emite la configuración como INI plano (sin comentarios). Sirve para
    /// `bongocat --print-default-config` y como base de un fichero nuevo. El
    /// escritor que **conserva** comentarios es otra cosa (spec 0004).
    /// Vuelve a parsearse a una `Config` idéntica.
    #[must_use]
    pub fn to_ini(&self) -> String {
        let b = |v: bool| if v { 1 } else { 0 };
        let mut s = String::new();
        let mut w = |k: &str, v: &dyn std::fmt::Display| {
            s.push_str(&format!("{k}={v}\n"));
        };
        w("cat_height", &self.cat_height);
        w("cat_opacity", &self.cat_opacity);
        w("cat_align", &self.cat_align.as_str());
        if !self.theme.is_empty() {
            w("theme", &self.theme);
        }
        w("cat_x_offset", &self.cat_x_offset);
        w("cat_y_offset", &self.cat_y_offset);
        w("mirror_x", &b(self.mirror_x));
        w("mirror_y", &b(self.mirror_y));
        w("enable_antialiasing", &b(self.enable_antialiasing));
        w("overlay_height", &self.overlay_height);
        w("overlay_opacity", &self.overlay_opacity);
        w("overlay_position", &self.overlay_position.as_str());
        w("layer", &self.layer.as_str());
        w("fps", &self.fps);
        w("idle_frame", &self.idle_frame);
        w("happy_kpm", &self.happy_kpm);
        w("keypress_duration", &self.keypress_duration);
        w("enable_hand_mapping", &b(self.enable_hand_mapping));
        w("test_animation_duration", &self.test_animation_duration);
        w("test_animation_interval", &self.test_animation_interval);
        w("hotplug_scan_interval", &self.hotplug_scan_interval);
        w("idle_sleep_timeout", &self.idle_sleep_timeout_sec);
        w("enable_scheduled_sleep", &b(self.enable_scheduled_sleep));
        w(
            "sleep_begin",
            &format!("{:02}:{:02}", self.sleep_begin.hour, self.sleep_begin.min),
        );
        w(
            "sleep_end",
            &format!("{:02}:{:02}", self.sleep_end.hour, self.sleep_end.min),
        );
        w("disable_fullscreen_hide", &b(self.disable_fullscreen_hide));
        w("enable_debug", &b(self.enable_debug));
        w("enable_ipc", &b(self.enable_ipc));
        w("enable_tray", &b(self.enable_tray));
        for dev in &self.keyboard_devices {
            w("keyboard_device", dev);
        }
        for name in &self.keyboard_names {
            w("keyboard_name", name);
        }
        w("enable_mouse", &b(self.enable_mouse));
        w("mouse_paw", &self.mouse_paw.as_str());
        w("mouse_move_interval", &self.mouse_move_interval);
        for dev in &self.mouse_devices {
            w("mouse_device", dev);
        }
        for name in &self.mouse_names {
            w("mouse_name", name);
        }
        if !self.output_names.is_empty() {
            w("monitor", &self.output_names.join(","));
        }
        s
    }
}

#[cfg(test)]
mod tests;
