//! Formato de tema **`theme_format = 3`**: sprite sheet PNG en rejilla, como
//! wayland-vpets (spec 0014). Aquí vive la parte **pura**: parseo del `theme.ini`
//! de rejilla, cálculo del rectángulo de cada frame, recorte de un frame de un
//! búfer RGBA/BGRA ya decodificado, y escalado **nearest-neighbor a escala
//! entera** (pixel-art). La decodificación PNG/APNG/GIF vive en el binario.

use crate::config::split_line;

/// Filtro de escalado de un tema de rejilla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleFilter {
    /// Nearest-neighbor a escala entera (pixel-art). Por defecto en formato 3.
    #[default]
    Nearest,
    /// Escalado suave (bilineal).
    Linear,
}

/// Modelo de entrada de la mascota (spec 0014 §5.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputModel {
    /// Con manos (bongo cat): poses izquierda/derecha/ambas.
    #[default]
    Hands,
    /// De actividad (la mayoría de vpets): cualquier actividad → un único estado.
    Activity,
}

/// Cómo se ancla la mascota en la barra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    /// Se apoya en el borde (como el gato).
    #[default]
    Baseline,
    /// Flota centrada verticalmente (mascotas voladoras).
    Center,
}

/// Un estado de la mascota: fila de la rejilla + cuántos frames + ritmo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetState {
    /// Nombre del estado (`idle`, `writing`, `sleep`, … los 15 de wayland-vpets).
    pub name: String,
    /// Fila de la rejilla (ya normalizada a 0-based).
    pub row: u32,
    /// Nº de frames del estado (columnas consecutivas).
    pub frames: u32,
    /// Fotogramas por segundo del estado.
    pub fps: u32,
    /// Columna inicial (0-based).
    pub col_start: u32,
}

/// Tema de rejilla ya parseado (metadatos + estados). No incluye los píxeles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetTheme {
    pub frame_w: u32,
    pub frame_h: u32,
    pub scale_filter: ScaleFilter,
    pub default_fps: u32,
    pub input_model: InputModel,
    pub anchor: Anchor,
    /// Nombre de la hoja única (`sheet =`), si no hay una por estado.
    pub sheet: Option<String>,
    pub states: Vec<SheetState>,
}

impl Default for SheetTheme {
    fn default() -> Self {
        Self {
            frame_w: 0,
            frame_h: 0,
            scale_filter: ScaleFilter::Nearest,
            default_fps: 12,
            input_model: InputModel::Activity, // los packs de vpets suelen no tener manos
            anchor: Anchor::Baseline,
            sheet: None,
            states: Vec::new(),
        }
    }
}

impl SheetTheme {
    /// Busca un estado por nombre.
    #[must_use]
    pub fn state(&self, name: &str) -> Option<&SheetState> {
        self.states.iter().find(|s| s.name == name)
    }
}

/// Acumulador de claves `state_<n>_*` mientras se parsea.
#[derive(Default)]
struct StateAccum {
    row: Option<u32>,
    frames: Option<u32>,
    fps: Option<u32>,
    col_start: Option<u32>,
}

/// Parsea un `theme.ini` de `theme_format = 3`. Claves desconocidas se ignoran;
/// nunca falla (los valores ausentes quedan por defecto / vacíos — la validación
/// de que hay algo usable la hace el llamante).
#[must_use]
pub fn parse_sheet_ini(text: &str) -> SheetTheme {
    use std::collections::BTreeMap;
    let mut t = SheetTheme::default();
    let mut per_state: BTreeMap<String, StateAccum> = BTreeMap::new();
    let mut row_base: u32 = 1;
    let mut sheets_per_state: BTreeMap<String, String> = BTreeMap::new();

    for raw in text.lines() {
        let s = raw.trim_start_matches([' ', '\t']);
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let Some(l) = split_line(raw) else { continue };
        let (k, v) = (l.key, l.value);
        match k.as_str() {
            "frame_w" => t.frame_w = v.parse().unwrap_or(0),
            "frame_h" => t.frame_h = v.parse().unwrap_or(0),
            "default_fps" | "fps" | "animation_speed" => {
                if let Ok(n) = v.parse::<u32>() {
                    if n > 0 {
                        t.default_fps = n;
                    }
                }
            }
            "row_base" => row_base = v.parse().unwrap_or(1),
            "sheet" | "custom_sprite_sheet_filename" => t.sheet = Some(v),
            "scale_filter" => {
                t.scale_filter = match v.as_str() {
                    "linear" => ScaleFilter::Linear,
                    _ => ScaleFilter::Nearest,
                }
            }
            "input_model" => {
                t.input_model = match v.as_str() {
                    "hands" => InputModel::Hands,
                    _ => InputModel::Activity,
                }
            }
            "anchor" => {
                t.anchor = match v.as_str() {
                    "center" => Anchor::Center,
                    _ => Anchor::Baseline,
                }
            }
            _ => {
                // state_<n>_row / _frames / _fps / _col_start ; sheet_<n>
                if let Some(rest) = k.strip_prefix("sheet_") {
                    sheets_per_state.insert(rest.to_string(), v);
                } else if let Some(rest) = k.strip_prefix("state_") {
                    let (name, field) = match rest.rsplit_once('_') {
                        Some(x) => x,
                        None => continue,
                    };
                    let slot = per_state.entry(name.to_string()).or_default();
                    let n: Option<u32> = v.parse().ok();
                    match field {
                        "row" => slot.row = n,
                        "frames" => slot.frames = n,
                        "fps" => slot.fps = n,
                        "col_start" | "col" => slot.col_start = n,
                        _ => {}
                    }
                }
            }
        }
    }

    for (name, acc) in per_state {
        // Un estado necesita al menos fila y nº de frames para ser utilizable.
        let (Some(row), Some(frames)) = (acc.row, acc.frames) else {
            continue;
        };
        if frames == 0 {
            continue;
        }
        t.states.push(SheetState {
            name,
            row: row.saturating_sub(row_base),
            frames,
            fps: acc.fps.filter(|&n| n > 0).unwrap_or(t.default_fps),
            col_start: acc.col_start.unwrap_or(row_base).saturating_sub(row_base),
        });
    }
    // `sheet_<estado>` no se guarda aquí (el llamante lo lee por estado); dejamos
    // `sheet` a None si no había una global.
    let _ = sheets_per_state;
    t
}

/// Rectángulo `(x, y, w, h)` en píxeles del frame `i` de `state` dentro de la
/// hoja. `i` se recorta a `[0, frames)`.
#[must_use]
pub fn frame_rect(sheet: &SheetTheme, state: &SheetState, i: u32) -> (u32, u32, u32, u32) {
    let i = i.min(state.frames.saturating_sub(1));
    let x = (state.col_start + i) * sheet.frame_w;
    let y = state.row * sheet.frame_h;
    (x, y, sheet.frame_w, sheet.frame_h)
}

/// Recorta el rect `(rx, ry, rw, rh)` de un búfer RGBA/BGRA (`src_w`×`src_h`,
/// 4 bytes/px) a un `Vec` nuevo. Rellena de 0 lo que caiga fuera de la hoja.
#[must_use]
pub fn crop_frame(src: &[u8], src_w: u32, src_h: u32, rect: (u32, u32, u32, u32)) -> Vec<u8> {
    let (rx, ry, rw, rh) = rect;
    let mut out = vec![0u8; (rw * rh * 4) as usize];
    for y in 0..rh {
        let sy = ry + y;
        if sy >= src_h {
            break;
        }
        for x in 0..rw {
            let sx = rx + x;
            if sx >= src_w {
                break;
            }
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((y * rw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

/// Factor de escala **entero** para llevar un frame de alto `frame_h` a una
/// altura objetivo `target_h` (mínimo 1). Nunca amplía a un factor no entero:
/// el pixel-art se mantiene nítido.
#[must_use]
pub fn integer_scale(frame_h: u32, target_h: u32) -> u32 {
    if frame_h == 0 {
        return 1;
    }
    (target_h / frame_h).max(1)
}

/// Escala `src` (RGBA/BGRA, `w`×`h`) por un factor entero `k` con
/// nearest-neighbor. Devuelve `(bytes, w*k, h*k)`.
#[must_use]
pub fn scale_nearest(src: &[u8], w: u32, h: u32, k: u32) -> (Vec<u8>, u32, u32) {
    let k = k.max(1);
    let (ow, oh) = (w * k, h * k);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for oy in 0..oh {
        let sy = oy / k;
        for ox in 0..ow {
            let sx = ox / k;
            let si = ((sy * w + sx) * 4) as usize;
            let di = ((oy * ow + ox) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    (out, ow, oh)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "\
theme_format = 3
name = Charizard
sheet = charizard.png
frame_w = 64
frame_h = 48
default_fps = 12
input_model = activity
anchor = center
state_idle_row = 1
state_idle_frames = 2
state_writing_row = 4
state_writing_frames = 6
state_writing_fps = 16
state_writing_col = 2
";

    #[test]
    fn parsea_rejilla_y_estados() {
        let t = parse_sheet_ini(INI);
        assert_eq!((t.frame_w, t.frame_h), (64, 48));
        assert_eq!(t.scale_filter, ScaleFilter::Nearest);
        assert_eq!(t.input_model, InputModel::Activity);
        assert_eq!(t.anchor, Anchor::Center);
        assert_eq!(t.sheet.as_deref(), Some("charizard.png"));

        let idle = t.state("idle").unwrap();
        assert_eq!(
            (idle.row, idle.frames, idle.fps),
            (0, 2, 12),
            "row 1-based -> 0"
        );
        let w = t.state("writing").unwrap();
        assert_eq!((w.row, w.frames, w.fps, w.col_start), (3, 6, 16, 1));
    }

    #[test]
    fn row_base_0_no_desplaza() {
        let t = parse_sheet_ini(
            "row_base = 0\nframe_w=8\nframe_h=8\nstate_idle_row=0\nstate_idle_frames=1\n",
        );
        assert_eq!(t.state("idle").unwrap().row, 0);
    }

    #[test]
    fn frame_rect_avanza_por_columnas() {
        let t = parse_sheet_ini(INI);
        let w = t.state("writing").unwrap();
        assert_eq!(
            frame_rect(&t, w, 0),
            (64, 144, 64, 48),
            "col_start 1 -> x=64, row 3 -> y=144"
        );
        assert_eq!(frame_rect(&t, w, 2), (192, 144, 64, 48));
        // i=99 -> se recorta a frames-1 = 5; columna = col_start(1) + 5 = 6.
        assert_eq!(frame_rect(&t, w, 99), (6 * 64, 144, 64, 48));
    }

    #[test]
    fn crop_y_escalado_entero() {
        // hoja 4x2, cada píxel un color por su índice de fila*4+col en el canal R.
        let mut sheet = vec![0u8; 4 * 2 * 4];
        for y in 0..2u32 {
            for x in 0..4u32 {
                sheet[((y * 4 + x) * 4) as usize] = (y * 4 + x) as u8;
            }
        }
        // recorta el frame (2,0,2,2) -> píxeles R = [2,3, 6,7]
        let f = crop_frame(&sheet, 4, 2, (2, 0, 2, 2));
        assert_eq!([f[0], f[4], f[8], f[12]], [2, 3, 6, 7]);

        assert_eq!(integer_scale(48, 96), 2);
        assert_eq!(integer_scale(48, 100), 2, "no amplía a factor no entero");
        assert_eq!(integer_scale(48, 20), 1, "mínimo 1");

        let (big, bw, bh) = scale_nearest(&f, 2, 2, 2);
        assert_eq!((bw, bh), (4, 4));
        // cada píxel original se replica 2x2; esquina sup-izq sigue siendo R=2
        assert_eq!(big[0], 2);
        assert_eq!(big[4], 2, "réplica horizontal");
        assert_eq!(big[(4 * 4) as usize], 2, "réplica vertical");
    }
}
