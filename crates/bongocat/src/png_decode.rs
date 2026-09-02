//! Decodificación PNG **acotada** para temas de sprite sheet
//! (`theme_format = 3`, spec 0014 M1).
//!
//! Se usa la crate `png` directamente: ya entra en el árbol de dependencias a
//! través de `tiny-skia`/`resvg`, así que no añade nada al binario ni a la
//! cadena de suministro (spec 0013). La spec 0014 hablaba de la crate `image`
//! con features `png apng gif`; APNG/GIF se abordan en el hito M3, donde `image`
//! (o `gif` a secas) sí aporta algo — para M1 es innecesaria.
//!
//! Límites duros **antes** de reservar memoria (defensa ante PNG "bomba"):
//! dimensión máxima por lado y número máximo de píxeles. El llamante añade,
//! además, un tope al tamaño del fichero en disco.

/// Lado máximo admitido de una hoja (px). Cubre de sobra rejillas grandes de
/// wayland-vpets sin permitir cabeceras absurdas.
pub const MAX_DIM: u32 = 8192;
/// Píxeles totales máximos (~16 Mpx ⇒ 64 MiB en RGBA8).
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024;

/// Una hoja ya decodificada a **RGBA8 recto** (sin premultiplicar), fila a fila
/// sin relleno entre filas.
pub struct DecodedPng {
    pub w: u32,
    pub h: u32,
    /// `w * h * 4` bytes, orden R,G,B,A.
    pub rgba: Vec<u8>,
}

/// Decodifica `bytes` (un PNG) a RGBA8 recto. Expande paleta/escala de grises y
/// reduce 16→8 bits. Devuelve `Err` con un motivo legible si el PNG está
/// corrupto o excede los límites.
pub fn decode_rgba8(bytes: &[u8]) -> Result<DecodedPng, String> {
    use png::{BitDepth, ColorType, Transformations};

    let mut dec = png::Decoder::new(bytes);
    // EXPAND: paleta→RGB, grises <8 bits→8, tRNS→alfa. ALPHA: garantiza canal
    // alfa en paleta. STRIP_16: 16→8 bits.
    dec.set_transformations(
        Transformations::EXPAND | Transformations::ALPHA | Transformations::STRIP_16,
    );

    let mut reader = dec.read_info().map_err(|e| format!("PNG ilegible: {e}"))?;

    // Cota de tamaño con las dimensiones declaradas en la cabecera, antes de
    // que `next_frame` reserve el búfer de salida.
    let (w, h) = {
        let info = reader.info();
        (info.width, info.height)
    };
    check_dims(w, h)?;

    let mut buf = vec![0u8; reader.output_buffer_size()];
    let out = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG: fallo al decodificar: {e}"))?;
    buf.truncate(out.buffer_size());

    if out.bit_depth != BitDepth::Eight {
        return Err(format!("PNG: profundidad {:?} no soportada", out.bit_depth));
    }

    // Normaliza cualquier tipo de color a RGBA8 recto.
    let rgba = match out.color_type {
        ColorType::Rgba => buf,
        ColorType::Rgb => expand(&buf, 3, |p, o| {
            o.copy_from_slice(&[p[0], p[1], p[2], 255]);
        }),
        ColorType::GrayscaleAlpha => expand(&buf, 2, |p, o| {
            o.copy_from_slice(&[p[0], p[0], p[0], p[1]]);
        }),
        ColorType::Grayscale => expand(&buf, 1, |p, o| {
            o.copy_from_slice(&[p[0], p[0], p[0], 255]);
        }),
        ColorType::Indexed => {
            return Err("PNG: paleta sin expandir (inesperado con EXPAND)".to_string());
        }
    };

    let expected = w as usize * h as usize * 4;
    if rgba.len() != expected {
        return Err(format!(
            "PNG: {} bytes decodificados, se esperaban {expected}",
            rgba.len()
        ));
    }
    Ok(DecodedPng { w, h, rgba })
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

    #[test]
    fn rgba_ida_y_vuelta() {
        let src: Vec<u8> = (0..(2 * 2 * 4) as u8).collect();
        let png = encode(2, 2, &src, png::ColorType::Rgba);
        let d = decode_rgba8(&png).unwrap();
        assert_eq!((d.w, d.h), (2, 2));
        assert_eq!(d.rgba, src);
    }

    #[test]
    fn rgb_gana_canal_alfa_opaco() {
        let src = [10u8, 20, 30, 40, 50, 60]; // 2x1 RGB
        let png = encode(2, 1, &src, png::ColorType::Rgb);
        let d = decode_rgba8(&png).unwrap();
        assert_eq!(d.rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
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
        assert!(decode_rgba8(b"no soy un png").is_err());
    }
}
