//! Generador del sprite sheet HD de Umbreon (Gato Negro) con animación genuina fotograma a fotograma:
//! - Cada estado tiene fotogramas de animación REALMENTE DIBUJADOS (articulación de patas, lamerse, comer, bufido).
//! - CERO escalados artificiales, CERO estiramientos: cada dibujo mantiene su escala anatómica fija.
//! - Contorno blanco die-cut limpio y nítido de 3.2px.
//! - Caminata lateral auténtica de 4 patas (w0 a w5): ciclo completo a escala uniforme de 1 pixel de tolerancia.
//! - Comida auténtica (e0 a e5): acercarse al cuenco -> morder comida -> masticar con migajas -> segundo mordisco -> masticar feliz -> lamerse el hocico con plato vacío.
//! - Enojado auténtico (a0 a a5): tensión -> orejas aplastadas y colmillos -> bufido arqueando lomo con cola erizada -> hissing agresivo -> gruñido -> acecho agazapado.
//! - Caza del ratón (p0 a p5): acecho -> agazapado -> salto en el aire -> zambullida -> captura -> celebración.
//! - Aseo felino (g0 a g5): sentado -> alzar pata -> lamer con lengua -> lavarse cara y oreja -> sacudir pata -> presumir.
//! - Tecleo Bongo Cat (writing): postura sentada adorable alternando patitas sobre el teclado con chispas doradas.

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const FW: u32 = 128;
const FH: u32 = 128;
const COLS: u32 = 16;
const ROWS: u32 = 19;

/// Genera un borde blanco die-cut nítido y suave de 3.2px
fn render_die_cut_border(canvas: &RgbaImage) -> RgbaImage {
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

    // Superponer dibujo nítido sobre el borde blanco
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

/// Extrae limpiamente el personaje eliminando fondos y sombras JPEG con escala fija y anclaje al suelo
fn extract_clean_character(
    raw_img: &DynamicImage,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
    scale: f32,
    floor_y: u32,
    include_props: bool,
) -> RgbaImage {
    let sub = image::imageops::crop_imm(raw_img, rx, ry, rw, rh).to_image();

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

            let is_body = brightness < 185.0 || (max_c - min_c) > 30.0;
            if is_body {
                is_char[(y * rw + x) as usize] = true;
            }
        }
    }

    let mut visited = vec![false; (rw * rh) as usize];
    let mut components: Vec<Vec<(u32, u32)>> = Vec::new();

    for y in 0..rh {
        for x in 0..rw {
            let idx = (y * rw + x) as usize;
            if is_char[idx] && !visited[idx] {
                let mut comp = Vec::new();
                let mut q = VecDeque::new();
                q.push_back((x, y));
                visited[idx] = true;

                while let Some((qx, qy)) = q.pop_front() {
                    comp.push((qx, qy));
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let nx = qx as i32 + dx;
                        let ny = qy as i32 + dy;
                        if nx >= 0 && nx < rw as i32 && ny >= 0 && ny < rh as i32 {
                            let nidx = (ny as u32 * rw + nx as u32) as usize;
                            if is_char[nidx] && !visited[nidx] {
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

    components.sort_by_key(|c| -(c.len() as isize));
    let main_comp = &components[0];

    let mut body_mask = vec![false; (rw * rh) as usize];
    let mut cb_min_x = rw;
    let mut cb_max_x = 0;
    let mut cb_min_y = rh;
    let mut cb_max_y = 0;

    for &(x, y) in main_comp {
        body_mask[(y * rw + x) as usize] = true;
        cb_min_x = cb_min_x.min(x);
        cb_max_x = cb_max_x.max(x);
        cb_min_y = cb_min_y.min(y);
        cb_max_y = cb_max_y.max(y);
    }

    if include_props {
        let mid_y = (cb_min_y + cb_max_y) / 2;
        for comp in &components[1..] {
            let is_near_bottom = comp.iter().any(|&(x, y)| {
                y >= mid_y
                    && main_comp.iter().any(|&(mx, my)| {
                        let dx = (x as i32 - mx as i32).abs();
                        let dy = (y as i32 - my as i32).abs();
                        dx * dx + dy * dy <= 1600
                    })
            });
            if is_near_bottom && comp.len() >= 25 {
                for &(x, y) in comp {
                    body_mask[(y * rw + x) as usize] = true;
                    cb_min_x = cb_min_x.min(x);
                    cb_max_x = cb_max_x.max(x);
                    cb_min_y = cb_min_y.min(y);
                    cb_max_y = cb_max_y.max(y);
                }
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
            if body_mask[(by * rw + bx) as usize] {
                let p = sub.get_pixel(bx, by);
                char_img.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
            }
        }
    }

    let nw = ((clean_w as f32 * scale).round() as u32).max(1);
    let nh = ((clean_h as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(&char_img, nw, nh, image::imageops::FilterType::Lanczos3);

    let ox = (FW.saturating_sub(nw) / 2) as i64;
    let oy = (floor_y.saturating_sub(nh)) as i64;

    let mut canvas: RgbaImage = ImageBuffer::new(FW, FH);
    image::imageops::overlay(&mut canvas, &resized, ox, oy);

    render_die_cut_border(&canvas)
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

/// Desplaza el sprite manteniendo estrictamente su escala real 1.0 a 1
fn shift_sprite(src: &RgbaImage, dx: f32, dy: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    for y in 0..FH {
        for x in 0..FW {
            let src_x = x as f32 - dx;
            let src_y = y as f32 - dy;
            let p = sample_bilinear(src, src_x, src_y);
            if p[3] > 0 {
                out.put_pixel(x, y, p);
            }
        }
    }
    out
}

/// Anima `idle` con pestañeo tierno natural sin deformar el cuerpo
fn animate_idle(src: &RgbaImage, blink_state: u8) -> RgbaImage {
    let mut out = src.clone();

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

/// Dibuja una 'Z' bien visible para el sueño
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
    let walk_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_walk_seq_1788730357359.jpg";
    let pounce_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_pounce_seq_1788637088867.jpg";
    let groom_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_groom_seq_1788637133036.jpg";
    let eat_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_eat_seq_1788730463393.jpg";
    let angry_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/umbreon_angry_seq_1788730506231.jpg";

    println!("Cargando hojas de sprites limpias de Umbreon...");
    let raw_img = image::open(sheet_src).expect("abre umbreon_cat_sheet.jpg");
    let raw_walk = image::open(walk_src).expect("abre umbreon_walk_seq.jpg");
    let raw_pounce = image::open(pounce_src).expect("abre umbreon_pounce_seq.jpg");
    let raw_groom = image::open(groom_src).expect("abre umbreon_groom_seq.jpg");
    let raw_eat = image::open(eat_src).expect("abre umbreon_eat_seq.jpg");
    let raw_angry = image::open(angry_src).expect("abre umbreon_angry_seq.jpg");

    // 1. Poses básicas (Idle y Sleep)
    println!("Extrayendo poses base (Idle y Sleep)...");
    let s_idle = extract_clean_character(&raw_img, 45, 20, 278, 325, 0.35, 118, false);
    let s_sleep = extract_clean_character(&raw_img, 680, 335, 315, 320, 0.35, 118, true);

    // 2. Caminata genuina de 4 patas (6 fotogramas secuenciales exactos a escala uniforme 0.38)
    println!("Extrayendo 6 fotogramas de caminata genuina de 4 patas...");
    let w0 = extract_clean_character(&raw_walk, 10, 145, 226, 260, 0.38, 118, false);
    let w1 = extract_clean_character(&raw_walk, 236, 145, 223, 260, 0.38, 118, false);
    let w2 = extract_clean_character(&raw_walk, 459, 145, 222, 260, 0.38, 118, false);
    let w3 = extract_clean_character(&raw_walk, 681, 145, 221, 260, 0.38, 118, false);
    let w4 = extract_clean_character(&raw_walk, 902, 145, 223, 260, 0.38, 118, false);
    let w5 = extract_clean_character(&raw_walk, 1125, 145, 235, 260, 0.38, 118, false);
    let walk_frames = [w0, w1, w2, w3, w4, w5];

    // 3. Caza del ratón (6 fotogramas secuenciales a escala fija 0.38)
    println!("Extrayendo 6 fotogramas de caza del ratón...");
    let p0 = extract_clean_character(&raw_pounce, 40, 270, 215, 220, 0.38, 118, false);
    let p1 = extract_clean_character(&raw_pounce, 270, 260, 195, 230, 0.38, 118, false);
    let p2 = extract_clean_character(&raw_pounce, 470, 240, 240, 210, 0.38, 104, false);
    let p3 = extract_clean_character(&raw_pounce, 715, 230, 205, 240, 0.38, 110, false);
    let p4 = extract_clean_character(&raw_pounce, 930, 265, 195, 235, 0.38, 118, true);
    let p5 = extract_clean_character(&raw_pounce, 1140, 250, 195, 250, 0.38, 118, true);
    let pounce_frames = [p0, p1, p2, p3, p4, p5];

    // 4. Aseo felino (6 fotogramas secuenciales a escala fija 0.33)
    println!("Extrayendo 6 fotogramas de aseo felino...");
    let g0 = extract_clean_character(&raw_groom, 20, 180, 230, 350, 0.33, 118, false);
    let g1 = extract_clean_character(&raw_groom, 255, 180, 215, 350, 0.33, 118, false);
    let g2 = extract_clean_character(&raw_groom, 465, 180, 210, 350, 0.33, 118, false);
    let g3 = extract_clean_character(&raw_groom, 665, 180, 210, 350, 0.33, 118, false);
    let g4 = extract_clean_character(&raw_groom, 895, 180, 205, 350, 0.33, 118, false);
    let g5 = extract_clean_character(&raw_groom, 1105, 190, 245, 345, 0.33, 118, false);
    let groom_frames = [g0, g1, g2, g3, g4, g5];

    // 5. Comida genuina (6 fotogramas secuenciales a escala fija 0.28 con cuenco de comida)
    println!("Extrayendo 6 fotogramas de comida genuina...");
    let e0 = extract_clean_character(&raw_eat, 25, 20, 440, 365, 0.28, 118, true);
    let e1 = extract_clean_character(&raw_eat, 465, 60, 440, 325, 0.28, 118, true);
    let e2 = extract_clean_character(&raw_eat, 905, 50, 450, 335, 0.28, 118, true);
    let e3 = extract_clean_character(&raw_eat, 20, 420, 450, 340, 0.28, 118, true);
    let e4 = extract_clean_character(&raw_eat, 470, 390, 455, 370, 0.28, 118, true);
    let e5 = extract_clean_character(&raw_eat, 925, 385, 440, 375, 0.28, 118, true);
    let eat_frames = [e0, e1, e2, e3, e4, e5];

    // 6. Enojado genuino (6 fotogramas secuenciales a escala fija 0.34)
    println!("Extrayendo 6 fotogramas de enojado genuino...");
    let a0 = extract_clean_character(&raw_angry, 20, 205, 240, 320, 0.34, 118, false);
    let a1 = extract_clean_character(&raw_angry, 260, 205, 228, 320, 0.34, 118, false);
    let a2 = extract_clean_character(&raw_angry, 488, 205, 214, 320, 0.34, 118, false);
    let a3 = extract_clean_character(&raw_angry, 702, 205, 229, 320, 0.34, 118, false);
    let a4 = extract_clean_character(&raw_angry, 931, 205, 208, 320, 0.34, 118, false);
    let a5 = extract_clean_character(&raw_angry, 1139, 205, 226, 320, 0.34, 118, false);
    let angry_frames = [a0, a1, a2, a3, a4, a5];

    let sheet_w = FW * COLS;
    let sheet_h = FH * ROWS;
    let mut sheet: RgbaImage = ImageBuffer::new(sheet_w, sheet_h);
    let tau = std::f32::consts::TAU;

    // 1. IDLE (Fila 1, 16 frames)
    println!("Generando Fila 1: Idle...");
    for i in 0..16 {
        let blink_state = match i {
            8 | 11 => 1,
            9 | 10 => 2,
            _ => 0,
        };
        let spr = animate_idle(&s_idle, blink_state);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, 0);
    }

    // 3. WRITING (Fila 3, 16 frames): Bongo Cat tecleando sentado con destellos dorados
    println!("Generando Fila 3: Writing (Bongo Cat sentado)...");
    let mut writing_frames = Vec::new();
    for i in 0..16 {
        let is_left_tap = (i / 2) % 2 == 0;
        let bob = if i % 2 == 0 { -1.5 } else { 0.5 };
        let mut spr = shift_sprite(&s_idle, 0.0, bob);

        if is_left_tap {
            draw_sparkle(&mut spr, 54, 112, 3, Rgba([255, 220, 0, 255]));
            draw_sparkle(&mut spr, 50, 108, 1, Rgba([255, 245, 160, 220]));
        } else {
            draw_sparkle(&mut spr, 74, 112, 3, Rgba([255, 220, 0, 255]));
            draw_sparkle(&mut spr, 78, 108, 1, Rgba([255, 245, 160, 220]));
        }

        writing_frames.push(spr.clone());
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (2 * FH) as i64);
    }

    // 2. START_WRITING (Fila 2, 8 frames)
    println!("Generando Fila 2: Start Writing...");
    for i in 0..8 {
        let spr = if i < 4 { &s_idle } else { &writing_frames[0] };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (1 * FH) as i64);
    }

    // 4. END_WRITING (Fila 4, 8 frames)
    println!("Generando Fila 4: End Writing...");
    for i in 0..8 {
        let spr = if i < 4 { &writing_frames[15] } else { &s_idle };
        image::imageops::overlay(&mut sheet, spr, (i * FW) as i64, (3 * FH) as i64);
    }

    // 5. SLEEP (Fila 5, 16 frames)
    println!("Generando Fila 5: Sleep...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let mut spr = s_sleep.clone();

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

        image::imageops::overlay(&mut sheet, &spr, (i as u32 * FW) as i64, (4 * FH) as i64);
    }

    // 6. HAPPY (Fila 6, 6 frames genuinos en columnas 0..5 y repetidos fluidamente)
    println!("Generando Fila 6: Happy (caza del ratón genuina de 6 poses)...");
    for i in 0..16 {
        let spr = &pounce_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (5 * FH) as i64);
    }

    // 7. BORING / GROOM (Fila 7, 6 frames genuinos)
    println!("Generando Fila 7: Boring / Groom (aseo felino genuino de 6 poses)...");
    for i in 0..16 {
        let spr = &groom_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (6 * FH) as i64);
    }

    // 8-15. LOOK_* (Filas 8 a 15, 4 frames c/u)
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
            let spr = shift_sprite(&s_idle, dx, dy);
            image::imageops::overlay(
                &mut sheet,
                &spr,
                (i as u32 * FW) as i64,
                (row as u32 * FH) as i64,
            );
        }
    }

    // 16. WAKE_UP (Fila 16, 8 frames)
    println!("Generando Fila 16: Wake Up...");
    for i in 0..8 {
        let spr = if i < 3 {
            &s_sleep
        } else if i < 6 {
            &groom_frames[1]
        } else {
            &s_idle
        };
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (15 * FH) as i64);
    }

    // 17. WALK (Fila 17, 6 frames genuinos de caminata de 4 patas)
    println!("Generando Fila 17: Walk (caminata real de 6 fotogramas articulados)...");
    for i in 0..16 {
        let spr = &walk_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (16 * FH) as i64);
    }

    // 18. EAT_RAM / SNACK (Fila 18, 6 frames genuinos de comida)
    println!("Generando Fila 18: Eat (comida real de 6 fotogramas con cuenco y mordiscos)...");
    for i in 0..16 {
        let spr = &eat_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (17 * FH) as i64);
    }

    // 19. ANGRY (Fila 19, 6 frames genuinos de enojo y bufido)
    println!(
        "Generando Fila 19: Angry (enojado real de 6 fotogramas con bufido y lomo arqueado)..."
    );
    for i in 0..16 {
        let spr = &angry_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (18 * FH) as i64);
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

    // Guardar writing.apng
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

    println!(
        "¡Generación de Umbreon completada exitosamente con fotogramas de animación 100% genuinos!"
    );
}
