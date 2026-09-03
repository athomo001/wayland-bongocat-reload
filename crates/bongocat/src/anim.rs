//! Rasterizado y caché de los 5 fotogramas del gato (rebanada 3 de la Fase 0.5).
//!
//! Porta `animation_cache_frames` / `blit_cached_frame` de
//! `src/graphics/animation.c`. La máquina de estados (qué fotograma toca según
//! el teclado) llega en la rebanada 4.

use std::collections::BTreeMap;
use std::error::Error;
use std::time::Instant;

use bongocat_common::sheet::{self, SheetTheme};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::png_decode::DecodedPng;
use crate::sheet_anim::SheetAnim;

/// Origen de píxeles de un fichero de hoja (`sheet =` / `sheet_<estado> =`).
pub enum SheetSource {
    /// Imagen única: los fotogramas del estado salen de la **rejilla**
    /// (`frame_rect`) según `state_<n>_row` / `_frames` / `_col`.
    Grid(DecodedPng),
    /// APNG (o GIF, en el futuro): los fotogramas ya vienen **separados** en el
    /// fichero; la rejilla se ignora para este estado.
    Frames(Vec<DecodedPng>),
}

/// Ficheros de hoja de un tema de sprite sheet ya decodificados, por **nombre de
/// fichero** (`sheet =` y cada `sheet_<estado> =`).
pub type SheetImages = BTreeMap<String, SheetSource>;

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

/// Fotogramas del gato ya rasterizados a `w`×`h` en BGRA premultiplicado
/// (formato nativo de `WL_SHM_FORMAT_ARGB8888`). El `classic` (5 SVG) y un tema
/// de sprite sheet (spec 0014) comparten `w`/`h`/`aspect`; se diferencian en
/// cómo se elige el fotograma visible (`kind`).
pub struct Frames {
    pub w: u32,
    pub h: u32,
    /// Relación de aspecto del tema activo (`(w, h)`); el blit y el hit-test del
    /// modo edición la usan como única fuente.
    pub aspect: (u32, u32),
    pub kind: FramesKind,
}

/// Cómo se decide el fotograma a pintar.
pub enum FramesKind {
    /// `classic` / temas SVG: 5 fotogramas fijos; el índice 0–4 lo elige la
    /// máquina "clásica" (`paw::frame_from_paw_state`).
    Classic(Box<[Vec<u8>; 5]>),
    /// Sprite sheet: caché por estado + cursor de la máquina de estados
    /// (`sheet_anim::SheetAnim`).
    Sheet(SheetAnim),
}

impl Frames {
    /// Bytes BGRA del fotograma `i` (0–4) de un tema `Classic`. Solo lo usan los
    /// tests del `classic`; el render usa [`Frames::current`].
    #[cfg(test)]
    #[must_use]
    pub fn frame(&self, i: usize) -> &[u8] {
        match &self.kind {
            FramesKind::Classic(f) => &f[i.min(4)],
            FramesKind::Sheet(_) => panic!("Frames::frame sobre un sprite sheet"),
        }
    }

    /// Bytes BGRA a pintar ahora mismo. Para `Classic`, el fotograma `classic_idx`
    /// (el que mantiene `State::frame`); para un sprite sheet, el que marque su
    /// máquina de estados.
    #[must_use]
    pub fn current(&self, classic_idx: u8) -> &[u8] {
        match &self.kind {
            FramesKind::Classic(f) => &f[classic_idx.min(4) as usize],
            FramesKind::Sheet(a) => a.current(),
        }
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
        kind: FramesKind::Classic(Box::new(frames)),
    })
}

/// Caché de un tema de sprite sheet (`theme_format = 3`, spec 0014): por cada
/// estado, sus frames ya recortados de **su** hoja (`sheet_<estado> =`, o la
/// global), escalados a escala **entera** nearest-neighbor (pixel-art nítido),
/// convertidos a **BGRA premultiplicado** y con el espejo H/V aplicado. Clave =
/// nombre del estado (`idle`, `writing`, …).
///
/// La fuente de cada estado puede ser una **rejilla** ([`SheetSource::Grid`],
/// se recortan `state_<n>_frames` celdas) o un **APNG** ([`SheetSource::Frames`],
/// cada fotograma del fichero es un frame; la rejilla se ignora). En ambos casos
/// el frame se ajusta a `frame_w`×`frame_h` (rellenando/recortando si el APNG no
/// coincide). Un estado sin fuente se omite. `cat_height` es la altura objetivo;
/// el factor entero real puede quedar por debajo si `frame_h` no la divide (el
/// `classic` SVG no tiene esta limitación; es el precio del pixel-art).
#[must_use]
pub fn build_sheet_cache(
    sheet: &SheetTheme,
    images: &SheetImages,
    cat_height: u32,
    mirror_x: bool,
    mirror_y: bool,
) -> BTreeMap<String, Vec<Vec<u8>>> {
    let (fw, fh) = (sheet.frame_w.max(1), sheet.frame_h.max(1));
    let k = sheet::integer_scale(fh, cat_height.max(1));
    let (w, h) = (fw * k, fh * k);

    // Ajusta un frame RGBA recto (`frame_w`×`frame_h`, ya recortado) a BGRA
    // premultiplicado, escalado y con el espejo aplicado. Se premultiplica
    // **antes** de escalar para que el filtro bilineal (`scale_filter = linear`)
    // no arrastre color de los píxeles transparentes.
    let linear = sheet.scale_filter == sheet::ScaleFilter::Linear;
    let finish = |mut px: Vec<u8>| -> Vec<u8> {
        sheet::premul_bgra_from_straight_rgba(&mut px);
        if k > 1 {
            px = if linear {
                sheet::scale_bilinear(&px, fw, fh, k).0
            } else {
                sheet::scale_nearest(&px, fw, fh, k).0
            };
        }
        if mirror_x {
            flip_h(&mut px, w, h);
        }
        if mirror_y {
            flip_v(&mut px, w, h);
        }
        px
    };

    let mut cache: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    for st in &sheet.states {
        let Some(src) = sheet.sheet_for(&st.name).and_then(|f| images.get(f)) else {
            continue; // sin hoja para este estado
        };
        let frames_st: Vec<Vec<u8>> = match src {
            SheetSource::Grid(png) => (0..st.frames)
                .map(|i| {
                    let rect = sheet::frame_rect(sheet, st, i);
                    finish(sheet::crop_frame(&png.rgba, png.w, png.h, rect))
                })
                .collect(),
            // Un APNG manda sus propios fotogramas; `state_<n>_frames` (concepto
            // de rejilla) se ignora para ese estado.
            SheetSource::Frames(fs) => fs
                .iter()
                .map(|f| finish(sheet::crop_frame(&f.rgba, f.w, f.h, (0, 0, fw, fh))))
                .collect(),
        };
        if !frames_st.is_empty() {
            cache.insert(st.name.clone(), frames_st);
        }
    }
    cache
}

/// Rasteriza un tema de sprite sheet (`theme_format = 3`, spec 0014) y monta su
/// máquina de estados. Cada estado que el tema **conduce** (spec §5.2) entra en
/// la caché con todos sus fotogramas; el cursor arranca en `idle` (con su cadena
/// de reserva). Error solo si no se produjo **ningún** fotograma (el llamante
/// cae al gato embebido).
pub fn rasterize_sheet(
    sheet: &SheetTheme,
    images: &SheetImages,
    cat_height: u32,
    mirror_x: bool,
    mirror_y: bool,
) -> Result<Frames, Box<dyn Error>> {
    let (fw, fh) = (sheet.frame_w.max(1), sheet.frame_h.max(1));
    let k = sheet::integer_scale(fh, cat_height.max(1));
    let (w, h) = (fw * k, fh * k);

    let cache = build_sheet_cache(sheet, images, cat_height, mirror_x, mirror_y);
    if cache.is_empty() {
        return Err("el sprite sheet no produjo ningún frame".into());
    }
    let anim = SheetAnim::from_cache(cache, sheet, Instant::now());
    if anim.is_empty() {
        // La caché tenía claves, pero ninguna era un estado que la v1 conduzca
        // (p. ej. solo `working`/`moving`): sin nada que animar.
        return Err("el sprite sheet no define ningún estado conducible".into());
    }

    Ok(Frames {
        w,
        h,
        aspect: (fw, fh),
        kind: FramesKind::Sheet(anim),
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
    fn golden_classic_de_disco_igual_al_embebido() {
        // T-0006-M7-golden: `themes/classic/` son los 5 SVG del embebido ya
        // recortados; rasterizarlos (sin `crop`) debe dar exactamente lo mismo
        // que el embebido (con `crop`). Si divergen, `themes/classic` quedó
        // desincronizado del arte embebido.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes/classic");
        let files = [
            "both-up.svg",
            "left-down.svg",
            "right-down.svg",
            "both-down.svg",
            "sleeping.svg",
        ];
        let svgs: [Vec<u8>; 5] = std::array::from_fn(|i| {
            std::fs::read(format!("{dir}/{}", files[i])).expect("SVG de themes/classic")
        });
        let disk = rasterize_from(&svgs, (500, 277), false, 40, false, false).unwrap();
        let embedded = rasterize(40, false, false).unwrap();
        assert_eq!((disk.w, disk.h), (embedded.w, embedded.h));
        for i in 0..5 {
            assert_eq!(disk.frame(i), embedded.frame(i), "fotograma {i} difiere");
        }
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
    fn hoja_sintetica() -> DecodedPng {
        let (cols, rows, fw, fh) = (3u32, 3u32, 4u32, 4u32);
        let (w, h) = (cols * fw, rows * fh);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for cy in 0..rows {
            for cx in 0..cols {
                let r = (cy * cols + cx) as u8;
                for y in 0..fh {
                    for x in 0..fw {
                        let px = (((cy * fh + y) * w) + (cx * fw + x)) * 4;
                        rgba[px as usize..px as usize + 4].copy_from_slice(&[r, 0, 50, 255]);
                    }
                }
            }
        }
        DecodedPng { w, h, rgba }
    }

    /// Un `SheetImages` con la hoja sintética bajo el nombre `hoja.png`.
    fn imgs() -> SheetImages {
        BTreeMap::from([("hoja.png".to_string(), SheetSource::Grid(hoja_sintetica()))])
    }

    const SHEET_INI: &str = "\
theme_format = 3
frame_w = 4
frame_h = 4
sheet = hoja.png
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
        // cat_height 8, frame_h 4 -> factor entero 2.
        let cache = build_sheet_cache(&sheet, &imgs(), 8, false, false);

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
    fn rasterize_sheet_monta_frames_y_arranca_en_idle() {
        let sheet = bongocat_common::sheet::parse_sheet_ini(SHEET_INI);
        let f = rasterize_sheet(&sheet, &imgs(), 8, false, false).unwrap();

        assert_eq!((f.w, f.h), (8, 8));
        assert_eq!(f.aspect, (4, 4));
        assert!(matches!(f.kind, FramesKind::Sheet(_)));
        // Arranca en idle, fotograma 0 = celda (0,0), r=0 -> BGRA [50,0,0,255].
        // `current` ignora el índice clásico para un sprite sheet.
        assert_eq!(&f.current(0)[0..4], &[50, 0, 0, 255]);
        assert_eq!(&f.current(3)[0..4], &[50, 0, 0, 255]);

        // Un tema sin ningún estado conducible (solo `working`) es error.
        let no_driv = bongocat_common::sheet::parse_sheet_ini(
            "frame_w=4\nframe_h=4\nsheet=hoja.png\nstate_working_row=1\nstate_working_frames=1\n",
        );
        assert!(rasterize_sheet(&no_driv, &imgs(), 4, false, false).is_err());
    }

    #[test]
    fn hoja_por_estado_gana_a_la_global() {
        // `writing` sale de su propia hoja (todo r=99); el resto, de la global.
        let ini = "\
frame_w = 4
frame_h = 4
sheet = base.png
sheet_writing = w.png
state_idle_row = 1
state_idle_frames = 1
state_writing_row = 1
state_writing_frames = 1
";
        let sheet = bongocat_common::sheet::parse_sheet_ini(ini);
        let solo_99 = DecodedPng {
            w: 4,
            h: 4,
            rgba: [99u8, 0, 50, 255].repeat(16),
        };
        let images = BTreeMap::from([
            ("base.png".to_string(), SheetSource::Grid(hoja_sintetica())),
            ("w.png".to_string(), SheetSource::Grid(solo_99)),
        ]);
        let cache = build_sheet_cache(&sheet, &images, 4, false, false);
        assert_eq!(
            &cache["idle"][0][0..4],
            &[50, 0, 0, 255],
            "idle <- base.png"
        );
        assert_eq!(
            &cache["writing"][0][0..4],
            &[50, 0, 99, 255],
            "writing <- w.png"
        );
    }

    #[test]
    fn estado_con_fuente_apng_toma_los_fotogramas_del_fichero() {
        // `writing` desde un APNG de 3 fotogramas; la rejilla (row/frames) se
        // ignora para ese estado.
        let ini = "\
frame_w = 4
frame_h = 4
sheet = base.png
sheet_writing = anim.apng
state_idle_row = 1
state_idle_frames = 1
state_writing_row = 1
state_writing_frames = 1
";
        let sheet = bongocat_common::sheet::parse_sheet_ini(ini);
        let mk = |r: u8| DecodedPng {
            w: 4,
            h: 4,
            rgba: [r, 0, 50, 255].repeat(16),
        };
        let images = BTreeMap::from([
            ("base.png".to_string(), SheetSource::Grid(hoja_sintetica())),
            (
                "anim.apng".to_string(),
                SheetSource::Frames(vec![mk(11), mk(22), mk(33)]),
            ),
        ]);
        let cache = build_sheet_cache(&sheet, &images, 4, false, false);
        assert_eq!(cache["writing"].len(), 3, "3 fotogramas del APNG");
        assert_eq!(&cache["writing"][0][0..4], &[50, 0, 11, 255]);
        assert_eq!(&cache["writing"][2][0..4], &[50, 0, 33, 255]);
        assert_eq!(cache["idle"].len(), 1, "idle sigue por rejilla");
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
