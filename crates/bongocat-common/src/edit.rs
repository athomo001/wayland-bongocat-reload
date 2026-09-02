//! Geometría pura del **modo edición con ratón** (spec 0005 M1): dónde está el
//! gato dentro de la barra, cómo pasar de "origen que quiero" a los `cat_*_offset`
//! de la config, y los topes (rueda, "que no se salga de la barra").
//!
//! Todo en coordenadas **lógicas** de la surface. El overlay ya convierte a
//! físicas al dibujar (`scale_*_120`), así que el hit-test y el arrastre viven
//! aquí sin saber nada de HiDPI.

use crate::config::{Align, Config};

/// Relación de aspecto de referencia del gato (igual que `anim::REF_W/REF_H`).
pub const CAT_ASPECT_W: i32 = 500;
pub const CAT_ASPECT_H: i32 = 277;

/// Rango de `cat_height` (igual que la validación de config).
pub const MIN_CAT_HEIGHT: i32 = 10;
pub const MAX_CAT_HEIGHT: i32 = 200;

/// Ancho del gato para una altura dada (relación de aspecto).
#[must_use]
pub fn cat_width_for_height(h: i32) -> i32 {
    (h.max(1) * CAT_ASPECT_W / CAT_ASPECT_H).max(1)
}

/// Rectángulo `(x, y, w, h)` del gato dentro de una barra de `bar_w`×`bar_h`,
/// en píxeles lógicos. Reproduce el posicionamiento de `draw_bar` / `cat_origin`.
#[must_use]
pub fn cat_rect(cfg: &Config, bar_w: i32, bar_h: i32) -> (i32, i32, i32, i32) {
    let ch = cfg.cat_height.clamp(MIN_CAT_HEIGHT, MAX_CAT_HEIGHT);
    let cw = cat_width_for_height(ch);
    let y = (bar_h - ch) / 2 + cfg.cat_y_offset;
    let x = match cfg.cat_align {
        Align::Left => cfg.cat_x_offset,
        Align::Center => (bar_w - cw) / 2 + cfg.cat_x_offset,
        Align::Right => bar_w - cw - cfg.cat_x_offset,
    };
    (x, y, cw, ch)
}

/// ¿El punto `(px, py)` cae dentro de `rect = (x, y, w, h)`?
#[must_use]
pub fn hit(rect: (i32, i32, i32, i32), px: i32, py: i32) -> bool {
    let (x, y, w, h) = rect;
    px >= x && px < x + w && py >= y && py < y + h
}

/// Invierte la fórmula de alineación: dado el **origen X** deseado del gato (su
/// borde izquierdo, en lógicas), devuelve el `cat_x_offset` que lo produce.
#[must_use]
pub fn origin_to_x_offset(align: Align, origin_x: i32, bar_w: i32, cat_w: i32) -> i32 {
    match align {
        Align::Left => origin_x,
        Align::Center => origin_x - (bar_w - cat_w) / 2,
        Align::Right => bar_w - cat_w - origin_x,
    }
}

/// Ídem para el eje Y (siempre centrado + offset).
#[must_use]
pub fn origin_to_y_offset(origin_y: i32, bar_h: i32, cat_h: i32) -> i32 {
    origin_y - (bar_h - cat_h) / 2
}

/// Recorta el **origen** `(ox, oy)` para que el **centro** del gato no salga de
/// la barra: así siempre queda al menos media figura dentro y se puede volver a
/// agarrar.
#[must_use]
pub fn clamp_origin(
    ox: i32,
    oy: i32,
    bar_w: i32,
    bar_h: i32,
    cat_w: i32,
    cat_h: i32,
) -> (i32, i32) {
    let cx = ox.clamp(-cat_w / 2, bar_w - cat_w / 2);
    let cy = oy.clamp(-cat_h / 2, bar_h - cat_h / 2);
    (cx, cy)
}

/// Paso de la rueda para `cat_height`: fino con `Shift`.
#[must_use]
pub fn wheel_step(shift: bool) -> i32 {
    if shift {
        1
    } else {
        4
    }
}

/// Aplica un paso a `cat_height` respetando el rango.
#[must_use]
pub fn resize_cat_height(current: i32, step: i32) -> i32 {
    (current + step).clamp(MIN_CAT_HEIGHT, MAX_CAT_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg(align: Align, xo: i32, yo: i32, h: i32) -> Config {
        Config {
            cat_align: align,
            cat_x_offset: xo,
            cat_y_offset: yo,
            cat_height: h,
            ..Config::default()
        }
    }

    #[test]
    fn ida_y_vuelta_origen_offset_por_alineacion() {
        let (bar_w, bar_h) = (1920, 120);
        for align in [Align::Left, Align::Center, Align::Right] {
            let c = cfg(align, 37, -11, 90);
            let (x, y, w, h) = cat_rect(&c, bar_w, bar_h);
            // origen -> offset -> debería reproducir los offsets originales.
            assert_eq!(
                origin_to_x_offset(align, x, bar_w, w),
                c.cat_x_offset,
                "{align:?}"
            );
            assert_eq!(origin_to_y_offset(y, bar_h, h), c.cat_y_offset, "{align:?}");
        }
    }

    #[test]
    fn hit_test() {
        let r = (100, 10, 40, 30);
        assert!(hit(r, 100, 10));
        assert!(hit(r, 139, 39));
        assert!(!hit(r, 140, 20), "borde derecho exclusivo");
        assert!(!hit(r, 50, 20));
    }

    #[test]
    fn clamp_mantiene_el_centro_del_gato_en_la_barra() {
        // Muy a la izquierda: el centro no pasa de x=0 (origen = -cat_w/2).
        let (x, _) = clamp_origin(-9999, 0, 1920, 120, 100, 80);
        assert_eq!(x, -50);
        // Muy a la derecha: el centro no pasa de x=bar_w.
        let (x2, _) = clamp_origin(9999, 0, 1920, 120, 100, 80);
        assert_eq!(x2, 1920 - 50);
    }

    #[test]
    fn rueda_respeta_el_rango() {
        assert_eq!(wheel_step(false), 4);
        assert_eq!(wheel_step(true), 1);
        assert_eq!(resize_cat_height(198, 4), 200, "tope superior");
        assert_eq!(resize_cat_height(12, -4), 10, "tope inferior");
        assert_eq!(resize_cat_height(100, 4), 104);
    }
}
