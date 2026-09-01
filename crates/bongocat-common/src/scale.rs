//! Aritmética de escalado HiDPI en base 120 (120 = 1.0×, 180 = 1.5×, 240 = 2.0×).
//!
//! Se trabaja con enteros para evitar acumular error de coma flotante entre
//! píxeles lógicos y físicos. Portado de `include/platform/scale.h`.

/// Convierte un tamaño lógico (px) a físico redondeando hacia arriba según la
/// escala (numerador sobre 120). Devuelve 0 si `logical <= 0` o `scale_120 == 0`.
#[must_use]
pub fn scale_size_120(logical: i32, scale_120: u32) -> i32 {
    if logical <= 0 || scale_120 == 0 {
        return 0;
    }
    // ceil(logical * scale_120 / 120), en i64 para no desbordar.
    let v = (i64::from(logical) * i64::from(scale_120) + 119) / 120;
    v as i32
}

/// Igual que [`scale_size_120`] pero conserva el signo: un offset negativo se
/// escala en magnitud y se vuelve a negar.
#[must_use]
pub fn scale_offset_120(logical: i32, scale_120: u32) -> i32 {
    if logical >= 0 {
        scale_size_120(logical, scale_120)
    } else {
        -scale_size_120(-logical, scale_120)
    }
}

/// Calcula el tamaño lógico de una salida.
///
/// - Si el compositor ya nos dio el tamaño lógico vía xdg-output
///   (`xdg_w`/`xdg_h` > 0), se usa tal cual.
/// - Si no, se parte del modo crudo: se intercambian ancho/alto en las
///   transformaciones rotadas (1, 3, 5, 7) y se divide por la escala entera
///   redondeando hacia arriba.
///
/// Devuelve `(ancho, alto)` en píxeles lógicos.
#[must_use]
pub fn output_logical_size(
    raw_width: i32,
    raw_height: i32,
    transform: i32,
    integer_scale: i32,
    xdg_width: i32,
    xdg_height: i32,
) -> (i32, i32) {
    if xdg_width > 0 && xdg_height > 0 {
        return (xdg_width, xdg_height);
    }
    let rotated = matches!(transform, 1 | 3 | 5 | 7);
    let (w, h) = if rotated {
        (raw_height, raw_width)
    } else {
        (raw_width, raw_height)
    };
    let scale = if integer_scale > 0 { integer_scale } else { 1 };
    let div_ceil = |n: i32| if n > 0 { (n + scale - 1) / scale } else { 0 };
    (div_ceil(w), div_ceil(h))
}

#[cfg(test)]
mod tests {
    // Casos portados verbatim de `tests/test_scale.c` (fuente independiente).
    use super::*;

    #[test]
    fn tamano_en_base_120() {
        assert_eq!(scale_size_120(100, 120), 100);
        assert_eq!(scale_size_120(100, 144), 120);
        assert_eq!(scale_size_120(100, 180), 150);
        assert_eq!(scale_size_120(100, 240), 200);
    }

    #[test]
    fn offset_negativo_conserva_signo() {
        assert_eq!(scale_offset_120(-10, 180), -15);
    }

    #[test]
    fn casos_borde() {
        assert_eq!(scale_size_120(0, 120), 0);
        assert_eq!(scale_size_120(100, 0), 0);
        assert_eq!(scale_size_120(-5, 120), 0);
    }

    #[test]
    fn tamano_logico_de_salida() {
        assert_eq!(output_logical_size(2560, 1600, 0, 2, 0, 0), (1280, 800));
        assert_eq!(output_logical_size(2560, 1600, 1, 2, 0, 0), (800, 1280));
        assert_eq!(
            output_logical_size(3840, 2160, 0, 2, 1920, 1080),
            (1920, 1080)
        );
        assert_eq!(output_logical_size(1920, 1080, 0, 1, 0, 0), (1920, 1080));
    }
}
