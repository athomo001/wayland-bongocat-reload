//! Rasterizado y caché de los 5 fotogramas del gato (rebanada 3 de la Fase 0.5).
//!
//! Porta `animation_cache_frames` / `blit_cached_frame` de
//! `src/graphics/animation.c`. La máquina de estados (qué fotograma toca según
//! el teclado) llega en la rebanada 4.

use std::error::Error;

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

/// Rasteriza los 5 SVG a una altura de gato de `cat_height` px (el ancho sale
/// de la relación de aspecto), aplicando el espejo horizontal/vertical.
pub fn rasterize(
    cat_height: u32,
    mirror_x: bool,
    mirror_y: bool,
) -> Result<Frames, Box<dyn Error>> {
    let h = cat_height.max(1);
    let w = ((u64::from(h) * REF_W) / REF_H).max(1) as u32;
    let opt = Options::default();

    let mut frames: [Vec<u8>; 5] = Default::default();
    for (i, raw) in SVGS.iter().enumerate() {
        let tree = Tree::from_data(preprocess(raw).as_bytes(), &opt)
            .map_err(|e| format!("SVG {i} no parsea: {e}"))?;
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
    Ok(Frames { w, h, frames })
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
/// "over". Recorta lo que se salga. Porta `blit_cached_frame`.
pub fn blit_over(
    dst: &mut [u8],
    dst_wh: (u32, u32),
    src: &[u8],
    src_wh: (u32, u32),
    origin: (i32, i32),
) {
    let (dst_w, dst_h) = dst_wh;
    let (src_w, src_h) = src_wh;
    let (ox, oy) = origin;
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
            let sa = src[si + 3];
            if sa == 0 {
                continue;
            }
            if sa == 255 {
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            } else {
                let inv = 255 - u16::from(sa);
                for c in 0..4 {
                    dst[di + c] =
                        (u16::from(src[si + c]) + (u16::from(dst[di + c]) * inv) / 255) as u8;
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

    #[test]
    fn blit_over_sobre_fondo_vacio_copia_el_gato() {
        let mut dst = vec![0u8; 4 * 4 * 4]; // 4x4
        let src = [0x10u8, 0x20, 0x30, 0xFF].repeat(4); // 2x2 opaco
        blit_over(&mut dst, (4, 4), &src, (2, 2), (1, 1));
        // píxel (fila 1, col 1) del destino = píxel opaco del src
        let (row, col) = (1usize, 1usize);
        let i = (row * 4 + col) * 4;
        assert_eq!(&dst[i..i + 4], &[0x10, 0x20, 0x30, 0xFF]);
        // esquina (0,0) sigue vacía
        assert_eq!(&dst[0..4], &[0, 0, 0, 0]);
    }
}
