//! Formato de tema **`theme_format = 3`**: sprite sheet PNG en rejilla, como
//! wayland-vpets (spec 0014). Aquí vive la parte **pura**: parseo del `theme.ini`
//! de rejilla, cálculo del rectángulo de cada frame, recorte de un frame de un
//! búfer RGBA/BGRA ya decodificado, y escalado **nearest-neighbor a escala
//! entera** (pixel-art). La decodificación PNG/APNG/GIF vive en el binario.

use std::collections::BTreeMap;

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
    /// Nombre de la hoja única (`sheet =` / `custom_sprite_sheet_filename =`).
    /// Es el respaldo para cualquier estado sin hoja propia.
    pub sheet: Option<String>,
    /// Hojas por estado (`sheet_<estado> =`): nombre de estado → fichero. Tienen
    /// prioridad sobre `sheet`.
    pub sheets_per_state: BTreeMap<String, String>,
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
            sheets_per_state: BTreeMap::new(),
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

    /// Fichero de hoja del que sale el estado `name`: su `sheet_<estado> =` si lo
    /// tiene, si no la hoja global. `None` = no hay ninguna fuente para ese
    /// estado (tema mal formado).
    #[must_use]
    pub fn sheet_for(&self, name: &str) -> Option<&str> {
        self.sheets_per_state
            .get(name)
            .or(self.sheet.as_ref())
            .map(String::as_str)
    }

    /// Todos los ficheros de hoja que el tema referencia (global + por estado),
    /// sin repetidos.
    #[must_use]
    pub fn sheet_files(&self) -> std::collections::BTreeSet<&str> {
        let mut set = std::collections::BTreeSet::new();
        if let Some(g) = &self.sheet {
            set.insert(g.as_str());
        }
        for f in self.sheets_per_state.values() {
            set.insert(f.as_str());
        }
        set
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
    let mut t = SheetTheme::default();
    let mut per_state: BTreeMap<String, StateAccum> = BTreeMap::new();
    let mut row_base: u32 = 1;
    // ¿Trae el `.ini` un `input_model =` explícito? Si no, se autodetecta al
    // final por la presencia de poses izquierda/derecha (spec 0014 §5.7).
    let mut input_model_set = false;

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
                input_model_set = true;
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
                    t.sheets_per_state.insert(rest.to_string(), v);
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

    // `state_<n>_fps` declarado sin `_row`/`_frames`: se usa abajo para el
    // estado "solo hoja" (un APNG lleva su propio nº de frames, pero el ritmo
    // sí puede fijarse).
    let solo_hoja_fps: BTreeMap<String, u32> = per_state
        .iter()
        .filter_map(|(n, a)| a.fps.filter(|&f| f > 0).map(|f| (n.clone(), f)))
        .collect();

    for (name, acc) in per_state {
        // Un estado de **rejilla** necesita fila y nº de frames para ser
        // utilizable; sin ellos se deja para la ronda de "solo hoja" de abajo.
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

    // Estados con **hoja propia** (`sheet_<estado> =`, típicamente un APNG) y sin
    // ningún `state_<estado>_*`: los fotogramas vienen del fichero, así que `row`
    // y `frames` no aplican (quedan a 0) y el ritmo es `default_fps`.
    let solo_hoja: Vec<String> = t
        .sheets_per_state
        .keys()
        .filter(|k| !t.states.iter().any(|s| &s.name == *k))
        .cloned()
        .collect();
    for name in solo_hoja {
        let fps = solo_hoja_fps.get(&name).copied().unwrap_or(t.default_fps);
        t.states.push(SheetState {
            name,
            row: 0,
            frames: 0,
            fps,
            col_start: 0,
        });
    }

    // Autodetección de `input_model` (spec 0014 §5.7) si no vino explícito: un
    // tema con poses izquierda/derecha es "con manos"; el resto, "de actividad".
    if !input_model_set {
        let has_hand_poses = t.states.iter().any(|s| is_hand_pose(&s.name));
        t.input_model = if has_hand_poses {
            InputModel::Hands
        } else {
            InputModel::Activity
        };
    }
    t
}

/// ¿Es `name` una de las poses del modelo "con manos" (izquierda / derecha /
/// ambas), en cualquiera de sus alias?
#[must_use]
fn is_hand_pose(name: &str) -> bool {
    matches!(
        name,
        "active_left"
            | "left_down"
            | "left-down"
            | "active_right"
            | "right_down"
            | "right-down"
            | "active_both"
            | "both_down"
            | "both-down"
    )
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

/// Escala `src` por un factor entero `k` con **interpolación bilineal**. Pensado
/// para temas con `scale_filter = linear` (arte no pixel-art que prefiere bordes
/// suaves). `src` debe estar en **alfa premultiplicado** para que la mezcla no
/// arrastre color de los píxeles transparentes. Devuelve `(bytes, w*k, h*k)`.
#[must_use]
pub fn scale_bilinear(src: &[u8], w: u32, h: u32, k: u32) -> (Vec<u8>, u32, u32) {
    let k = k.max(1);
    let (ow, oh) = (w * k, h * k);
    if k == 1 || w == 0 || h == 0 {
        return (src.to_vec(), ow, oh);
    }
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    let kf = k as f32;
    let (wmax, hmax) = ((w - 1) as f32, (h - 1) as f32);
    let sample =
        |x: u32, y: u32, c: usize| -> f32 { f32::from(src[((y * w + x) * 4) as usize + c]) };
    for oy in 0..oh {
        // Centro del texel de salida en coordenadas de origen.
        let fy = ((oy as f32 + 0.5) / kf - 0.5).clamp(0.0, hmax);
        let y0 = fy as u32;
        let y1 = (y0 + 1).min(h - 1);
        let wy = fy - y0 as f32;
        for ox in 0..ow {
            let fx = ((ox as f32 + 0.5) / kf - 0.5).clamp(0.0, wmax);
            let x0 = fx as u32;
            let x1 = (x0 + 1).min(w - 1);
            let wx = fx - x0 as f32;
            let di = ((oy * ow + ox) * 4) as usize;
            for c in 0..4 {
                let top = sample(x0, y0, c) * (1.0 - wx) + sample(x1, y0, c) * wx;
                let bot = sample(x0, y1, c) * (1.0 - wx) + sample(x1, y1, c) * wx;
                out[di + c] = (top * (1.0 - wy) + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    (out, ow, oh)
}

/// Convierte un frame de **RGBA recto** (el que entrega un PNG) a **BGRA
/// premultiplicado** in situ — el formato que consume `anim::blit_over` y que
/// espera `WL_SHM_FORMAT_ARGB8888`. Cada canal de color se multiplica por el
/// alfa (`c·a/255`, redondeado) y se intercambian R↔B. Con `a = 255` la
/// operación es solo el swap (exacta); con `a = 0` el píxel queda a cero.
pub fn premul_bgra_from_straight_rgba(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        let mul = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
        let (r, g, b) = (px[0], px[1], px[2]);
        px[0] = mul(b);
        px[1] = mul(g);
        px[2] = mul(r);
        // px[3] (alfa) no cambia.
    }
}

/// Primer estado de `wanted` (por nombre) que el tema define. Semilla de la
/// "regla de oro" de la spec 0014 §5.2: si el estado pedido no existe se cae al
/// siguiente candidato; el llamante decide el último recurso.
#[must_use]
pub fn pick_state<'a>(sheet: &'a SheetTheme, wanted: &[&str]) -> Option<&'a SheetState> {
    wanted.iter().find_map(|name| sheet.state(name))
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
    fn input_model_se_autodetecta_por_las_poses() {
        // Sin `input_model =`: poses izq./der. → hands.
        let t = parse_sheet_ini(
            "frame_w=8\nframe_h=8\nsheet=s.png\n\
             state_idle_row=1\nstate_idle_frames=1\n\
             state_active_left_row=2\nstate_active_left_frames=1\n\
             state_active_right_row=3\nstate_active_right_frames=1\n",
        );
        assert_eq!(t.input_model, InputModel::Hands);

        // Sin poses → activity (el default de los packs de vpets).
        let t = parse_sheet_ini(
            "frame_w=8\nframe_h=8\nsheet=s.png\n\
             state_idle_row=1\nstate_idle_frames=1\n\
             state_writing_row=2\nstate_writing_frames=2\n",
        );
        assert_eq!(t.input_model, InputModel::Activity);

        // `input_model =` explícito manda aunque haya poses.
        let t = parse_sheet_ini(
            "frame_w=8\nframe_h=8\nsheet=s.png\ninput_model=activity\n\
             state_left_down_row=1\nstate_left_down_frames=1\n\
             state_right_down_row=2\nstate_right_down_frames=1\n",
        );
        assert_eq!(t.input_model, InputModel::Activity, "explícito gana");
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

    #[test]
    fn scale_bilinear_interpola_y_conserva_esquinas() {
        // 2×1, dos colores planos opacos; k=4 escala **ambas** dimensiones → 8×4.
        let src = [0u8, 0, 0, 255, 255, 255, 255, 255];
        let (out, w, h) = scale_bilinear(&src, 2, 1, 4);
        assert_eq!((w, h), (8, 4));
        // Fila 0 (bytes 0..32): esquinas clavadas a los extremos y rampa en medio.
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        assert_eq!(&out[28..32], &[255, 255, 255, 255]);
        let r: Vec<u8> = out[0..32].chunks_exact(4).map(|p| p[0]).collect();
        assert!(r.windows(2).all(|w| w[0] <= w[1]), "rampa monótona: {r:?}");
        assert!(r[3] > 0 && r[4] < 255, "los centrales son intermedios");
        // Todas las filas son iguales (el origen tenía una sola).
        assert_eq!(&out[0..32], &out[32..64]);
        // k=1 es identidad.
        assert_eq!(scale_bilinear(&src, 2, 1, 1).0, src);
    }

    #[test]
    fn premul_bgra_swap_y_multiplica() {
        // Opaco: solo swap R↔B, sin pérdida.
        let mut a = [10u8, 20, 30, 255];
        premul_bgra_from_straight_rgba(&mut a);
        assert_eq!(a, [30, 20, 10, 255]);
        // Alfa 0: píxel a cero salvo el propio alfa.
        let mut z = [10u8, 20, 30, 0];
        premul_bgra_from_straight_rgba(&mut z);
        assert_eq!(z, [0, 0, 0, 0]);
        // Alfa 128 (~50 %): cada canal ≈ mitad, y en orden BGRA.
        let mut h = [200u8, 100, 40, 128];
        premul_bgra_from_straight_rgba(&mut h);
        assert_eq!(h, [20, 50, 100, 128]);
    }

    #[test]
    fn pick_state_cae_al_siguiente() {
        let t = parse_sheet_ini(INI); // define idle y writing, no sleep
        assert_eq!(
            pick_state(&t, &["sleep", "boring", "idle"]).unwrap().name,
            "idle"
        );
        assert_eq!(pick_state(&t, &["writing"]).unwrap().name, "writing");
        assert!(pick_state(&t, &["nope", "nada"]).is_none());
    }

    #[test]
    fn hojas_por_estado_tienen_prioridad_sobre_la_global() {
        let t = parse_sheet_ini(
            "frame_w=8\nframe_h=8\n\
             sheet = base.png\n\
             sheet_writing = escribe.png\n\
             sheet_sleep = duerme.png\n\
             state_idle_row=1\nstate_idle_frames=1\n\
             state_writing_row=1\nstate_writing_frames=2\n\
             state_sleep_row=1\nstate_sleep_frames=1\n",
        );
        assert_eq!(t.sheet.as_deref(), Some("base.png"));
        assert_eq!(
            t.sheet_for("idle"),
            Some("base.png"),
            "sin override -> global"
        );
        assert_eq!(t.sheet_for("writing"), Some("escribe.png"));
        assert_eq!(t.sheet_for("sleep"), Some("duerme.png"));
        assert_eq!(
            t.sheet_files().into_iter().collect::<Vec<_>>(),
            ["base.png", "duerme.png", "escribe.png"]
        );
    }

    #[test]
    fn estado_con_solo_hoja_propia_existe_sin_row_ni_frames() {
        let t = parse_sheet_ini(
            "frame_w=8\nframe_h=8\ndefault_fps=9\n\
             sheet = base.png\n\
             sheet_writing = w.apng\n\
             state_idle_row=1\nstate_idle_frames=2\n",
        );
        let w = t
            .state("writing")
            .expect("writing existe por su sheet_writing");
        assert_eq!(
            (w.row, w.frames, w.fps),
            (0, 0, 9),
            "row/frames 0, fps default"
        );
        assert_eq!(t.sheet_for("writing"), Some("w.apng"));
        // idle sigue siendo un estado de rejilla normal.
        assert_eq!(t.state("idle").unwrap().frames, 2);

        // `state_<n>_fps` sí se respeta aunque no haya row/frames (APNG).
        let t2 = parse_sheet_ini(
            "frame_w=8\nframe_h=8\ndefault_fps=9\n\
             sheet_writing = w.apng\nstate_writing_fps = 24\n",
        );
        assert_eq!(t2.state("writing").unwrap().fps, 24, "fps propio del APNG");
    }

    #[test]
    fn sin_hoja_global_solo_los_estados_con_la_suya() {
        let t = parse_sheet_ini(
            "frame_w=8\nframe_h=8\n\
             sheet_writing = w.png\n\
             state_idle_row=1\nstate_idle_frames=1\n\
             state_writing_row=1\nstate_writing_frames=1\n",
        );
        assert_eq!(t.sheet, None);
        assert_eq!(t.sheet_for("writing"), Some("w.png"));
        assert_eq!(t.sheet_for("idle"), None, "sin global ni propia -> None");
    }
}
