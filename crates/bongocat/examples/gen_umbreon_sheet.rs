//! Generador del sprite sheet HD de Umbreon (Gato Negro) con animación genuina fotograma a fotograma:
//! - Sin recortes rectangulares que partan la cabeza o el cuerpo.
//! - Escalas fijas y consistentes para que el personaje no se engorde ni encoja entre fotogramas.
//! - Un único contorno blanco die-cut limpio y suave (sin bordes fantasma ni artefactos de compresión JPEG).
//! - Caminata lateral auténtica de 4 patas (w0 a w6) con puntos intermedios y patas fijadas al suelo.
//! - Caza completa del ratón (p0 a p5): acecho -> impulso -> vuelo -> zambullida -> captura -> celebración.
//! - Aseo felino ("ponerse lindo", g0 a g5): sentado -> alzar pata -> lamer con lengua -> lavarse cara y oreja -> sacudir pata -> presumir.
//! - Modo dormir con Zzz que ascienden en olas y respiración suave.
//! - Modo idle con respiración orgánica y parpadeo tierno con punto intermedio.

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const FW: u32 = 128;
const FH: u32 = 128;
const COLS: u32 = 16;
const ROWS: u32 = 19;

/// Extrae limpiamente el personaje eliminando el fondo blanco/gris y artefactos JPEG,
/// escalándolo con una escala GLOBAL fija y posicionándolo en el lienzo con su línea de suelo.
fn extract_clean_character(
    raw_img: &DynamicImage,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
    scale: f32,
    floor_y: u32,
) -> RgbaImage {
    let sub = image::imageops::crop_imm(raw_img, rx, ry, rw, rh).to_image();

    // 1. Detección de píxeles del personaje (cuerpo oscuro, marcas amarillas, ojos rojos, props)
    let mut min_x = rw;
    let mut max_x = 0;
    let mut min_y = rh;
    let mut max_y = 0;

    let mut is_char = vec![false; (rw * rh) as usize];
    for y in 0..rh {
        for x in 0..rw {
            let p = sub.get_pixel(x, y);
            let r = p[0] as f32;
            let g = p[1] as f32;
            let b = p[2] as f32;
            let brightness = (r + g + b) / 3.0;
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);

            // Carácter: oscuro (cuerpo negro/gris) o saturado (anillos amarillos, ojos rojos, lengua, ratón)
            let is_body = brightness < 175.0 || (max_c - min_c) > 30.0;
            if is_body {
                is_char[(y * rw + x) as usize] = true;
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

    // Componentes conectados para aislar el cuerpo principal y descartar bordes o ruido exterior
    let mut visited = vec![false; (bw * bh) as usize];
    let mut components: Vec<Vec<(u32, u32)>> = Vec::new();

    for y in 0..bh {
        for x in 0..bw {
            let sx = min_x + x;
            let sy = min_y + y;
            let idx = (y * bw + x) as usize;
            if is_char[(sy * rw + sx) as usize] && !visited[idx] {
                let mut comp = Vec::new();
                let mut q = VecDeque::new();
                q.push_back((x, y));
                visited[idx] = true;

                while let Some((qx, qy)) = q.pop_front() {
                    comp.push((qx, qy));
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let nx = qx as i32 + dx;
                        let ny = qy as i32 + dy;
                        if nx >= 0 && nx < bw as i32 && ny >= 0 && ny < bh as i32 {
                            let nidx = (ny as u32 * bw + nx as u32) as usize;
                            let nsx = min_x + nx as u32;
                            let nsy = min_y + ny as u32;
                            if is_char[(nsy * rw + nsx) as usize] && !visited[nidx] {
                                visited[nidx] = true;
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

    // Ordenar por tamaño descendente (el cuerpo principal es el más grande)
    components.sort_by_key(|c| -(c.len() as isize));
    let main_comp = &components[0];

    let mut body_mask = vec![false; (bw * bh) as usize];
    let mut cb_min_x = bw;
    let mut cb_max_x = 0;
    let mut cb_min_y = bh;
    let mut cb_max_y = 0;

    for &(x, y) in main_comp {
        body_mask[(y * bw + x) as usize] = true;
        cb_min_x = cb_min_x.min(x);
        cb_max_x = cb_max_x.max(x);
        cb_min_y = cb_min_y.min(y);
        cb_max_y = cb_max_y.max(y);
    }

    // Incluir componentes adyacentes de props (ratón, comida, almohada)
    for comp in &components[1..] {
        let is_near = comp.iter().any(|&(x, y)| {
            main_comp.iter().any(|&(mx, my)| {
                let dx = (x as i32 - mx as i32).abs();
                let dy = (y as i32 - my as i32).abs();
                dx * dx + dy * dy <= 900
            })
        });
        if is_near && comp.len() >= 30 {
            for &(x, y) in comp {
                body_mask[(y * bw + x) as usize] = true;
                cb_min_x = cb_min_x.min(x);
                cb_max_x = cb_max_x.max(x);
                cb_min_y = cb_min_y.min(y);
                cb_max_y = cb_max_y.max(y);
            }
        }
    }

    let clean_w = cb_max_x - cb_min_x + 1;
    let clean_h = cb_max_y - cb_min_y + 1;
    let mut char_img: RgbaImage = ImageBuffer::new(clean_w, clean_h);

    for y in 0..clean_h {
        for x in 0..clean_w {
            let bx = cb_min_x + x;
            let by = cb_min_y + y;
            if body_mask[(by * bw + bx) as usize] {
                let p = sub.get_pixel(min_x + bx, min_y + by);
                char_img.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
            }
        }
    }

    // 2. Escalar con Lanczos3 usando la escala GLOBAL fija (no se deforma ni se agranda)
    let nw = ((clean_w as f32 * scale).round() as u32).max(1);
    let nh = ((clean_h as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(&char_img, nw, nh, image::imageops::FilterType::Lanczos3);

    // 3. Posicionar de forma armónica centrado horizontalmente y con las patas en floor_y
    let ox = (FW.saturating_sub(nw) / 2) as i64;
    let oy = (floor_y.saturating_sub(nh)) as i64;

    let mut canvas: RgbaImage = ImageBuffer::new(FW, FH);
    image::imageops::overlay(&mut canvas, &resized, ox, oy);

    // 4. Generar UN SOLO borde blanco puro die-cut nítido de 3.2px
    let radius = 3.2f32;
    let r_i = (radius + 1.0).ceil() as i32;
    let mut out: RgbaImage = ImageBuffer::new(FW, FH);

    let mut is_solid = vec![false; (FW * FH) as usize];
    for y in 0..FH {
        for x in 0..FW {
            if canvas.get_pixel(x, y)[3] > 80 {
                is_solid[(y * FW + x) as usize] = true;
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
                        && is_solid[(ny as u32 * FW + nx as u32) as usize]
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
                out.put_pixel(x, y, Rgba([255, 255, 255, (alpha * 255.0).round() as u8]));
            }
        }
    }

    // Superponer personaje nítido encima del borde blanco sin halos
    for y in 0..FH {
        for x in 0..FW {
            let p = canvas.get_pixel(x, y);
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

/// Transforma el sprite completo orgánicamente (sin recortar extremidades ni partir la cabeza)
fn transform_sprite(src: &RgbaImage, dx: f32, dy: f32, scale_x: f32, scale_y: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let pivot_x = FW as f32 / 2.0;
    let pivot_y = 118.0f32; // Anclaje constante en el suelo

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

/// Anima `idle` con respiración completa armónica y parpadeo tierno con punto intermedio
fn animate_idle(src: &RgbaImage, breath_scale: f32, breath_dy: f32, blink_state: u8) -> RgbaImage {
    let mut out = transform_sprite(src, 0.0, breath_dy, 1.0, breath_scale);

    // Puntos intermedios de parpadeo (ojos en x: 43..48 y 69..74, y: 50..56)
    if blink_state == 1 {
        // Punto intermedio: párpados bajando hasta la mitad
        for dx in -3i32..=3i32 {
            let px_l = 46 + dx;
            let px_r = 71 + dx;
            let py = 52 + (dx.abs() / 2);
            if py >= 0 && py < FH as i32 {
                if px_l >= 0 && px_l < FW as i32 {
                    out.put_pixel(px_l as u32, py as u32, Rgba([40, 36, 38, 255]));
                }
                if px_r >= 0 && px_r < FW as i32 {
                    out.put_pixel(px_r as u32, py as u32, Rgba([40, 36, 38, 255]));
                }
            }
        }
    } else if blink_state == 2 {
        // Ojos completamente cerrados en curvatura tierna
        for dx in -3i32..=3i32 {
            let px_l = 46 + dx;
            let px_r = 71 + dx;
            let py = 54 + (dx.abs() / 2);
            if py >= 0 && py < FH as i32 {
                for dy in 0..=1 {
                    let y_curr = py + dy;
                    if y_curr >= 0 && y_curr < FH as i32 {
                        if px_l >= 0 && px_l < FW as i32 {
                            out.put_pixel(px_l as u32, y_curr as u32, Rgba([30, 26, 28, 255]));
                        }
                        if px_r >= 0 && px_r < FW as i32 {
                            out.put_pixel(px_r as u32, y_curr as u32, Rgba([30, 26, 28, 255]));
                        }
                    }
                }
            }
        }
    }

    out
}

/// Dibuja una 'Z' bien visible, nítida y con borde oscuro para el sueño
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

    println!("Cargando hojas de sprites limpias de Umbreon...");
    let raw_img = image::open(sheet_src).expect("abre umbreon_cat_sheet.jpg");
    let raw_walk = image::open(walk_src).expect("abre umbreon_walk_cycle.jpg");
    let raw_pounce = image::open(pounce_src).expect("abre umbreon_pounce_seq.jpg");
    let raw_groom = image::open(groom_src).expect("abre umbreon_groom_seq.jpg");

    // 1. Poses básicas con escala fija y anclaje al suelo consistente
    println!("Extrayendo poses base limpias (escala fija 0.35, suelo 118)...");
    let s_idle = extract_clean_character(&raw_img, 45, 20, 278, 325, 0.35, 118);
    let s_sleep = extract_clean_character(&raw_img, 680, 335, 315, 320, 0.35, 118);
    let s_eat = extract_clean_character(&raw_img, 375, 680, 300, 325, 0.35, 118);
    let s_angry = extract_clean_character(&raw_img, 335, 335, 330, 345, 0.35, 118);

    // 2. Caminata de 4 patas auténtica (7 fotogramas secuenciales con escala fija 0.35 y suelo 118)
    println!("Extrayendo 7 fotogramas de caminata limpios...");
    let w0 = extract_clean_character(&raw_walk, 20, 100, 340, 320, 0.35, 118);
    let w1 = extract_clean_character(&raw_walk, 360, 100, 330, 320, 0.35, 118);
    let w2 = extract_clean_character(&raw_walk, 690, 100, 320, 320, 0.35, 118);
    let w3 = extract_clean_character(&raw_walk, 1010, 100, 350, 370, 0.35, 118);
    let w4 = extract_clean_character(&raw_walk, 110, 380, 360, 320, 0.35, 118);
    let w5 = extract_clean_character(&raw_walk, 500, 380, 360, 320, 0.35, 118);
    let w6 = extract_clean_character(&raw_walk, 890, 380, 360, 320, 0.35, 118);

    // 3. Salto y caza completa del ratón (6 fotogramas secuenciales con escala fija 0.38)
    println!("Extrayendo 6 fotogramas de caza del ratón...");
    let p0 = extract_clean_character(&raw_pounce, 40, 270, 215, 220, 0.38, 118);
    let p1 = extract_clean_character(&raw_pounce, 270, 260, 195, 230, 0.38, 118);
    let p2 = extract_clean_character(&raw_pounce, 470, 240, 240, 210, 0.38, 104);
    let p3 = extract_clean_character(&raw_pounce, 715, 230, 205, 240, 0.38, 110);
    let p4 = extract_clean_character(&raw_pounce, 930, 265, 195, 235, 0.38, 118);
    let p5 = extract_clean_character(&raw_pounce, 1140, 250, 195, 250, 0.38, 118);

    // 4. Aseo felino auténtico (6 fotogramas secuenciales con escala fija 0.33 y suelo 118)
    println!("Extrayendo 6 fotogramas de aseo felino...");
    let g0 = extract_clean_character(&raw_groom, 20, 180, 230, 350, 0.33, 118);
    let g1 = extract_clean_character(&raw_groom, 255, 180, 215, 350, 0.33, 118);
    let g2 = extract_clean_character(&raw_groom, 465, 180, 210, 350, 0.33, 118);
    let g3 = extract_clean_character(&raw_groom, 665, 180, 210, 350, 0.33, 118);
    let g4 = extract_clean_character(&raw_groom, 895, 180, 205, 350, 0.33, 118);
    let g5 = extract_clean_character(&raw_groom, 1105, 190, 245, 345, 0.33, 118);

    let sheet_w = FW * COLS;
    let sheet_h = FH * ROWS;
    let mut sheet: RgbaImage = ImageBuffer::new(sheet_w, sheet_h);
    let tau = std::f32::consts::TAU;

    // 1. IDLE (Fila 1, 16 frames): Respiración suave armónica y parpadeo tierno con punto intermedio
    println!("Generando Fila 1: Idle...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let breath_scale = 1.0 + angle.sin() * 0.015;
        let breath_dy = angle.sin() * 0.5;

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

        let spr = animate_idle(&s_idle, breath_scale, breath_dy, blink_state);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, 0);
    }

    // 3. WRITING (Fila 3, 16 frames): Tecleo ágil con patitas y destellos
    println!("Generando Fila 3: Writing...");
    let mut writing_frames = Vec::new();
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let bob = -(angle * 2.0).sin().abs() * 2.0;

        let base_w = if i % 4 < 2 { &p1 } else { &s_idle };
        let mut spr = transform_sprite(base_w, 0.0, bob, 1.01, 0.99);

        let spark_x = 64 + (angle.cos() * 22.0) as i32;
        let spark_y = 96 + (angle.sin().abs() * 6.0) as i32;
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

    // 5. SLEEP (Fila 5, 16 frames): Respiración profunda y 3 olas de Zzz ascendentes
    println!("Generando Fila 5: Sleep...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let sleep_breath = 1.0 + angle.sin() * 0.015;

        let mut spr = transform_sprite(&s_sleep, 0.0, 0.0, 1.0, sleep_breath);

        for wave in 0..3 {
            let wave_offset = wave as f32 / 3.0;
            let p_z = (phase + wave_offset) % 1.0;

            let z_x = 78 + (p_z * 20.0) as i32 + ((p_z * tau * 2.0).sin() * 4.0) as i32;
            let z_y = 52 - (p_z * 36.0) as i32;
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

    // 6. HAPPY (Fila 6, 16 frames): Secuencia auténtica completa de caza del ratón
    println!("Generando Fila 6: Happy (caza del ratón)...");
    for i in 0..16 {
        let spr = match i {
            0..=2 => &p0,   // Acecho bajo en el suelo mirando al ratón
            3..=4 => &p1,   // Punto intermedio: agazapado para tomar impulso
            5..=7 => &p2,   // Vuelo completo en el aire
            8..=10 => &p3,  // Punto intermedio: zambullida cayendo en picado
            11..=13 => &p4, // Aterrizaje atrapando el ratón bajo las patas
            _ => &p5,       // Celebración sosteniendo el ratón con gran sonrisa
        };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (5 * FH) as i64);
    }

    // 7. BORING / GROOM (Fila 7, 16 frames): Secuencia auténtica completa de aseo
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

    // 8-15. LOOK_* (Filas 8 a 15, 4 frames c/u): Miradas direccionales orgánicas
    println!("Generando Filas 8-15: Look_*...");
    let look_dirs = [
        (-3.5f32, 0.0f32),
        (3.5, 0.0),
        (0.0, -3.0),
        (0.0, 3.0),
        (-2.5, -2.0),
        (2.5, -2.0),
        (-2.5, 2.0),
        (2.5, 2.0),
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

    // 16. WAKE_UP (Fila 16, 8 frames): Despertar fluido
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
    println!("Generando Fila 17: Walk (caminata de 4 patas auténtica con inbetweens)...");
    for i in 0..16 {
        let spr = match i {
            0 | 1 => &w0,   // Contacto pata delantera y trasera
            2 | 3 => &w1,   // Punto intermedio: pata delantera en el aire
            4 | 5 => &w2,   // Apoyo y cruce de patas
            6 | 7 => &w3,   // Punto intermedio: avance de zancada en el aire
            8 | 9 => &w4,   // Contacto opuesto con suelo
            10 | 11 => &w5, // Punto intermedio: empuje de pata trasera
            12 | 13 => &w6, // Extensión final de zancada
            _ => &w0,       // Cierre fluido del ciclo
        };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (16 * FH) as i64);
    }

    // 18. EAT_RAM / SNACK (Fila 18, 16 frames): Comiendo con cabeza bajando al cuenco y subiendo a masticar
    println!("Generando Fila 18: Eat...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let chew_dy = (angle * 2.0).sin() * 1.5;

        let mut spr = transform_sprite(&s_eat, 0.0, chew_dy, 1.0, 1.0);

        let crunch_angle = angle * 3.0;
        let cx = 40 + (crunch_angle.cos() * 8.0) as i32;
        let cy = 88 + (crunch_angle.sin() * 6.0) as i32;
        draw_sparkle(&mut spr, cx, cy, 2, Rgba([230, 160, 50, 200]));

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (17 * FH) as i64);
    }

    // 19. ANGRY (Fila 19, 16 frames): Bufido amenazante con lomo arqueado y destellos
    println!("Generando Fila 19: Angry...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let hiss_arch = 1.0 + angle.sin() * 0.02;

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

    println!("¡Generación de Umbreon completada exitosamente sin deformaciones ni artefactos!");
}
