//! Decodificación PNG / APNG **acotada** para temas de sprite sheet
//! (`theme_format = 3`, spec 0014 M1 + M3).
//!
//! Se usa la crate `png` directamente: ya entra en el árbol de dependencias a
//! través de `tiny-skia`/`resvg`, así que no añade nada al binario ni a la
//! cadena de suministro (spec 0013). Cubre PNG normal y **APNG** (animación →
//! varios fotogramas ya compuestos). GIF queda para un hito posterior (necesita
//! la crate `gif`).
//!
//! Límites duros **antes** de reservar memoria (defensa ante PNG "bomba"):
//! dimensión máxima por lado, número máximo de píxeles y número máximo de
//! fotogramas. El llamante añade un tope al tamaño del fichero en disco.

/// Lado máximo admitido de una hoja (px). Cubre de sobra rejillas grandes de
/// wayland-vpets sin permitir cabeceras absurdas.
pub const MAX_DIM: u32 = 8192;
/// Píxeles totales máximos (~16 Mpx ⇒ 64 MiB en RGBA8).
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024;
/// Fotogramas máximos de un APNG (wayland-vpets tope 500 por estado; con margen).
pub const MAX_FRAMES: u32 = 1000;

/// Una imagen (o un fotograma) ya decodificada a **RGBA8 recto** (sin
/// premultiplicar), fila a fila sin relleno entre filas.
pub struct DecodedPng {
    pub w: u32,
    pub h: u32,
    /// `w * h * 4` bytes, orden R,G,B,A.
    pub rgba: Vec<u8>,
}

/// Decodifica un PNG a uno o varios fotogramas RGBA8 rectos:
/// - PNG normal → 1 fotograma.
/// - **APNG** → todos los fotogramas de la animación, ya **compuestos** sobre el
///   lienzo `W×H` según `dispose_op` / `blend_op` (cada elemento es un fotograma
///   completo listo para pintar).
pub fn decode_frames(bytes: &[u8]) -> Result<Vec<DecodedPng>, String> {
    use png::{BlendOp, DisposeOp};

    let mut reader = open(bytes)?;
    let (w, h) = dims(&reader)?;

    // Nº de fotogramas: 1 si no es APNG; si lo es, `num_frames` (+1 si la imagen
    // por defecto no forma parte de la animación — misma regla que usa `png`).
    let total = match reader.info().animation_control {
        None => 1,
        Some(ac) => {
            let mut n = ac.num_frames;
            if reader.info().frame_control.is_none() {
                n = n.saturating_add(1);
            }
            n
        }
    };
    if total > MAX_FRAMES {
        return Err(format!("APNG: {total} fotogramas, máximo {MAX_FRAMES}"));
    }

    let canvas_len = w as usize * h as usize * 4;
    let mut canvas = vec![0u8; canvas_len];
    let mut frames: Vec<DecodedPng> = Vec::with_capacity(total as usize);
    let mut sub = vec![0u8; reader.output_buffer_size()];

    for _ in 0..total {
        let out = reader
            .next_frame(&mut sub)
            .map_err(|e| format!("APNG: fallo al decodificar un fotograma: {e}"))?;
        let region = normalize_rgba8(&sub[..out.buffer_size()], &out)?;
        let (sw, sh) = (out.width, out.height);

        let (xoff, yoff, dispose, blend) = match reader.info().frame_control {
            Some(fc) => (fc.x_offset, fc.y_offset, fc.dispose_op, fc.blend_op),
            None => (0, 0, DisposeOp::None, BlendOp::Source), // imagen por defecto, marco completo
        };
        // Recorta la subregión declarada al lienzo (la spec APNG lo garantiza,
        // pero un fichero corrupto no tiene por qué).
        if xoff >= w || yoff >= h {
            return Err("APNG: subfotograma fuera del lienzo".to_string());
        }
        let rw = sw.min(w - xoff);
        let rh = sh.min(h - yoff);

        let saved = (dispose == DisposeOp::Previous).then(|| canvas.clone());
        composite(&mut canvas, w, &region, sw, (xoff, yoff), (rw, rh), blend);
        frames.push(DecodedPng {
            w,
            h,
            rgba: canvas.clone(),
        });
        match dispose {
            DisposeOp::None => {}
            DisposeOp::Background => clear_region(&mut canvas, w, (xoff, yoff), (rw, rh)),
            DisposeOp::Previous => {
                if let Some(s) = saved {
                    canvas = s;
                }
            }
        }
    }

    if frames.is_empty() {
        return Err("PNG sin fotogramas".to_string());
    }
    Ok(frames)
}

/// Abre el decodificador con las transformaciones estándar (paleta→RGB, grises
/// <8→8, tRNS→alfa, 16→8) y valida la cabecera.
fn open(bytes: &[u8]) -> Result<png::Reader<&[u8]>, String> {
    use png::Transformations;
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(
        Transformations::EXPAND | Transformations::ALPHA | Transformations::STRIP_16,
    );
    dec.read_info().map_err(|e| format!("PNG ilegible: {e}"))
}

/// Dimensiones del lienzo, ya validadas contra los topes.
fn dims(reader: &png::Reader<&[u8]>) -> Result<(u32, u32), String> {
    let info = reader.info();
    let (w, h) = (info.width, info.height);
    check_dims(w, h)?;
    Ok((w, h))
}

/// Normaliza el búfer de un (sub)fotograma a RGBA8 recto según su tipo de color.
fn normalize_rgba8(buf: &[u8], out: &png::OutputInfo) -> Result<Vec<u8>, String> {
    use png::{BitDepth, ColorType};
    if out.bit_depth != BitDepth::Eight {
        return Err(format!("PNG: profundidad {:?} no soportada", out.bit_depth));
    }
    Ok(match out.color_type {
        ColorType::Rgba => buf.to_vec(),
        ColorType::Rgb => expand(buf, 3, |p, o| o.copy_from_slice(&[p[0], p[1], p[2], 255])),
        ColorType::GrayscaleAlpha => expand(buf, 2, |p, o| {
            o.copy_from_slice(&[p[0], p[0], p[0], p[1]]);
        }),
        ColorType::Grayscale => expand(buf, 1, |p, o| {
            o.copy_from_slice(&[p[0], p[0], p[0], 255]);
        }),
        ColorType::Indexed => {
            return Err("PNG: paleta sin expandir (inesperado con EXPAND)".to_string());
        }
    })
}

/// Compone `region` (`sw` de ancho, RGBA8 recto) sobre `canvas` (`cw` de ancho)
/// en `(xoff, yoff)`, `rw`×`rh` píxeles, según `blend`.
fn composite(
    canvas: &mut [u8],
    cw: u32,
    region: &[u8],
    sw: u32,
    (xoff, yoff): (u32, u32),
    (rw, rh): (u32, u32),
    blend: png::BlendOp,
) {
    for y in 0..rh {
        for x in 0..rw {
            let si = (((y * sw) + x) * 4) as usize;
            let di = ((((y + yoff) * cw) + (x + xoff)) * 4) as usize;
            let s = &region[si..si + 4];
            match blend {
                png::BlendOp::Source => canvas[di..di + 4].copy_from_slice(s),
                png::BlendOp::Over => {
                    let sa = u16::from(s[3]);
                    let inv = 255 - sa;
                    for c in 0..4 {
                        let d = u16::from(canvas[di + c]);
                        canvas[di + c] = (u16::from(s[c]) + (d * inv) / 255).min(255) as u8;
                    }
                }
            }
        }
    }
}

/// Pone a cero un rectángulo del lienzo (`DisposeOp::Background`).
fn clear_region(canvas: &mut [u8], cw: u32, (xoff, yoff): (u32, u32), (rw, rh): (u32, u32)) {
    for y in 0..rh {
        let di = ((((y + yoff) * cw) + xoff) * 4) as usize;
        canvas[di..di + (rw as usize) * 4].fill(0);
    }
}

/// Rechaza dimensiones nulas o por encima de los topes, sin reservar memoria.
pub fn check_dims(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("PNG de dimensión nula".to_string());
    }
    if w > MAX_DIM || h > MAX_DIM {
        return Err(format!(
            "PNG {w}x{h}: excede el máximo de {MAX_DIM} px por lado"
        ));
    }
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!(
            "PNG {w}x{h}: excede el máximo de {MAX_PIXELS} píxeles"
        ));
    }
    Ok(())
}

/// Reescribe `src` (grupos de `src_bpp` bytes) a RGBA8 aplicando `f` a cada
/// píxel (`f(entrada, salida_de_4_bytes)`).
fn expand(src: &[u8], src_bpp: usize, f: impl Fn(&[u8], &mut [u8])) -> Vec<u8> {
    let n = src.len() / src_bpp;
    let mut out = vec![0u8; n * 4];
    for (p, o) in src.chunks_exact(src_bpp).zip(out.chunks_exact_mut(4)) {
        f(p, o);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Codifica un PNG RGBA8 mínimo en memoria para las pruebas.
    fn encode(w: u32, h: u32, rgba: &[u8], color: png::ColorType) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            writer.write_image_data(rgba).unwrap();
        }
        out
    }

    /// Codifica un APNG RGBA8 de marcos completos (sin subregiones).
    fn encode_apng(w: u32, h: u32, frames: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_animated(frames.len() as u32, 0).unwrap();
            let mut writer = enc.write_header().unwrap();
            for f in frames {
                writer.write_image_data(f).unwrap();
            }
        }
        out
    }

    #[test]
    fn rgba_ida_y_vuelta() {
        let src: Vec<u8> = (0..(2 * 2 * 4) as u8).collect();
        let png = encode(2, 2, &src, png::ColorType::Rgba);
        let fs = decode_frames(&png).unwrap();
        assert_eq!(fs.len(), 1);
        assert_eq!((fs[0].w, fs[0].h), (2, 2));
        assert_eq!(fs[0].rgba, src);
    }

    #[test]
    fn rgb_gana_canal_alfa_opaco() {
        let src = [10u8, 20, 30, 40, 50, 60]; // 2x1 RGB
        let png = encode(2, 1, &src, png::ColorType::Rgb);
        let fs = decode_frames(&png).unwrap();
        assert_eq!(fs[0].rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn dimensiones_absurdas_se_rechazan() {
        assert!(check_dims(0, 10).is_err());
        assert!(check_dims(MAX_DIM + 1, 10).is_err());
        assert!(check_dims(100_000, 100_000).unwrap_err().contains("excede"));
        assert!(check_dims(64, 512).is_ok());
    }

    #[test]
    fn basura_no_es_png() {
        assert!(decode_frames(b"no soy un png").is_err());
    }

    #[test]
    fn apng_devuelve_todos_los_fotogramas_compuestos() {
        // 3 marcos completos, cada uno de un color plano opaco (blend Source por
        // defecto): el compuesto es idéntico a cada marco.
        let rojo = [200u8, 0, 0, 255].repeat(4);
        let verde = [0u8, 200, 0, 255].repeat(4);
        let azul = [0u8, 0, 200, 255].repeat(4);
        let apng = encode_apng(2, 2, &[&rojo, &verde, &azul]);
        let fs = decode_frames(&apng).unwrap();
        assert_eq!(fs.len(), 3);
        assert_eq!(fs[0].rgba, rojo);
        assert_eq!(fs[1].rgba, verde);
        assert_eq!(fs[2].rgba, azul);
    }
}
