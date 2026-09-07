//! Generador del sprite sheet HD de Gabumon (Digimon) a partir del set de stickers
//! con 7 poses genuinas (idle, walk1, walk2, walk3, sleep, eat_meat, happy).
//! Produce `vpets/gabumon/sheet.png` (2048x2304 px) y `vpets/gabumon/writing.apng`.

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const FW: u32 = 128;
const FH: u32 = 128;
const COLS: u32 = 16;
const ROWS: u32 = 18;

fn extract_sticker_from_rect(
    raw_img: &DynamicImage,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
) -> RgbaImage {
    let sub = image::imageops::crop_imm(raw_img, rx, ry, rw, rh).to_image();

    // 1. Identificar características del personaje (color saturado o líneas de dibujo oscuras)
    let mut is_feature = vec![false; (rw * rh) as usize];
    for y in 0..rh {
        for x in 0..rw {
            let p = sub.get_pixel(x, y);
            let r = p[0] as f32;
            let g = p[1] as f32;
            let b = p[2] as f32;
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);

            let has_color = (max_c - min_c) >= 14.0;
            let is_dark_line = max_c < 100.0;

            if has_color || is_dark_line {
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

    // Incluir componentes secundarios conectados o muy cercanos (< 16px)
    for comp in &components[1..] {
        let is_near = comp.iter().any(|&(x, y)| {
            main_comp.iter().any(|&(mx, my)| {
                let dx = (x as i32 - mx as i32).abs();
                let dy = (y as i32 - my as i32).abs();
                dx * dx + dy * dy <= 256
            })
        });
        if is_near && comp.len() > 8 {
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

    // 5. Borde blanco puro (#FFFFFF) de sticker die-cut de 4.2px
    let radius = 4.2f32;
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
            let a = p[3] as f32 / 255.0;
            [
                p[0] as f32 * a,
                p[1] as f32 * a,
                p[2] as f32 * a,
                p[3] as f32,
            ]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        }
    };

    let p00 = get(x0, y0);
    let p10 = get(x1, y0);
    let p01 = get(x0, y1);
    let p11 = get(x1, y1);

    let a =
        (p00[3] * (1.0 - fx) + p10[3] * fx) * (1.0 - fy) + (p01[3] * (1.0 - fx) + p11[3] * fx) * fy;
    let a_clamped = a.clamp(0.0, 255.0);
    if a_clamped < 1.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let a_norm = a_clamped / 255.0;
    let mut res = [0u8; 4];
    for c in 0..3 {
        let col = (p00[c] * (1.0 - fx) + p10[c] * fx) * (1.0 - fy)
            + (p01[c] * (1.0 - fx) + p11[c] * fx) * fy;
        res[c] = (col / a_norm).round().clamp(0.0, 255.0) as u8;
    }
    res[3] = a_clamped.round() as u8;
    Rgba(res)
}

fn transform_sprite(src: &RgbaImage, dx: f32, dy: f32, scale_x: f32, scale_y: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let base_x = FW as f32 / 2.0;
    let base_y = FH as f32 - 5.0;

    for py in 0..FH {
        let sy = (py as f32 - dy - base_y) / scale_y + base_y;
        for px in 0..FW {
            let sx = (px as f32 - dx - base_x) / scale_x + base_x;
            let p = sample_bilinear(src, sx, sy);
            if p[3] > 0 {
                out.put_pixel(px, py, p);
            }
        }
    }
    out
}

fn blend_sprites(a: &RgbaImage, b: &RgbaImage, t: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let t = t.clamp(0.0, 1.0);
    let ease = t * t * (3.0 - 2.0 * t);
    for y in 0..FH {
        for x in 0..FW {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);
            if pa[3] == 0 && pb[3] == 0 {
                continue;
            }
            let a_norm = pa[3] as f32 / 255.0;
            let b_norm = pb[3] as f32 / 255.0;
            let out_a = pa[3] as f32 * (1.0 - ease) + pb[3] as f32 * ease;
            if out_a <= 0.0 {
                continue;
            }
            let mut pix = [0u8; 4];
            for c in 0..3 {
                let col = pa[c] as f32 * a_norm * (1.0 - ease) + pb[c] as f32 * b_norm * ease;
                let normalized = (col / (out_a / 255.0)).round().clamp(0.0, 255.0);
                pix[c] = normalized as u8;
            }
            pix[3] = out_a.round().clamp(0.0, 255.0) as u8;
            out.put_pixel(x, y, Rgba(pix));
        }
    }
    out
}

fn draw_sparkle(img: &mut RgbaImage, cx: i32, cy: i32, r: i32, color: Rgba<u8>) {
    for dy in -r..=r {
        for dx in -r..=r {
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
                let d = (dx * dx + dy * dy) as f32;
                if d <= (r * r) as f32 {
                    let a = 1.0 - (d / ((r * r) as f32)).sqrt();
                    let cur = img.get_pixel(px as u32, py as u32);
                    let blend_a = (a * 255.0).round() as u8;
                    if blend_a > cur[3] {
                        img.put_pixel(px as u32, py as u32, color);
                    }
                }
            }
        }
    }
}

fn draw_z(img: &mut RgbaImage, x0: i32, y0: i32, size: i32, color: Rgba<u8>) {
    for i in 0..size {
        let px1 = x0 + i;
        let py1 = y0;
        if px1 >= 0 && px1 < FW as i32 && py1 >= 0 && py1 < FH as i32 {
            img.put_pixel(px1 as u32, py1 as u32, color);
        }
        let px2 = x0 + size - 1 - i;
        let py2 = y0 + i;
        if px2 >= 0 && px2 < FW as i32 && py2 >= 0 && py2 < FH as i32 {
            img.put_pixel(px2 as u32, py2 as u32, color);
        }
        let px3 = x0 + i;
        let py3 = y0 + size - 1;
        if px3 >= 0 && px3 < FW as i32 && py3 >= 0 && py3 < FH as i32 {
            img.put_pixel(px3 as u32, py3 as u32, color);
        }
    }
}

fn main() {
    let sheet_src = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/gabumon_chibi_sheet_1788631338177.jpg";
    println!("Cargando set de stickers de Gabumon desde: {sheet_src}");
    let raw_img = image::open(sheet_src).expect("no se pudo abrir gabumon_chibi_sheet.jpg");

    // Extracción de las 7 poses reales
    println!("Extrayendo poses genuinas de Gabumon...");
    let s_idle = extract_sticker_from_rect(&raw_img, 30, 15, 315, 335);
    let s_walk1 = extract_sticker_from_rect(&raw_img, 360, 20, 305, 330);
    let s_walk2 = extract_sticker_from_rect(&raw_img, 650, 20, 310, 330);
    let s_walk3 = extract_sticker_from_rect(&raw_img, 70, 345, 295, 335);
    let s_sleep = extract_sticker_from_rect(&raw_img, 605, 380, 345, 245);
    let s_eat = extract_sticker_from_rect(&raw_img, 50, 660, 310, 320);
    let s_happy = extract_sticker_from_rect(&raw_img, 635, 630, 340, 345);

    let sheet_w = FW * COLS;
    let sheet_h = FH * ROWS;
    let mut sheet: RgbaImage = ImageBuffer::new(sheet_w, sheet_h);
    let tau = std::f32::consts::TAU;

    // 1. IDLE (Fila 1, 16 frames): Gabumon frontal de pie con respiración elástica, parpadeo y vaivén
    println!("Generando Fila 1: Idle (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let breath_scale = 1.0 + angle.sin() * 0.025;
        let breath_dy = angle.sin() * 1.5;
        let sway_dx = (angle * 0.5).cos() * 1.2;

        let mut spr = transform_sprite(&s_idle, sway_dx, breath_dy, 1.0, breath_scale);

        // Parpadeo tierno en frames 10 y 11
        if i == 10 || i == 11 {
            for dx in -4i32..=4i32 {
                let px = 64 + dx;
                let py = 45 + (dx.abs() / 2);
                if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
                    spr.put_pixel(px as u32, py as u32, Rgba([35, 25, 25, 255]));
                }
            }
        }

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, 0);
    }

    // 3. WRITING (Fila 3, 16 frames): Tecleo/acción rítmica con chispas doradas
    println!("Generando Fila 3: Writing (16 frames)...");
    let mut writing_frames = Vec::new();
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let bob = -(angle * 2.0).sin().abs() * 3.2;
        let sway = angle.sin() * 2.0;

        // Alterna entre s_happy y s_idle con balanceo enérgico
        let base_w = if i % 4 < 2 { &s_happy } else { &s_idle };
        let mut spr = transform_sprite(base_w, sway, bob, 1.02, 0.98);

        let spark_x = 64 + (angle.cos() * 28.0) as i32;
        let spark_y = 100 + (angle.sin().abs() * 8.0) as i32;
        draw_sparkle(&mut spr, spark_x, spark_y, 4, Rgba([255, 215, 0, 230]));

        writing_frames.push(spr.clone());
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (2 * FH) as i64);
    }

    // 2. START_WRITING (Fila 2, 8 frames): Transición suave a tecleo
    println!("Generando Fila 2: Start Writing (8 frames)...");
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&s_idle, &writing_frames[0], t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, FH as i64);
    }

    // 4. END_WRITING (Fila 4, 8 frames): Transición suave de tecleo a reposo
    println!("Generando Fila 4: End Writing (8 frames)...");
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&writing_frames[15], &s_idle, t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (3 * FH) as i64);
    }

    // 5. SLEEP (Fila 5, 16 frames): GABUMON REAL ACOSTADO/ACURRUCADO CON OJOS CERRADOS Y ZZZ
    println!("Generando Fila 5: Sleep (16 frames, pose acostada genuina)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let sleep_breath = 1.0 + angle.sin() * 0.018;
        let sleep_dy = angle.sin() * 1.0;

        let mut spr = transform_sprite(&s_sleep, 0.0, sleep_dy, 1.0, sleep_breath);

        // Zzzz flotando suavemente sobre el cuerno de Gabumon
        let z_offset_y = (phase * 24.0) as i32;
        let z_offset_x = ((phase * tau * 2.0).sin() * 4.0) as i32;
        draw_z(
            &mut spr,
            80 + z_offset_x,
            32 - z_offset_y,
            7,
            Rgba([90, 170, 255, 230]),
        );
        draw_z(
            &mut spr,
            92 + z_offset_x,
            44 - z_offset_y,
            5,
            Rgba([130, 195, 255, 190]),
        );

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (4 * FH) as i64);
    }

    // 6. HAPPY (Fila 6, 16 frames): Salto alegre y celebración
    println!("Generando Fila 6: Happy (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let jump_y = -(angle.sin().max(0.0) * 8.5);
        let happy_sway = (angle * 2.0).sin() * 2.5;

        let mut spr = transform_sprite(&s_happy, happy_sway, jump_y, 1.0, 1.0);
        let star_x = 64 + ((i as i32 * 17) % 80 - 40);
        let star_y = 22 + ((i as i32 * 13) % 40);
        draw_sparkle(&mut spr, star_x, star_y, 4, Rgba([255, 225, 70, 240]));

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (5 * FH) as i64);
    }

    // 7. BORING (Fila 7, 8 frames): Descansando relajado
    println!("Generando Fila 7: Boring (8 frames)...");
    for i in 0..8 {
        let phase = i as f32 / 8.0;
        let angle = phase * tau;
        let spr = transform_sprite(&s_idle, 0.0, 3.0 + angle.sin() * 1.0, 1.02, 0.96);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (6 * FH) as i64);
    }

    // 8-15. LOOK_* (Filas 8-15, 4 frames c/u): Mirada en 8 direcciones
    println!("Generando Filas 8-15: Look directions (4 frames c/u)...");
    let look_offsets = [
        (-4.0, 0.0),  // LookLeft (Fila 8)
        (4.0, 0.0),   // LookRight (Fila 9)
        (0.0, -4.0),  // LookUp (Fila 10)
        (0.0, 3.5),   // LookDown (Fila 11)
        (-3.0, -3.0), // LookUpLeft (Fila 12)
        (3.0, -3.0),  // LookUpRight (Fila 13)
        (-3.0, 3.0),  // LookDownLeft (Fila 14)
        (3.0, 3.0),   // LookDownRight (Fila 15)
    ];

    for (idx, &(lx, ly)) in look_offsets.iter().enumerate() {
        let row = 7 + idx as u32;
        for i in 0..4 {
            let breath = (i as f32 / 4.0 * tau).sin() * 0.8;
            let spr = transform_sprite(&s_idle, lx, ly + breath, 1.0, 1.0);
            image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (row * FH) as i64);
        }
    }

    // 16. WAKE_UP (Fila 16, 8 frames): Despertar estirándose desde s_sleep hacia s_idle
    println!("Generando Fila 16: Wake Up (8 frames)...");
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&s_sleep, &s_idle, t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (15 * FH) as i64);
    }

    // 17. WALK (Fila 17, 16 frames): CAMINATA REAL CON LAS POSES GENUINAS DE PASOS
    println!("Generando Fila 17: Walk (16 frames, ciclo de caminata articulado)...");
    let walk_cycle = [&s_walk1, &s_walk2, &s_walk3, &s_walk2];
    for i in 0..16 {
        let cycle_pos = (i as f32 / 16.0) * 4.0;
        let idx0 = cycle_pos.floor() as usize % 4;
        let idx1 = (idx0 + 1) % 4;
        let t = cycle_pos.fract();

        let base_step = blend_sprites(walk_cycle[idx0], walk_cycle[idx1], t);

        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let bob = -(angle * 2.0).sin().abs() * 2.5;
        let sway = angle.sin() * 1.5;

        let spr = transform_sprite(&base_step, sway, bob, 1.0, 1.0);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (16 * FH) as i64);
    }

    // 18. EAT_RAM / EAT_SNACK (Fila 18, 16 frames): GABUMON REAL SENTADO COMIENDO CARNE CON HUESO
    println!("Generando Fila 18: Eat Snack (16 frames, pose comiendo carne genuina)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let chew = (angle * 3.0).sin().abs();
        let chew_scale_x = 1.0 + chew * 0.035;
        let chew_scale_y = 1.0 - chew * 0.025;
        let chew_dy = chew * 1.2;

        let spr = transform_sprite(&s_eat, 0.0, chew_dy, chew_scale_x, chew_scale_y);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (17 * FH) as i64);
    }

    // Guardar vpets/gabumon/sheet.png
    let out_dir = Path::new("vpets/gabumon");
    std::fs::create_dir_all(out_dir).expect("no se pudo crear directorio vpets/gabumon");

    let sheet_path = out_dir.join("sheet.png");
    sheet
        .save(&sheet_path)
        .expect("no se pudo guardar sheet.png");
    println!("Guardado exitosamente: {}", sheet_path.display());

    // Generar vpets/gabumon/writing.apng
    let apng_path = out_dir.join("writing.apng");
    let apng_file = File::create(&apng_path).expect("no se pudo crear writing.apng");
    let mut writer = BufWriter::new(apng_file);
    let mut enc = png::Encoder::new(&mut writer, FW, FH);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let num_frames = writing_frames.len() as u32;
    enc.set_animated(num_frames, 0)
        .expect("activar animacion APNG");
    enc.set_frame_delay(1, 12).expect("configurar fps");
    let mut stream = enc.write_header().expect("escribir cabecera APNG");

    for f in &writing_frames {
        stream
            .write_image_data(f.as_raw())
            .expect("escribir fotograma APNG");
    }
    println!("Guardado exitosamente: {}", apng_path.display());
    println!("¡Generación completada con 7 poses reales de Gabumon!");
}
