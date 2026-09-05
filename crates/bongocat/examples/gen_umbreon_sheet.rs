//! Generador del sprite sheet HD de Umbreon (Gato Negro) con animación genuina fotograma a fotograma:
//! - Puntos intermedios reales (inbetweens) sin morphing borroso.
//! - Oreja que baja al punto intermedio y sube de nuevo.
//! - Pestañeo con punto intermedio de ojos entreabiertos.
//! - Cola felina que se mueve de izquierda a derecha pasando por el centro.
//! - Caza completa del ratón (p0 a p5): acecho -> impulso -> vuelo -> zambullida -> captura -> celebración.
//! - Aseo felino ("ponerse lindo", g0 a g5): sentado -> alzar pata -> lamer con lengua -> lavarse cara y oreja -> sacudir pata -> presumir.
//! - Caminata lateral de 4 patas auténtica (w0 a w6): pisada, elevación en el aire (intermedio), apoyo y empuje.
//! - Modo dormir con Zzz que ascienden en olas y cola acurrucándose.

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const FW: u32 = 128;
const FH: u32 = 128;
const COLS: u32 = 16;
const ROWS: u32 = 19;

fn extract_sticker_from_rect(
    raw_img: &DynamicImage,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
) -> RgbaImage {
    let sub = image::imageops::crop_imm(raw_img, rx, ry, rw, rh).to_image();

    let mut is_feature = vec![false; (rw * rh) as usize];
    for y in 0..rh {
        for x in 0..rw {
            let p = sub.get_pixel(x, y);
            let r = p[0] as f32;
            let g = p[1] as f32;
            let b = p[2] as f32;
            let brightness = (r + g + b) / 3.0;
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);

            let has_color = (max_c - min_c) >= 16.0;
            let is_dark_or_mid = brightness < 225.0;

            if has_color || is_dark_or_mid {
                is_feature[(y * rw + x) as usize] = true;
            }
        }
    }

    let cx = rw as i32 / 2;
    let cy = rh as i32 / 2;
    let mut visited_feat = vec![false; (rw * rh) as usize];
    let mut components: Vec<Vec<(u32, u32)>> = Vec::new();

    for y in 0..rh {
        for x in 0..rw {
            let idx = (y * rw + x) as usize;
            if is_feature[idx] && !visited_feat[idx] {
                let mut comp = Vec::new();
                let mut q = VecDeque::new();
                q.push_back((x, y));
                visited_feat[idx] = true;

                while let Some((qx, qy)) = q.pop_front() {
                    comp.push((qx, qy));
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let nx = qx as i32 + dx;
                        let ny = qy as i32 + dy;
                        if nx >= 0 && nx < rw as i32 && ny >= 0 && ny < rh as i32 {
                            let nidx = (ny as u32 * rw + nx as u32) as usize;
                            if is_feature[nidx] && !visited_feat[nidx] {
                                visited_feat[nidx] = true;
                                q.push_back((nx as u32, ny as u32));
                            }
                        }
                    }
                }
                components.push(comp);
            }
        }
    }

    if components.is_empty() {
        return ImageBuffer::new(FW, FH);
    }

    components.sort_by_key(|c| {
        let size = c.len() as i32;
        let avg_x = c.iter().map(|&(x, _)| x as i32).sum::<i32>() / size.max(1);
        let avg_y = c.iter().map(|&(_, y)| y as i32).sum::<i32>() / size.max(1);
        let dist_center = (avg_x - cx).pow(2) + (avg_y - cy).pow(2);
        -(size * 1000 - dist_center)
    });

    let main_comp = &components[0];
    let mut char_mask = vec![false; (rw * rh) as usize];
    for &(x, y) in main_comp {
        char_mask[(y * rw + x) as usize] = true;
    }

    // Incluir props (ratón, plato, etc.)
    for comp in &components[1..] {
        let is_near = comp.iter().any(|&(x, y)| {
            main_comp.iter().any(|&(mx, my)| {
                let dx = (x as i32 - mx as i32).abs();
                let dy = (y as i32 - my as i32).abs();
                dx * dx + dy * dy <= 784
            })
        });
        if is_near && comp.len() >= 40 {
            for &(x, y) in comp {
                char_mask[(y * rw + x) as usize] = true;
            }
        }
    }

    let mut outside = vec![false; (rw * rh) as usize];
    let mut q_out = VecDeque::new();

    for x in 0..rw {
        q_out.push_back((x, 0));
        q_out.push_back((x, rh - 1));
    }
    for y in 0..rh {
        q_out.push_back((0, y));
        q_out.push_back((rw - 1, y));
    }

    while let Some((x, y)) = q_out.pop_front() {
        let idx = (y * rw + x) as usize;
        if outside[idx] {
            continue;
        }
        outside[idx] = true;

        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx >= 0 && nx < rw as i32 && ny >= 0 && ny < rh as i32 {
                let nidx = (ny as u32 * rw + nx as u32) as usize;
                if !outside[nidx] && !char_mask[nidx] {
                    q_out.push_back((nx as u32, ny as u32));
                }
            }
        }
    }

    let mut body_mask = vec![false; (rw * rh) as usize];
    let mut min_x = rw;
    let mut max_x = 0;
    let mut min_y = rh;
    let mut max_y = 0;

    for y in 0..rh {
        for x in 0..rw {
            let idx = (y * rw + x) as usize;
            if !outside[idx] {
                body_mask[idx] = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }

    if min_x > max_x || min_y > max_y {
        return ImageBuffer::new(FW, FH);
    }

    let bw = max_x - min_x + 1;
    let bh = max_y - min_y + 1;
    let mut char_img: RgbaImage = ImageBuffer::new(bw, bh);

    for y in 0..bh {
        for x in 0..bw {
            let sx = min_x + x;
            let sy = min_y + y;
            let idx = (sy * rw + sx) as usize;
            if body_mask[idx] {
                let p = sub.get_pixel(sx, sy);
                char_img.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
            } else {
                char_img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }

    let target_w = FW - 14;
    let target_h = FH - 14;
    let scale = (target_w as f32 / bw as f32).min(target_h as f32 / bh as f32);
    let nw = ((bw as f32 * scale).round() as u32).max(1);
    let nh = ((bh as f32 * scale).round() as u32).max(1);

    let resized_char =
        image::imageops::resize(&char_img, nw, nh, image::imageops::FilterType::Lanczos3);

    let ox = (FW - nw) / 2;
    let oy = FH.saturating_sub(nh + 5);

    let mut char_canvas: RgbaImage = ImageBuffer::new(FW, FH);
    image::imageops::overlay(&mut char_canvas, &resized_char, ox as i64, oy as i64);

    // Borde blanco die-cut puro de 3.8px
    let radius = 3.8f32;
    let r_i = (radius + 1.0).ceil() as i32;
    let mut out: RgbaImage = ImageBuffer::new(FW, FH);

    let mut is_solid_px = vec![false; (FW * FH) as usize];
    for y in 0..FH {
        for x in 0..FW {
            if char_canvas.get_pixel(x, y)[3] > 60 {
                is_solid_px[(y * FW + x) as usize] = true;
            }
        }
    }

    for y in 0..FH {
        for x in 0..FW {
            let mut min_d2 = 999999.0f32;
            for dy in -r_i..=r_i {
                for dx in -r_i..=r_i {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0
                        && nx < FW as i32
                        && ny >= 0
                        && ny < FH as i32
                        && is_solid_px[(ny as u32 * FW + nx as u32) as usize]
                    {
                        let d2 = (dx * dx + dy * dy) as f32;
                        if d2 < min_d2 {
                            min_d2 = d2;
                        }
                    }
                }
            }

            let dist = min_d2.sqrt();
            if dist <= radius {
                let alpha = (radius + 0.6 - dist).clamp(0.0, 1.0);
                let a_byte = (alpha * 255.0).round() as u8;
                out.put_pixel(x, y, Rgba([255, 255, 255, a_byte]));
            }
        }
    }

    for y in 0..FH {
        for x in 0..FW {
            let p = char_canvas.get_pixel(x, y);
            if p[3] > 0 {
                let a_norm = p[3] as f32 / 255.0;
                let bg = out.get_pixel(x, y);
                let r = (p[0] as f32 * a_norm + bg[0] as f32 * (1.0 - a_norm)).round() as u8;
                let g = (p[1] as f32 * a_norm + bg[1] as f32 * (1.0 - a_norm)).round() as u8;
                let b = (p[2] as f32 * a_norm + bg[2] as f32 * (1.0 - a_norm)).round() as u8;
                let out_a = (bg[3] as f32).max(p[3] as f32) as u8;
                out.put_pixel(x, y, Rgba([r, g, b, out_a]));
            }
        }
    }

    out
}

fn sample_bilinear(src: &RgbaImage, sx: f32, sy: f32) -> Rgba<u8> {
    let (w, h) = src.dimensions();
    if sx < -0.5 || sx > w as f32 - 0.5 || sy < -0.5 || sy > h as f32 - 0.5 {
        return Rgba([0, 0, 0, 0]);
    }
    let x0 = sx.floor() as i32;
    let y0 = sy.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let fx = (sx - x0 as f32).clamp(0.0, 1.0);
    let fy = (sy - y0 as f32).clamp(0.0, 1.0);

    let get = |x: i32, y: i32| -> [f32; 4] {
        if x >= 0 && x < w as i32 && y >= 0 && y < h as i32 {
            let p = src.get_pixel(x as u32, y as u32);
            [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        }
    };

    let p00 = get(x0, y0);
    let p10 = get(x1, y0);
    let p01 = get(x0, y1);
    let p11 = get(x1, y1);

    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] * (1.0 - fx) + p10[c] * fx;
        let bot = p01[c] * (1.0 - fx) + p11[c] * fx;
        let v = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0);
        out[c] = v as u8;
    }
    Rgba(out)
}

fn transform_sprite(src: &RgbaImage, dx: f32, dy: f32, scale_x: f32, scale_y: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let pivot_x = FW as f32 / 2.0;
    let pivot_y = FH as f32 - 10.0;

    for y in 0..FH {
        for x in 0..FW {
            let cur_x = x as f32 - dx;
            let cur_y = y as f32 - dy;

            let src_x = pivot_x + (cur_x - pivot_x) / scale_x.max(0.01);
            let src_y = pivot_y + (cur_y - pivot_y) / scale_y.max(0.01);

            let p = sample_bilinear(src, src_x, src_y);
            if p[3] > 0 {
                out.put_pixel(x, y, p);
            }
        }
    }
    out
}

/// Anima `idle` con:
/// - Cola que oscila suavemente de izquierda a derecha.
/// - Oreja izquierda que se mueve hacia abajo pasando por el punto intermedio.
/// - Pestañeo con punto intermedio de ojos entreabiertos.
fn animate_idle(
    src: &RgbaImage,
    tail_angle: f32,
    ear_angle: f32,
    breath_dy: f32,
    breath_scale: f32,
    blink_state: u8, // 0: abierto, 1: entreabierto (intermedio), 2: cerrado
) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let pivot_x = FW as f32 / 2.0;
    let pivot_y = FH as f32 - 10.0;

    let tail_pivot_x = 73.0f32;
    let tail_pivot_y = 96.0f32;
    let (sin_t, cos_t) = tail_angle.sin_cos();

    let ear_pivot_x = 48.0f32;
    let ear_pivot_y = 48.0f32;
    let (sin_e, cos_e) = ear_angle.sin_cos();

    for y in 0..FH {
        for x in 0..FW {
            // 1. Región de la oreja izquierda: articulación independiente con punto intermedio
            if x <= 56 && y <= 50 {
                let rx = x as f32 - ear_pivot_x;
                let ry = y as f32 - ear_pivot_y;
                let src_x = ear_pivot_x + rx * cos_e + ry * sin_e;
                let src_y = ear_pivot_y - rx * sin_e + ry * cos_e;
                let p = sample_bilinear(src, src_x, src_y);
                if p[3] > 0 {
                    out.put_pixel(x, y, p);
                    continue;
                }
            }

            // 2. Región de la cola felina: vaivén suave a izquierda y derecha
            if x >= 70 && y >= 45 && y <= 112 {
                let rx = x as f32 - tail_pivot_x;
                let ry = y as f32 - tail_pivot_y;
                let src_x = tail_pivot_x + rx * cos_t + ry * sin_t;
                let src_y = tail_pivot_y - rx * sin_t + ry * cos_t;
                let p = sample_bilinear(src, src_x, src_y);
                if p[3] > 0 {
                    out.put_pixel(x, y, p);
                    continue;
                }
            }

            // 3. Cabeza y cuerpo: respiración vertical suave
            let cur_x = x as f32;
            let cur_y = y as f32 - breath_dy;
            let src_x = pivot_x + (cur_x - pivot_x);
            let src_y = pivot_y + (cur_y - pivot_y) / breath_scale.max(0.01);

            let p = sample_bilinear(src, src_x, src_y);
            if p[3] > 0 {
                out.put_pixel(x, y, p);
            }
        }
    }

    // 4. Parpadeo con punto intermedio real
    if blink_state == 1 {
        // Punto intermedio: párpado cayendo hasta la mitad
        for dx in -3i32..=3i32 {
            let px_l = 48 + dx;
            let px_r = 74 + dx;
            let py = 52 + (dx.abs() / 2);
            if py >= 0 && py < FH as i32 {
                if px_l >= 0 && px_l < FW as i32 {
                    out.put_pixel(px_l as u32, py as u32, Rgba([30, 24, 24, 255]));
                }
                if px_r >= 0 && px_r < FW as i32 {
                    out.put_pixel(px_r as u32, py as u32, Rgba([30, 24, 24, 255]));
                }
            }
        }
    } else if blink_state == 2 {
        // Ojos completamente cerrados en curvatura tierna
        for dx in -3i32..=3i32 {
            let px_l = 48 + dx;
            let px_r = 74 + dx;
            let py = 55 + (dx.abs() / 2);
            if py >= 0 && py < FH as i32 {
                if px_l >= 0 && px_l < FW as i32 {
                    out.put_pixel(px_l as u32, py as u32, Rgba([25, 20, 20, 255]));
                }
                if px_r >= 0 && px_r < FW as i32 {
                    out.put_pixel(px_r as u32, py as u32, Rgba([25, 20, 20, 255]));
                }
            }
        }
    }

    out
}

/// Anima el gato durmiendo con movimiento de punta de cola articulada
fn animate_sleep_with_tail(src: &RgbaImage, tail_curl: f32, breath_scale: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let pivot_x = FW as f32 / 2.0;
    let pivot_y = FH as f32 - 10.0;

    let tail_pivot_x = 84.0f32;
    let tail_pivot_y = 92.0f32;
    let (sin_t, cos_t) = tail_curl.sin_cos();

    for y in 0..FH {
        for x in 0..FW {
            // Región de la punta de la cola
            if x >= 78 && y >= 68 && y <= 108 {
                let rx = x as f32 - tail_pivot_x;
                let ry = y as f32 - tail_pivot_y;
                let src_x = tail_pivot_x + rx * cos_t + ry * sin_t;
                let src_y = tail_pivot_y - rx * sin_t + ry * cos_t;
                let p = sample_bilinear(src, src_x, src_y);
                if p[3] > 0 {
                    out.put_pixel(x, y, p);
                    continue;
                }
            }

            let cur_x = x as f32;
            let cur_y = y as f32;
            let src_x = pivot_x + (cur_x - pivot_x);
            let src_y = pivot_y + (cur_y - pivot_y) / breath_scale.max(0.01);

            let p = sample_bilinear(src, src_x, src_y);
            if p[3] > 0 {
                out.put_pixel(x, y, p);
            }
        }
    }
    out
}

/// Dibuja una 'Z' bien visible, nítida y con borde oscuro
fn draw_bold_z(img: &mut RgbaImage, cx: i32, cy: i32, size: i32, alpha_factor: f32) {
    let s = size.max(3);
    let a = (245.0 * alpha_factor).clamp(0.0, 255.0) as u8;
    let color = Rgba([255, 225, 75, a]);
    let outline_a = (180.0 * alpha_factor).clamp(0.0, 255.0) as u8;
    let outline = Rgba([30, 20, 20, outline_a]);

    for dx in 0..s {
        let px = cx - s / 2 + dx;
        let py = cy - s / 2;
        put_px_thick(img, px, py, color, outline);
    }
    for i in 0..s {
        let px = cx + s / 2 - i;
        let py = cy - s / 2 + i;
        put_px_thick(img, px, py, color, outline);
    }
    for dx in 0..s {
        let px = cx - s / 2 + dx;
        let py = cy + s / 2;
        put_px_thick(img, px, py, color, outline);
    }
}

fn put_px_thick(img: &mut RgbaImage, x: i32, y: i32, col: Rgba<u8>, outline: Rgba<u8>) {
    for dy in -1..=1 {
        for dx in -1..=1 {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
                if dx == 0 && dy == 0 {
                    img.put_pixel(px as u32, py as u32, col);
                } else if img.get_pixel(px as u32, py as u32)[3] < 120 {
                    img.put_pixel(px as u32, py as u32, outline);
                }
            }
        }
    }
}

fn draw_sparkle(img: &mut RgbaImage, cx: i32, cy: i32, rad: i32, color: Rgba<u8>) {
    for dy in -rad..=rad {
        for dx in -rad..=rad {
            if dx.abs() + dy.abs() <= rad {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
                    img.put_pixel(px as u32, py as u32, color);
                }
            }
        }
    }
}

fn main() {
    let sheet_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_cat_sheet_1788633569390.jpg";
    let walk_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_walk_cycle_1788636213890.jpg";
    let pounce_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_pounce_seq_1788637088867.jpg";
    let groom_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_groom_seq_1788637133036.jpg";

    println!("Cargando sets de sprites...");
    let raw_img = image::open(sheet_src).expect("abre umbreon_cat_sheet.jpg");
    let raw_walk = image::open(walk_src).expect("abre umbreon_walk_cycle.jpg");
    let raw_pounce = image::open(pounce_src).expect("abre umbreon_pounce_seq.jpg");
    let raw_groom = image::open(groom_src).expect("abre umbreon_groom_seq.jpg");

    // 1. Poses básicas
    let s_idle = extract_sticker_from_rect(&raw_img, 45, 20, 278, 325);
    let s_angry = extract_sticker_from_rect(&raw_img, 335, 335, 330, 345);
    let s_sleep = extract_sticker_from_rect(&raw_img, 680, 335, 315, 320);
    let s_eat = extract_sticker_from_rect(&raw_img, 375, 680, 300, 325);

    // 2. Caminata de 4 patas lateral (7 fotogramas secuenciales con sus puntos intermedios reales)
    println!("Extrayendo 7 fotogramas de caminata...");
    let w0 = extract_sticker_from_rect(&raw_walk, 25, 160, 310, 285);
    let w1 = extract_sticker_from_rect(&raw_walk, 380, 145, 305, 250);
    let w2 = extract_sticker_from_rect(&raw_walk, 710, 150, 300, 245);
    let w3 = extract_sticker_from_rect(&raw_walk, 1040, 160, 305, 250);
    let w4 = extract_sticker_from_rect(&raw_walk, 125, 410, 325, 255);
    let w5 = extract_sticker_from_rect(&raw_walk, 525, 405, 325, 255);
    let w6 = extract_sticker_from_rect(&raw_walk, 905, 410, 325, 255);

    // 3. Salto y caza completa del ratón (6 fotogramas secuenciales)
    println!("Extrayendo secuencia de caza del ratón...");
    let p0 = extract_sticker_from_rect(&raw_pounce, 40, 295, 205, 190);
    let p1 = extract_sticker_from_rect(&raw_pounce, 280, 275, 180, 210);
    let p2 = extract_sticker_from_rect(&raw_pounce, 480, 250, 230, 190);
    let p3 = extract_sticker_from_rect(&raw_pounce, 725, 240, 185, 225);
    let p4 = extract_sticker_from_rect(&raw_pounce, 940, 275, 180, 220);
    let p5 = extract_sticker_from_rect(&raw_pounce, 1150, 260, 175, 235);

    // 4. Aseo felino auténtico (6 fotogramas secuenciales)
    println!("Extrayendo secuencia de aseo...");
    let g0 = extract_sticker_from_rect(&raw_groom, 30, 200, 215, 325);
    let g1 = extract_sticker_from_rect(&raw_groom, 265, 200, 200, 325);
    let g2 = extract_sticker_from_rect(&raw_groom, 475, 195, 195, 330);
    let g3 = extract_sticker_from_rect(&raw_groom, 675, 195, 195, 330);
    let g4 = extract_sticker_from_rect(&raw_groom, 905, 195, 185, 330);
    let g5 = extract_sticker_from_rect(&raw_groom, 1115, 205, 235, 325);

    let sheet_w = FW * COLS;
    let sheet_h = FH * ROWS;
    let mut sheet: RgbaImage = ImageBuffer::new(sheet_w, sheet_h);
    let tau = std::f32::consts::TAU;

    // 1. IDLE (Fila 1, 16 frames): Oreja con punto intermedio, parpadeo con punto intermedio y vaivén de cola
    println!("Generando Fila 1: Idle...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let breath_scale = 1.0 + angle.sin() * 0.015;
        let breath_dy = angle.sin() * 0.8;
        let tail_angle = angle.sin() * 0.16;

        // Oreja izquierda: en frames 3..7 hace un movimiento con punto intermedio
        // frame 3: arriba (0°) -> frame 4: medio (10°) -> frame 5: abajo (20°) -> frame 6: medio (10°) -> frame 7: arriba (0°)
        let ear_angle = match i {
            4 => 0.17, // Punto intermedio bajando (~10 grados)
            5 => 0.35, // Abajo (~20 grados)
            6 => 0.17, // Punto intermedio subiendo (~10 grados)
            _ => 0.0,  // Arriba recta
        };

        // Pestañeo:
        // frame 9: ojos entreabiertos (punto intermedio = 1)
        // frame 10: ojos completamente cerrados (2)
        // frame 11: ojos entreabiertos (punto intermedio = 1)
        // otros: abiertos (0)
        let blink_state = match i {
            9 | 11 => 1,
            10 => 2,
            _ => 0,
        };

        let spr = animate_idle(
            &s_idle,
            tail_angle,
            ear_angle,
            breath_dy,
            breath_scale,
            blink_state,
        );
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, 0);
    }

    // 3. WRITING (Fila 3, 16 frames): Tocando/tecleando con patitas ágiles
    println!("Generando Fila 3: Writing...");
    let mut writing_frames = Vec::new();
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let bob = -(angle * 2.0).sin().abs() * 2.5;

        let base_w = if i % 4 < 2 { &p1 } else { &s_idle };
        let mut spr = transform_sprite(base_w, 0.0, bob, 1.01, 0.99);

        let spark_x = 64 + (angle.cos() * 24.0) as i32;
        let spark_y = 96 + (angle.sin().abs() * 8.0) as i32;
        draw_sparkle(&mut spr, spark_x, spark_y, 4, Rgba([255, 225, 0, 230]));

        writing_frames.push(spr.clone());
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (2 * FH) as i64);
    }

    // 2. START_WRITING (Fila 2, 8 frames): Transición a tecleo
    println!("Generando Fila 2: Start Writing...");
    for i in 0..8 {
        let spr = if i < 4 { &s_idle } else { &writing_frames[0] };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (1 * FH) as i64);
    }

    // 4. END_WRITING (Fila 4, 8 frames): Transición a reposo
    println!("Generando Fila 4: End Writing...");
    for i in 0..8 {
        let spr = if i < 4 { &writing_frames[15] } else { &s_idle };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (3 * FH) as i64);
    }

    // 5. SLEEP (Fila 5, 16 frames): Cola que se enrosca y 3 olas de Zzz ascendentes
    println!("Generando Fila 5: Sleep...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let sleep_breath = 1.0 + angle.sin() * 0.018;
        let tail_curl = angle.sin() * 0.12;

        let mut spr = animate_sleep_with_tail(&s_sleep, tail_curl, sleep_breath);

        for wave in 0..3 {
            let wave_offset = wave as f32 / 3.0;
            let p_z = (phase + wave_offset) % 1.0;

            let z_x = 76 + (p_z * 22.0) as i32 + ((p_z * tau * 2.0).sin() * 4.0) as i32;
            let z_y = 52 - (p_z * 38.0) as i32;
            let size = 3 + (p_z * 6.0) as i32;
            let alpha = if p_z < 0.15 {
                p_z / 0.15
            } else if p_z > 0.75 {
                (1.0 - p_z) / 0.25
            } else {
                1.0
            };

            draw_bold_z(&mut spr, z_x, z_y, size, alpha);
        }

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (4 * FH) as i64);
    }

    // 6. HAPPY (Fila 6, 16 frames): SECUENCIA REAL COMPLETA DE CAZA DEL RATÓN
    // Distribución progresiva de los 6 fotogramas para que cada pose se aprecie con claridad
    println!("Generando Fila 6: Happy (caza del ratón)...");
    for i in 0..16 {
        let spr = match i {
            0..=2 => &p0,   // Acecho bajo en el suelo mirando al ratón
            3..=4 => &p1,   // Punto intermedio: encogiéndose para tomar impulso
            5..=7 => &p2,   // Vuelo completo en el aire
            8..=10 => &p3,  // Punto intermedio: zambullida cayendo en picado
            11..=13 => &p4, // Aterrizaje atrapando el ratón bajo las patas
            _ => &p5,       // Celebración sosteniendo el ratón con gran sonrisa
        };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (5 * FH) as i64);
    }

    // 7. BORING / GROOM (Fila 7, 16 frames): SECUENCIA REAL COMPLETA DE ASEO
    // Sentarse -> punto intermedio alzar pata -> lamer con lengua -> lavarse cara y oreja -> sacudir -> presumir
    println!("Generando Fila 7: Boring / Groom (aseo)...");
    for i in 0..16 {
        let spr = match i {
            0..=2 => &g0,   // Sentado tranquilo
            3..=4 => &g1,   // Punto intermedio: pata levantándose a la boca
            5..=7 => &g2,   // Lamiéndose la pata con la lengua rosada afuera
            8..=10 => &g3,  // Lavándose la cara y la oreja derecha doblada hacia abajo
            11..=13 => &g4, // Punto intermedio: oreja subiendo y sacudiendo la pata
            _ => &g5,       // Sentado orgulloso, limpio y con una tierna sonrisa
        };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (6 * FH) as i64);
    }

    // 8-15. LOOK_* (Filas 8 a 15, 4 frames c/u): Miradas direccionales
    println!("Generando Filas 8-15: Look_*...");
    let look_dirs = [
        (-4.0f32, 0.0f32),
        (4.0, 0.0),
        (0.0, -3.5),
        (0.0, 3.5),
        (-3.0, -2.5),
        (3.0, -2.5),
        (-3.0, 2.5),
        (3.0, 2.5),
    ];

    for (d_idx, &(look_dx, look_dy)) in look_dirs.iter().enumerate() {
        let row = 7 + d_idx;
        for i in 0..4 {
            let t = i as f32 / 3.0;
            let dx = look_dx * t;
            let dy = look_dy * t;
            let spr = transform_sprite(&s_idle, dx, dy, 1.0, 1.0);
            image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (row as u32 * FH) as i64);
        }
    }

    // 16. WAKE_UP (Fila 16, 8 frames): Despertar
    println!("Generando Fila 16: Wake Up...");
    for i in 0..8 {
        let spr = if i < 3 {
            &s_sleep
        } else if i < 6 {
            &g1 // Punto intermedio desperezándose
        } else {
            &s_idle
        };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (15 * FH) as i64);
    }

    // 17. WALK (Fila 17, 16 frames): CAMINATA DE 4 PATAS CON PUNTOS INTERMEDIOS DE MARCHA REAL
    // Paso rítmico coordinado de los 7 fotogramas secuenciales en perfil
    println!("Generando Fila 17: Walk (caminata de 4 patas con puntos intermedios)...");
    for i in 0..16 {
        let spr = match i {
            0 | 1 => &w0,   // Contacto pata trasera y delantera
            2 | 3 => &w1,   // Punto intermedio: elevación de pata en el aire
            4 | 5 => &w2,   // Apoyo y cruce de patas
            6 | 7 => &w3,   // Punto intermedio: avance de zancada en el aire
            8 | 9 => &w4,   // Contacto de la otra pata
            10 | 11 => &w5, // Punto intermedio: empuje contra el suelo
            12 | 13 => &w6, // Extensión final de zancada
            _ => &w0,       // Retorno al ciclo
        };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (16 * FH) as i64);
    }

    // 18. EAT_RAM / SNACK (Fila 18, 16 frames): Comiendo con cabeza bajando al cuenco y subiendo a masticar
    println!("Generando Fila 18: Eat...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let chew_dy = (angle * 2.0).sin() * 2.0;

        let mut spr = transform_sprite(&s_eat, 0.0, chew_dy, 1.0, 1.0);

        let crunch_angle = angle * 3.0;
        let cx = 40 + (crunch_angle.cos() * 8.0) as i32;
        let cy = 88 + (crunch_angle.sin() * 6.0) as i32;
        draw_sparkle(&mut spr, cx, cy, 2, Rgba([230, 160, 50, 200]));

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (17 * FH) as i64);
    }

    // 19. ANGRY (Fila 19, 16 frames): Enojarse / bufido con lomo arqueado
    println!("Generando Fila 19: Angry...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let hiss_arch = 1.0 + angle.sin() * 0.025;

        let mut spr = transform_sprite(&s_angry, 0.0, 0.0, 1.0, hiss_arch);

        draw_sparkle(&mut spr, 38, 64, 2, Rgba([255, 30, 30, 230]));
        draw_sparkle(&mut spr, 54, 64, 2, Rgba([255, 30, 30, 230]));

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (18 * FH) as i64);
    }

    // Guardar sheet.png
    let out_dir = Path::new("themes/umbreon");
    std::fs::create_dir_all(out_dir).expect("crea directorio themes/umbreon");
    let sheet_path = out_dir.join("sheet.png");
    println!(
        "Guardando sprite sheet ({sheet_w}x{sheet_h}) en: {}",
        sheet_path.display()
    );
    sheet.save(&sheet_path).expect("guarda sheet.png");

    // Guardar writing.apng (16 frames APNG)
    let apng_path = out_dir.join("writing.apng");
    println!("Guardando writing.apng en: {}", apng_path.display());
    let file = File::create(&apng_path).expect("crea writing.apng");
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, FW, FH);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(16, 0).expect("configura APNG");
    encoder
        .set_frame_delay(1, 12)
        .expect("configura delay APNG");

    let mut writer = encoder.write_header().expect("escribe cabecera APNG");
    for frame in &writing_frames {
        writer
            .write_image_data(frame.as_raw())
            .expect("escribe fotograma APNG");
    }
    writer.finish().expect("finaliza APNG");

    println!("¡Generación de Umbreon con puntos intermedios completada exitosamente!");
}
