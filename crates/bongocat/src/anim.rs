//! Rasterizado y caché de los 5 fotogramas del gato (rebanada 3 de la Fase 0.5).
//!
//! Porta `animation_cache_frames` / `blit_cached_frame` de
//! `src/graphics/animation.c`. La máquina de estados (qué fotograma toca según
//! el teclado) llega en la rebanada 4.

use std::collections::BTreeMap;
use std::error::Error;

use bongocat_common::sheet::{self, SheetTheme};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

/// Relación de aspecto de referencia del gato (`CAT_IMAGE_WIDTH/HEIGHT`).
const REF_W: u64 = 500;
const REF_H: u64 = 277;

/// SVG embebidos, en el orden de `bongocat_common::paw::FRAME_*`:
/// 0 both-up · 1 left-down · 2 right-down · 3 both-down · 4 sleeping.
const SVGS: [&[u8]; 5] = [
    include_bytes!("../../../assets/new/bongo-both-up.svg"),
    include_bytes!("../../../assets/new/bongo-left-down.svg"),
    include_bytes!("../../../assets/new/bongo-right-down.svg"),
    include_bytes!("../../../assets/new/bongo-both-down.svg"),
    include_bytes!("../../../assets/new/bongo-sleeping.svg"),
];

/// Los 5 fotogramas ya rasterizados a `w`x`h`, en BGRA premultiplicado
/// (formato nativo de `WL_SHM_FORMAT_ARGB8888`).
pub struct Frames {
    pub w: u32,
    pub h: u32,
    /// Relación de aspecto del tema activo (`(w, h)`); el blit y el hit-test del
    /// modo edición la usan como única fuente.
    pub aspect: (u32, u32),
    frames: [Vec<u8>; 5],
}

impl Frames {
    /// Bytes BGRA del fotograma `i` (0–4).
    #[must_use]
    pub fn frame(&self, i: usize) -> &[u8] {
        &self.frames[i.min(4)]
    }
}

/// Recorta el viewBox al área visible del gato y quita el `<rect>` de fondo del
/// editor — el mismo preprocesado que hace `scripts/embed_assets.sh` con `sed`.
/// Temporal hasta mover los SVG ya recortados a `themes/classic/` (spec 0006).
fn preprocess(svg: &[u8]) -> String {
    let s = std::str::from_utf8(svg).unwrap_or_default();
    s.replace(r#"viewBox="0 0 500 500""#, r#"viewBox="0 101 500 277""#)
        .lines()
        .filter(|l| !l.contains("<rect "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Los 5 SVG del `classic` ya preprocesados (viewBox recortado, sin `<rect>`).
/// Los usa `bongocat theme new` como plantilla de partida.
#[must_use]
pub fn classic_frame_svgs() -> [String; 5] {
    [
        preprocess(SVGS[0]),
        preprocess(SVGS[1]),
        preprocess(SVGS[2]),
        preprocess(SVGS[3]),
        preprocess(SVGS[4]),
    ]
}

/// Rasteriza el **gato embebido** (`classic`) a `cat_height` px. Fallback siempre
/// disponible (spec 0006 §4).
pub fn rasterize(
    cat_height: u32,
    mirror_x: bool,
    mirror_y: bool,
) -> Result<Frames, Box<dyn Error>> {
    rasterize_from(
        &SVGS,
        (REF_W as u32, REF_H as u32),
        true, // los SVG embebidos llevan el margen del editor: recortar
        cat_height,
        mirror_x,
        mirror_y,
    )
}

/// Rasteriza 5 SVG (de un tema o del embebido) a una altura de gato de
/// `cat_height` px; el ancho sale de `aspect`. `crop` aplica el recorte del
/// viewBox y el borrado de `<rect>` (solo para el embebido; un tema ya da el SVG
/// "recortado", spec 0006). Aplica el espejo H/V.
pub fn rasterize_from<S: AsRef<[u8]>>(
    sources: &[S; 5],
    aspect: (u32, u32),
    crop: bool,
    cat_height: u32,
    mirror_x: bool,
    mirror_y: bool,
) -> Result<Frames, Box<dyn Error>> {
    let (aw, ah) = (aspect.0.max(1), aspect.1.max(1));
    let h = cat_height.max(1);
    let w = ((u64::from(h) * u64::from(aw)) / u64::from(ah)).max(1) as u32;
    let opt = Options::default();

    let mut frames: [Vec<u8>; 5] = Default::default();
    for (i, raw) in sources.iter().enumerate() {
        let bytes = raw.as_ref();
        let data = if crop {
            preprocess(bytes).into_bytes()
        } else {
            bytes.to_vec()
        };
        let tree = Tree::from_data(&data, &opt).map_err(|e| format!("SVG {i} no parsea: {e}"))?;
        let mut pm = Pixmap::new(w, h).ok_or("no se pudo crear el pixmap")?;
        let size = tree.size();
        let sx = w as f32 / size.width();
        let sy = h as f32 / size.height();
        resvg::render(&tree, Transform::from_scale(sx, sy), &mut pm.as_mut());

        // tiny-skia entrega RGBA premultiplicado; Wayland quiere BGRA. Swap R↔B.
        let mut buf = pm.data().to_vec();
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        if mirror_x {
            flip_h(&mut buf, w, h);
        }
        if mirror_y {
            flip_v(&mut buf, w, h);
        }
        frames[i] = buf;
    }
    Ok(Frames {
        w,
        h,
        aspect: (aw, ah),
        frames,
    })
}

/// Caché de un tema de sprite sheet (`theme_format = 3`, spec 0014): por cada
/// estado, sus frames ya recortados de la hoja, escalados a escala **entera**
/// nearest-neighbor (pixel-art nítido), convertidos a **BGRA premultiplicado** y
/// con el espejo H/V aplicado. Clave = nombre del estado (`idle`, `writing`, …).
///
/// `png_rgba` es la hoja decodificada a RGBA8 recto (`png_w`×`png_h`).
/// `cat_height` es la altura objetivo en píxeles; el factor entero real puede
/// quedar por debajo si `frame_h` no la divide (el `classic` SVG no tiene esta
/// limitación; es el precio del pixel-art).
#[must_use]
pub fn build_sheet_cache(
    sheet: &SheetTheme,
    png_rgba: &[u8],
    png_w: u32,
    png_h: u32,
    cat_height: u32,
    mirror_x: bool,
    mirror_y: bool,
) -> BTreeMap<String, Vec<Vec<u8>>> {
    let (fw, fh) = (sheet.frame_w.max(1), sheet.frame_h.max(1));
    let k = sheet::integer_scale(fh, cat_height.max(1));
    let (w, h) = (fw * k, fh * k);

    let mut cache: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    for st in &sheet.states {
        let mut frames_st = Vec::with_capacity(st.frames as usize);
        for i in 0..st.frames {
            let rect = sheet::frame_rect(sheet, st, i);
            let cropped = sheet::crop_frame(png_rgba, png_w, png_h, rect);
            let mut px = if k > 1 {
                sheet::scale_nearest(&cropped, fw, fh, k).0
            } else {
                cropped
            };
            sheet::premul_bgra_from_straight_rgba(&mut px);
            if mirror_x {
                flip_h(&mut px, w, h);
            }
            if mirror_y {
                flip_v(&mut px, w, h);
            }
            frames_st.push(px);
        }
        if !frames_st.is_empty() {
            cache.insert(st.name.clone(), frames_st);
        }
    }
    cache
}

/// Rasteriza un tema de sprite sheet a la estructura `Frames` de 5 huecos que
/// hoy consume el render (spec 0014 M1).
///
/// Los 5 huecos "clásicos" de `paw.rs` se mapean a estados de la hoja con la
/// "regla de oro" de la spec §5.2: si el estado pedido falta, se cae al
/// siguiente candidato; si no hay ninguno, al hueco 0; si la hoja no produjo
/// **ningún** frame, es error (el llamante cae al gato embebido).
///
/// **M1 solo usa el frame 0 de cada estado.** La animación multi-frame dentro
/// de un estado, los estados de bucle/one-shot y las transiciones
/// reposo↔writing↔sleep son el hito M2, que sustituirá este colapso a 5 huecos
/// por una máquina de estados sobre [`build_sheet_cache`].
pub fn rasterize_sheet(
    sheet: &SheetTheme,
    png_rgba: &[u8],
    png_w: u32,
    png_h: u32,
    cat_height: u32,
    mirror_x: bool,
    mirror_y: bool,
) -> Result<Frames, Box<dyn Error>> {
    let (fw, fh) = (sheet.frame_w.max(1), sheet.frame_h.max(1));
    let k = sheet::integer_scale(fh, cat_height.max(1));
    let (w, h) = (fw * k, fh * k);

    let cache = build_sheet_cache(
        sheet, png_rgba, png_w, png_h, cat_height, mirror_x, mirror_y,
    );
    if cache.is_empty() {
        return Err("el sprite sheet no produjo ningún frame".into());
    }

    // Qué estados sirve cada hueco, en orden de preferencia.
    const SLOT_WANTS: [&[&str]; 5] = [
        &["idle", "boring", "writing"],         // FRAME_BOTH_UP
        &["writing", "start_writing", "idle"],  // FRAME_LEFT_DOWN
        &["writing", "start_writing", "idle"],  // FRAME_RIGHT_DOWN
        &["writing", "start_writing", "idle"],  // FRAME_BOTH_DOWN
        &["sleep", "asleep", "boring", "idle"], // FRAME_SLEEPING
    ];
    let pick0 = |wants: &[&str]| -> Option<Vec<u8>> {
        wants
            .iter()
            .find_map(|n| cache.get(*n))
            .and_then(|v| v.first())
            .cloned()
    };
    // Último recurso para el hueco 0: el primer frame de cualquier estado.
    let slot0 = pick0(SLOT_WANTS[0]).unwrap_or_else(|| {
        cache
            .values()
            .next()
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_else(|| vec![0u8; (w * h * 4) as usize])
    });

    let mut frames: [Vec<u8>; 5] = Default::default();
    frames[0] = slot0.clone();
    for (s, wants) in SLOT_WANTS.iter().enumerate().skip(1) {
        frames[s] = pick0(wants).unwrap_or_else(|| slot0.clone());
    }

    Ok(Frames {
        w,
        h,
        aspect: (fw, fh),
        frames,
    })
}

/// Voltea `buf` (RGBA/BGRA, 4 bytes/px) en horizontal, in situ.
fn flip_h(buf: &mut [u8], w: u32, h: u32) {
    let w = w as usize;
    for y in 0..h as usize {
        let row = y * w * 4;
        for x in 0..w / 2 {
            let (l, r) = (row + x * 4, row + (w - 1 - x) * 4);
            for k in 0..4 {
                buf.swap(l + k, r + k);
            }
        }
    }
}

/// Voltea `buf` en vertical, in situ.
fn flip_v(buf: &mut [u8], w: u32, h: u32) {
    let stride = w as usize * 4;
    let h = h as usize;
    let mut tmp = vec![0u8; stride];
    for y in 0..h / 2 {
        let (top, bot) = (y * stride, (h - 1 - y) * stride);
        tmp.copy_from_slice(&buf[top..top + stride]);
        buf.copy_within(bot..bot + stride, top);
        buf[bot..bot + stride].copy_from_slice(&tmp);
    }
}

/// Compone `src` (BGRA premultiplicado, tamaño `src_wh`) sobre `dst`
/// (BGRA premultiplicado, tamaño `dst_wh`) en el `origin` dado, con compositing
/// "over". `opacity` (0–255, 255 = sin cambio) escala uniformemente los 4
/// canales de cada píxel del gato — como es premultiplicado, eso baja también su
/// alfa efectivo (`cat_opacity`). Recorta lo que se salga. Porta
/// `blit_cached_frame`.
pub fn blit_over(
    dst: &mut [u8],
    dst_wh: (u32, u32),
    src: &[u8],
    src_wh: (u32, u32),
    origin: (i32, i32),
    opacity: u8,
) {
    let (dst_w, dst_h) = dst_wh;
    let (src_w, src_h) = src_wh;
    let (ox, oy) = origin;
    let op = u16::from(opacity);
    for sy in 0..src_h as i32 {
        let dy = sy + oy;
        if dy < 0 || dy >= dst_h as i32 {
            continue;
        }
        for sx in 0..src_w as i32 {
            let dx = sx + ox;
            if dx < 0 || dx >= dst_w as i32 {
                continue;
            }
            let si = ((sy as u32 * src_w + sx as u32) * 4) as usize;
            let di = ((dy as u32 * dst_w + dx as u32) * 4) as usize;
            // Píxel del gato ya escalado por la opacidad (exacto a 255).
            let s: [u8; 4] = std::array::from_fn(|c| (u16::from(src[si + c]) * op / 255) as u8);
            let sa = s[3];
            if sa == 0 {
                continue;
            }
            if sa == 255 {
                dst[di..di + 4].copy_from_slice(&s);
            } else {
                let inv = 255 - u16::from(sa);
                for c in 0..4 {
                    dst[di + c] = (u16::from(s[c]) + (u16::from(dst[di + c]) * inv) / 255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasteriza_los_cinco_a_la_altura_pedida() {
        let f = rasterize(110, false, false).expect("rasterizado");
        assert_eq!(f.h, 110);
        assert_eq!(f.w, (110 * 500) / 277);
        for i in 0..5 {
            assert_eq!(f.frame(i).len(), (f.w * f.h * 4) as usize, "frame {i}");
        }
    }

    #[test]
    fn algun_pixel_del_gato_es_opaco() {
        // El gato (relleno blanco) debe dejar píxeles con alfa alto.
        let f = rasterize(80, true, false).unwrap();
        let opaco = f.frame(0).chunks_exact(4).any(|p| p[3] > 200);
        assert!(opaco, "el fotograma 'both-up' salió transparente");
    }

    /// Firma compacta y estable de un rasterizado: por fotograma, la suma de
    /// todos los bytes y cuántos píxeles quedan no transparentes.
    fn firma(f: &Frames) -> Vec<(u64, usize)> {
        (0..5)
            .map(|i| {
                let px = f.frame(i);
                let sum = px.iter().map(|&b| u64::from(b)).sum();
                let opacos = px.chunks_exact(4).filter(|p| p[3] != 0).count();
                (sum, opacos)
            })
            .collect()
    }

    #[test]
    #[ignore = "solo para (re)fijar el snapshot: cargo test -- --ignored imprime_firma"]
    fn imprime_firma() {
        let f = rasterize(40, false, false).unwrap();
        eprintln!("dims = ({}, {})", f.w, f.h);
        eprintln!("firma = {:?}", firma(&f));
    }

    #[test]
    fn snapshot_rasterizado_classic() {
        // T-0010-M4: fija el resultado de rasterizar los 5 SVG a la altura por
        // defecto (`cat_height=40`). Si `resvg`/`usvg` cambian el render, este
        // test falla: hay que mirar el gato a ojo y re-fijar los números con
        // `cargo test -- --ignored imprime_firma`.
        let f = rasterize(40, false, false).expect("rasterizado");
        assert_eq!((f.w, f.h), (72, 40));
        assert_eq!(
            firma(&f),
            SNAPSHOT_CLASSIC,
            "cambió el rasterizado; revísalo visualmente antes de re-fijar"
        );
    }

    /// Firma por fotograma `(suma_de_bytes, píxeles_no_transparentes)` de
    /// `rasterize(40, false, false)`. Ver [`snapshot_rasterizado_classic`].
    const SNAPSHOT_CLASSIC: [(u64, usize); 5] = [
        (647_986, 769),
        (650_081, 787),
        (683_898, 820),
        (688_100, 839),
        (682_807, 801),
    ];

    #[test]
    fn blit_over_sobre_fondo_vacio_copia_el_gato() {
        let mut dst = vec![0u8; 4 * 4 * 4]; // 4x4
        let src = [0x10u8, 0x20, 0x30, 0xFF].repeat(4); // 2x2 opaco
        blit_over(&mut dst, (4, 4), &src, (2, 2), (1, 1), 255);
        // píxel (fila 1, col 1) del destino = píxel opaco del src
        let (row, col) = (1usize, 1usize);
        let i = (row * 4 + col) * 4;
        assert_eq!(&dst[i..i + 4], &[0x10, 0x20, 0x30, 0xFF]);
        // esquina (0,0) sigue vacía
        assert_eq!(&dst[0..4], &[0, 0, 0, 0]);
    }

    /// Hoja sintética 3×3 celdas de 4×4 px: cada celda `(fila, col)` pintada de
    /// un color RGBA recto único `[fila*3+col, 0, 50, 255]`.
    fn hoja_sintetica() -> (Vec<u8>, u32, u32) {
        let (cols, rows, fw, fh) = (3u32, 3u32, 4u32, 4u32);
        let (w, h) = (cols * fw, rows * fh);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for cy in 0..rows {
            for cx in 0..cols {
                let r = (cy * cols + cx) as u8;
                for y in 0..fh {
                    for x in 0..fw {
                        let px = (((cy * fh + y) * w) + (cx * fw + x)) * 4;
                        buf[px as usize..px as usize + 4].copy_from_slice(&[r, 0, 50, 255]);
                    }
                }
            }
        }
        (buf, w, h)
    }

    const SHEET_INI: &str = "\
theme_format = 3
frame_w = 4
frame_h = 4
state_idle_row = 1
state_idle_frames = 2
state_writing_row = 2
state_writing_frames = 3
state_sleep_row = 3
state_sleep_frames = 1
";

    #[test]
    fn cache_de_sprite_sheet_recorta_escala_y_premultiplica() {
        let sheet = bongocat_common::sheet::parse_sheet_ini(SHEET_INI);
        let (buf, w, h) = hoja_sintetica();
        // cat_height 8, frame_h 4 -> factor entero 2.
        let cache = build_sheet_cache(&sheet, &buf, w, h, 8, false, false);

        assert_eq!(cache.len(), 3, "idle, writing, sleep");
        assert_eq!(cache["idle"].len(), 2);
        assert_eq!(cache["writing"].len(), 3);
        assert_eq!(cache["sleep"].len(), 1);

        // Cada frame cacheado: 8×8 px BGRA.
        assert_eq!(cache["idle"][0].len(), 8 * 8 * 4);

        // idle frame 0 = celda (0,0), r=0. Recto [0,0,50,255] -> BGRA [50,0,0,255].
        assert_eq!(&cache["idle"][0][0..4], &[50, 0, 0, 255]);
        // writing frame 1 = celda (1,1), r=4 -> BGRA [50,0,4,255].
        assert_eq!(&cache["writing"][1][0..4], &[50, 0, 4, 255]);
        // El escalado 2× replica: el píxel (5,5) del frame 8×8 sigue en la celda.
        let i = ((5 * 8 + 5) * 4) as usize;
        assert_eq!(
            &cache["sleep"][0][i..i + 4],
            &[50, 0, 6, 255],
            "celda (2,0), r=6"
        );
    }

    #[test]
    fn rasterize_sheet_mapea_los_cinco_huecos_con_fallback() {
        let sheet = bongocat_common::sheet::parse_sheet_ini(SHEET_INI);
        let (buf, w, h) = hoja_sintetica();
        let f = rasterize_sheet(&sheet, &buf, w, h, 8, false, false).unwrap();

        assert_eq!((f.w, f.h), (8, 8));
        assert_eq!(f.aspect, (4, 4));
        // hueco 0 -> idle f0 (r=0); huecos 1..3 -> writing f0 (r=3); hueco 4 -> sleep f0 (r=6).
        assert_eq!(&f.frame(0)[0..4], &[50, 0, 0, 255]);
        assert_eq!(&f.frame(1)[0..4], &[50, 0, 3, 255]);
        assert_eq!(&f.frame(3)[0..4], &[50, 0, 3, 255]);
        assert_eq!(&f.frame(4)[0..4], &[50, 0, 6, 255]);

        // Sin estado 'sleep': el hueco 4 cae al hueco 0.
        let solo_idle = bongocat_common::sheet::parse_sheet_ini(
            "frame_w=4\nframe_h=4\nstate_idle_row=1\nstate_idle_frames=1\n",
        );
        let f2 = rasterize_sheet(&solo_idle, &buf, w, h, 4, false, false).unwrap();
        assert_eq!(f2.frame(4), f2.frame(0), "sin sleep -> fallback al hueco 0");
    }

    #[test]
    fn blit_over_con_opacidad_reduce_todos_los_canales() {
        // Gato opaco [40,80,120,255] al 50 % sobre fondo vacío → mitad de cada
        // canal y alfa ≈ 127 (compuesto "over" con inv≈128).
        let mut dst = vec![0u8; 4]; // 1x1
        let src = [40u8, 80, 120, 255];
        blit_over(&mut dst, (1, 1), &src, (1, 1), (0, 0), 128);
        // s = [20,40,60,128]; como sa != 255, va por la rama "over" sobre 0:
        // dst[c] = s[c] + 0 = s[c].
        assert_eq!(&dst[..], &[20, 40, 60, 128]);
        // opacidad 0 → nada.
        let mut d0 = vec![9u8; 4];
        blit_over(&mut d0, (1, 1), &src, (1, 1), (0, 0), 0);
        assert_eq!(&d0[..], &[9, 9, 9, 9], "opacidad 0 no dibuja");
    }
}
