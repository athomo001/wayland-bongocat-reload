//! Generador del sprite sheet HD de Umbreon (Gato Negro / Pokémon) a partir del set de stickers
//! con 9 poses genuinas felinas (idle, walk1, walk2, run, angry, sleep, groom, eat, pounce).
//! Produce `themes/umbreon/sheet.png` (2048x2432 px, 16x19) y `themes/umbreon/writing.apng`.

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

    // 1. Identificar características del personaje
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

    // 2. Componentes conectados
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

    // Ordenar componentes por cercanía al centro y masa
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

    // Incluir componentes secundarios conectados o muy cercanos (< 28px) si tienen tamaño razonable (props: plato, ratón, zzz)
    for comp in &components[1..] {
        let is_near = comp.iter().any(|&(x, y)| {
            main_comp.iter().any(|&(mx, my)| {
                let dx = (x as i32 - mx as i32).abs();
                let dy = (y as i32 - my as i32).abs();
                dx * dx + dy * dy <= 784
            })
        });
        // Descartar rayas diminutas aisladas de cómic (< 40 px)
        if is_near && comp.len() >= 40 {
            for &(x, y) in comp {
                char_mask[(y * rw + x) as usize] = true;
            }
        }
    }

    // 3. Flood-fill desde bordes exteriores para obtener la silueta sólida interior
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

    // 4. Escalar a FW-14 x FH-14
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

    // 5. Borde blanco puro (#FFFFFF) de sticker die-cut de 4.0px
    let radius = 4.0f32;
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

    // Superponer personaje nítido sobre el borde blanco
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

fn blend_sprites(a: &RgbaImage, b: &RgbaImage, t: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let t_clamped = t.clamp(0.0, 1.0);
    let w_a = 1.0 - t_clamped;
    let w_b = t_clamped;

    for y in 0..FH {
        for x in 0..FW {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);

            let aa = pa[3] as f32 / 255.0;
            let ab = pb[3] as f32 / 255.0;

            let alpha = (aa * w_a + ab * w_b).clamp(0.0, 1.0);
            if alpha > 0.001 {
                let r = ((pa[0] as f32 * aa * w_a + pb[0] as f32 * ab * w_b) / alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                let g = ((pa[1] as f32 * aa * w_a + pb[1] as f32 * ab * w_b) / alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                let b = ((pa[2] as f32 * aa * w_a + pb[2] as f32 * ab * w_b) / alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                let a_byte = (alpha * 255.0).round() as u8;
                out.put_pixel(x, y, Rgba([r, g, b, a_byte]));
            }
        }
    }
    out
}

fn draw_z(img: &mut RgbaImage, cx: i32, cy: i32, size: i32, color: Rgba<u8>) {
    let s = size.max(2);
    for dx in 0..s {
        let px = cx - s / 2 + dx;
        let py_top = cy - s / 2;
        let py_bot = cy + s / 2;
        if px >= 0 && px < FW as i32 {
            if py_top >= 0 && py_top < FH as i32 {
                img.put_pixel(px as u32, py_top as u32, color);
            }
            if py_bot >= 0 && py_bot < FH as i32 {
                img.put_pixel(px as u32, py_bot as u32, color);
            }
        }
    }
    for i in 0..s {
        let px = cx + s / 2 - i;
        let py = cy - s / 2 + i;
        if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
            img.put_pixel(px as u32, py as u32, color);
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
    println!("Cargando set de stickers de Umbreon desde: {sheet_src}");
    let raw_img = image::open(sheet_src).expect("no se pudo abrir umbreon_cat_sheet.jpg");

    // Extracción de las 9 poses
    println!("Extrayendo poses felinas genuinas de Umbreon...");
    let s_idle = extract_sticker_from_rect(&raw_img, 45, 20, 278, 325);
    let s_walk1 = extract_sticker_from_rect(&raw_img, 360, 20, 300, 325);
    let s_walk2 = extract_sticker_from_rect(&raw_img, 685, 20, 305, 325);
    let s_run = extract_sticker_from_rect(&raw_img, 25, 335, 320, 350);
    let s_angry = extract_sticker_from_rect(&raw_img, 335, 335, 330, 345);
    let s_sleep = extract_sticker_from_rect(&raw_img, 680, 335, 315, 320);
    let s_groom = extract_sticker_from_rect(&raw_img, 70, 675, 275, 330);
    let s_eat = extract_sticker_from_rect(&raw_img, 375, 680, 300, 325);
    let s_pounce = extract_sticker_from_rect(&raw_img, 675, 685, 325, 320);

    let sheet_w = FW * COLS;
    let sheet_h = FH * ROWS;
    let mut sheet: RgbaImage = ImageBuffer::new(sheet_w, sheet_h);
    let tau = std::f32::consts::TAU;

    // 1. IDLE (Fila 1, 16 frames): Gato sentado erguido con respiración felina, vaivén de cola y parpadeo
    println!("Generando Fila 1: Idle (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let breath_scale = 1.0 + angle.sin() * 0.022;
        let breath_dy = angle.sin() * 1.2;
        let tail_dx = (angle * 0.5).cos() * 1.5;

        let mut spr = transform_sprite(&s_idle, tail_dx, breath_dy, 1.0, breath_scale);

        // Parpadeo sutil con ojos cerrados de satisfacción en frames 9 y 10
        if i == 9 || i == 10 {
            for dx in -3i32..=3i32 {
                let px_l = 48 + dx;
                let px_r = 74 + dx;
                let py = 54 + (dx.abs() / 2);
                if py >= 0 && py < FH as i32 {
                    if px_l >= 0 && px_l < FW as i32 {
                        spr.put_pixel(px_l as u32, py as u32, Rgba([25, 20, 20, 255]));
                    }
                    if px_r >= 0 && px_r < FW as i32 {
                        spr.put_pixel(px_r as u32, py as u32, Rgba([25, 20, 20, 255]));
                    }
                }
            }
        }

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, 0);
    }

    // 3. WRITING (Fila 3, 16 frames): Tocando/tecleando con patitas ágiles y destellos dorados
    println!("Generando Fila 3: Writing (16 frames)...");
    let mut writing_frames = Vec::new();
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let bob = -(angle * 2.0).sin().abs() * 3.0;
        let sway = angle.sin() * 2.0;

        // Alterna entre s_pounce y s_idle con ritmo vivaz
        let base_w = if i % 4 < 2 { &s_pounce } else { &s_idle };
        let mut spr = transform_sprite(base_w, sway, bob, 1.02, 0.98);

        // Anillos amarillos y destellos mágicos de tipo siniestro
        let spark_x = 64 + (angle.cos() * 26.0) as i32;
        let spark_y = 96 + (angle.sin().abs() * 8.0) as i32;
        draw_sparkle(&mut spr, spark_x, spark_y, 4, Rgba([255, 225, 0, 230]));

        writing_frames.push(spr.clone());
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (2 * FH) as i64);
    }

    // 2. START_WRITING (Fila 2, 8 frames): Transición a tecleo
    println!("Generando Fila 2: Start Writing (8 frames)...");
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&s_idle, &writing_frames[0], t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (1 * FH) as i64);
    }

    // 4. END_WRITING (Fila 4, 8 frames): Transición de tecleo a reposo
    println!("Generando Fila 4: End Writing (8 frames)...");
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&writing_frames[15], &s_idle, t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (3 * FH) as i64);
    }

    // 5. SLEEP (Fila 5, 16 frames): Ovillado en almohada durmiendo con ojos cerrados y Zzz
    println!("Generando Fila 5: Sleep (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let sleep_breath = 1.0 + angle.sin() * 0.02;
        let sleep_dy = angle.sin() * 1.0;

        let mut spr = transform_sprite(&s_sleep, 0.0, sleep_dy, 1.0, sleep_breath);

        let z_offset_y = (phase * 22.0) as i32;
        let z_offset_x = ((phase * tau * 2.0).sin() * 3.5) as i32;
        draw_z(
            &mut spr,
            88 + z_offset_x,
            34 - z_offset_y,
            7,
            Rgba([255, 215, 80, 230]),
        );
        draw_z(
            &mut spr,
            98 + z_offset_x,
            46 - z_offset_y,
            5,
            Rgba([255, 235, 140, 190]),
        );

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (4 * FH) as i64);
    }

    // 6. HAPPY (Fila 6, 16 frames): Saltar y jugar a cazar el ratón
    println!("Generando Fila 6: Happy (16 frames, salto felino)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let jump_y = -(angle.sin().max(0.0)) * 9.0;
        let squash = 1.0 + (angle * 2.0).cos() * 0.06;

        let base_h = if angle.sin() > 0.3 {
            &s_pounce
        } else {
            &s_idle
        };
        let mut spr = transform_sprite(base_h, 0.0, jump_y, 2.0 - squash, squash);

        draw_sparkle(
            &mut spr,
            64 + (angle.cos() * 24.0) as i32,
            50 + (angle.sin() * 12.0) as i32,
            3,
            Rgba([255, 220, 0, 220]),
        );

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (5 * FH) as i64);
    }

    // 7. BORING / GROOM (Fila 7, 16 frames): "Ponerse lindo" — lamiéndose la pata y lavándose la cara
    println!("Generando Fila 7: Boring / Groom (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let groom_dy = (angle * 2.0).sin() * 1.5;
        let groom_dx = angle.cos() * 1.0;

        let spr = transform_sprite(&s_groom, groom_dx, groom_dy, 1.0, 1.0);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (6 * FH) as i64);
    }

    // 8-15. LOOK_* (Filas 8 a 15, 4 frames c/u): Miradas direccionales hacia el ratón
    println!("Generando Filas 8-15: Look_* (8 direcciones)...");
    let look_dirs = [
        (-4.0f32, 0.0f32), // Left (Fila 8)
        (4.0, 0.0),        // Right (Fila 9)
        (0.0, -3.5),       // Up (Fila 10)
        (0.0, 3.5),        // Down (Fila 11)
        (-3.0, -2.5),      // Up-Left (Fila 12)
        (3.0, -2.5),       // Up-Right (Fila 13)
        (-3.0, 2.5),       // Down-Left (Fila 14)
        (3.0, 2.5),        // Down-Right (Fila 15)
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

    // 16. WAKE_UP (Fila 16, 8 frames): Despertar estirando patas delanteras y lomo
    println!("Generando Fila 16: Wake Up (8 frames)...");
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let intermediate = if t < 0.5 {
            let sub_t = t * 2.0;
            blend_sprites(&s_sleep, &s_groom, sub_t)
        } else {
            let sub_t = (t - 0.5) * 2.0;
            blend_sprites(&s_groom, &s_idle, sub_t)
        };
        image::imageops::overlay(&mut sheet, &intermediate, (i * FW) as i64, (15 * FH) as i64);
    }

    // 17. WALK (Fila 17, 16 frames): Caminata y trote felino de 4 patas articulado y fluido
    println!("Generando Fila 17: Walk (16 frames, 4 patas felinas articuladas)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let cycle_phase = (phase * 2.0) % 1.0;

        let base_step = if cycle_phase < 0.5 {
            let t = cycle_phase * 2.0;
            blend_sprites(&s_walk1, &s_walk2, t)
        } else {
            let t = (cycle_phase - 0.5) * 2.0;
            blend_sprites(&s_walk2, &s_walk1, t)
        };

        // Rebote elástico del lomo y oscilación natural de la cadera al trotar
        let trot_angle = phase * tau * 2.0;
        let trot_bob = -(trot_angle.sin().abs()) * 2.2;
        let trot_sway = trot_angle.sin() * 1.5;

        let spr = transform_sprite(&base_step, trot_sway, trot_bob, 1.0, 1.0);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (16 * FH) as i64);
    }

    // 18. EAT_RAM / SNACK (Fila 18, 16 frames): Comiendo felizmente de su plato con crujidos
    println!("Generando Fila 18: Eat (16 frames, plato de comida)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        // Cabeza sube y baja al masticar
        let chew_dy = (angle * 2.0).sin() * 2.0;
        let chew_scale_x = 1.0 + (angle * 2.0).cos() * 0.02;

        let mut spr = transform_sprite(&s_eat, 0.0, chew_dy, chew_scale_x, 1.0);

        // Chispitas o crujidos dorados alrededor del plato
        let crunch_angle = angle * 3.0;
        let cx = 40 + (crunch_angle.cos() * 8.0) as i32;
        let cy = 88 + (crunch_angle.sin() * 6.0) as i32;
        draw_sparkle(&mut spr, cx, cy, 2, Rgba([230, 160, 50, 200]));

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (17 * FH) as i64);
    }

    // 19. ANGRY (Fila 19, 16 frames): Enojarse / bufido con lomo arqueado y cola erizada
    println!("Generando Fila 19: Angry (16 frames, lomo arqueado y cola erizada)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        // Vaivén rápido de cola y respiración agitada
        let hiss_sway = (angle * 2.0).sin() * 2.2;
        let hiss_arch = 1.0 + angle.sin() * 0.03;

        let mut spr = transform_sprite(&s_angry, hiss_sway, 0.0, 1.0, hiss_arch);

        // Resplandor rojizo de ojos enojados
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

    // Guardar writing.apng (16 frames APNG con retardo de 1/12 s)
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

    println!("¡Generación de Umbreon completada exitosamente!");
}
