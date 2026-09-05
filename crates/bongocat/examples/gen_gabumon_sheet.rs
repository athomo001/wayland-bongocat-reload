//! Generador del sprite sheet HD de Gabumon (Digimon) en formato sticker die-cut
//! con borde blanco limpio y animaciones completas (16 columnas x 18 filas)
//! para `themes/gabumon/sheet.png` y `writing.apng`.

use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba, RgbaImage};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const FW: u32 = 128;
const FH: u32 = 128;
const COLS: u32 = 16;
const ROWS: u32 = 18;

fn extract_gabumon_sticker(src: &DynamicImage) -> RgbaImage {
    let (iw, ih) = src.dimensions();
    let mut min_x = iw;
    let mut min_y = ih;
    let mut max_x = 0;
    let mut max_y = 0;

    for y in 0..ih {
        for x in 0..iw {
            let p = src.get_pixel(x, y);
            if p[3] > 25 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if min_x > max_x || min_y > max_y {
        return ImageBuffer::new(FW, FH);
    }

    let bw = max_x - min_x + 1;
    let bh = max_y - min_y + 1;
    let cropped = image::imageops::crop_imm(src, min_x, min_y, bw, bh).to_image();

    // Escalar para que quepa en FW-14 x FH-14 con margen para el sticker blanco
    let target_w = FW - 14;
    let target_h = FH - 14;
    let scale = (target_w as f32 / bw as f32).min(target_h as f32 / bh as f32);
    let nw = ((bw as f32 * scale).round() as u32).max(1);
    let nh = ((bh as f32 * scale).round() as u32).max(1);

    let resized = image::imageops::resize(&cropped, nw, nh, image::imageops::FilterType::Lanczos3);

    let ox = (FW - nw) / 2;
    let oy = FH.saturating_sub(nh + 5);

    let mut canvas: RgbaImage = ImageBuffer::new(FW, FH);
    image::imageops::overlay(&mut canvas, &resized, ox as i64, oy as i64);

    // Generar borde blanco de sticker die-cut de 4.2px
    let radius = 4.2f32;
    let r_i = (radius + 1.0).ceil() as i32;
    let mut out: RgbaImage = ImageBuffer::new(FW, FH);

    let mut is_solid = vec![false; (FW * FH) as usize];
    for y in 0..FH {
        for x in 0..FW {
            if canvas.get_pixel(x, y)[3] > 50 {
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
                let a_byte = (alpha * 255.0).round() as u8;
                out.put_pixel(x, y, Rgba([255, 255, 255, a_byte]));
            }
        }
    }

    // Superponer personaje sobre el borde blanco
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

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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

/// Caminata continua y articulada de Gabumon mediante deformación suave (smoothstep):
/// - Pata izquierda y derecha alternan zancadas y elevación sin cortar el cuerpo.
/// - Garras de los brazos balancean en contraposición.
/// - El cuerno dorado y las orejas de la piel de Garurumon tienen vaivén inercial elástico.
fn walk_gabumon_sprite(src: &RgbaImage, phase: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let tau = std::f32::consts::TAU;
    let angle = phase * tau;

    // Rebote rítmico y vaivén de peso
    let body_bob = -(angle * 2.0).sin().abs() * 3.2;
    let body_sway = angle.sin() * 1.8;

    // Amplitud de zancadas
    let stride_amp = 4.2f32;
    let lift_amp = 3.5f32;

    let left_leg_dx = angle.sin() * stride_amp;
    let left_leg_dy = -angle.sin().max(0.0) * lift_amp;

    let right_leg_dx = -angle.sin() * stride_amp;
    let right_leg_dy = -(-angle.sin()).max(0.0) * lift_amp;

    // Balanceo inercial del cuerno y orejas
    let horn_sway = (angle + 0.5).sin() * 3.0;
    let horn_bob = -(angle * 2.0 + 0.3).cos() * 1.5;

    // Balanceo de brazos y garras
    let arm_swing = (angle + std::f32::consts::PI).sin() * 3.5;

    for py in 0..FH {
        let y_f = py as f32;
        let leg_mask_y = smoothstep(80.0, 115.0, y_f);
        let horn_mask_y = smoothstep(55.0, 15.0, y_f); // Cuerno y orejas en la parte superior

        for px in 0..FW {
            let x_f = px as f32;

            // Pierna izquierda (delantera en la pose)
            let leg_l_weight =
                smoothstep(25.0, 42.0, x_f) * smoothstep(75.0, 58.0, x_f) * leg_mask_y;
            // Pierna derecha (trasera en la pose)
            let leg_r_weight =
                smoothstep(60.0, 78.0, x_f) * smoothstep(112.0, 95.0, x_f) * leg_mask_y;

            // Brazos / garras laterales
            let arm_l_weight = smoothstep(45.0, 18.0, x_f) * smoothstep(40.0, 85.0, y_f);
            let arm_r_weight = smoothstep(85.0, 115.0, x_f) * smoothstep(40.0, 85.0, y_f);

            let mut dx = body_sway;
            let mut dy = body_bob;

            // Cuerno y orejas superiores
            dx += horn_mask_y * horn_sway;
            dy += horn_mask_y * horn_bob;

            // Brazos
            dx += arm_l_weight * arm_swing;
            dx -= arm_r_weight * arm_swing;

            // Piernas
            dx += leg_l_weight * left_leg_dx;
            dy += leg_l_weight * left_leg_dy;

            dx += leg_r_weight * right_leg_dx;
            dy += leg_r_weight * right_leg_dy;

            let sx = x_f - dx;
            let sy = y_f - dy;

            let p = sample_bilinear(src, sx, sy);
            if p[3] > 0 {
                out.put_pixel(px, py, p);
            }
        }
    }
    out
}

/// Dibuja notas musicales o estrellas transparentes para los estados felices o musicales
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

/// Dibuja un snack / trozo de carne en un hueso
fn draw_meat_snack(img: &mut RgbaImage, x0: i32, y0: i32, bite_ratio: f32) {
    if bite_ratio >= 1.0 {
        return;
    }
    // Hueso blanco central
    for dx in -10..=10 {
        for dy in -2..=2 {
            let px = x0 + dx;
            let py = y0 + dy;
            if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
                img.put_pixel(px as u32, py as u32, Rgba([245, 245, 245, 255]));
            }
        }
    }
    // Carne asada dorada / rojiza
    let meat_w = ((14.0 * (1.0 - bite_ratio * 0.7)).round() as i32).max(2);
    for dx in -meat_w..=meat_w {
        for dy in -7..=7 {
            let px = x0 + dx;
            let py = y0 + dy;
            if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
                let d2 = ((dx * dx) as f32 / (meat_w * meat_w) as f32) + ((dy * dy) as f32 / 49.0);
                if d2 <= 1.0 {
                    let c = if (dx + dy) % 4 == 0 {
                        Rgba([180, 70, 30, 255]) // Marca tostada
                    } else {
                        Rgba([215, 95, 45, 255]) // Carne apetitosa
                    };
                    img.put_pixel(px as u32, py as u32, c);
                }
            }
        }
    }
}

/// Dibuja la letra Z para el estado de sueño
fn draw_z(img: &mut RgbaImage, x0: i32, y0: i32, size: i32, color: Rgba<u8>) {
    for i in 0..size {
        // Línea superior
        let px1 = x0 + i;
        let py1 = y0;
        if px1 >= 0 && px1 < FW as i32 && py1 >= 0 && py1 < FH as i32 {
            img.put_pixel(px1 as u32, py1 as u32, color);
        }
        // Diagonal
        let px2 = x0 + size - 1 - i;
        let py2 = y0 + i;
        if px2 >= 0 && px2 < FW as i32 && py2 >= 0 && py2 < FH as i32 {
            img.put_pixel(px2 as u32, py2 as u32, color);
        }
        // Línea inferior
        let px3 = x0 + i;
        let py3 = y0 + size - 1;
        if px3 >= 0 && px3 < FW as i32 && py3 >= 0 && py3 < FH as i32 {
            img.put_pixel(px3 as u32, py3 as u32, color);
        }
    }
}

fn main() {
    let source_path = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e/.user_uploaded/media_1788629384792.png";
    println!("Cargando arte de Gabumon desde: {source_path}");
    let raw_img = image::open(source_path).expect("no se pudo abrir la imagen fuente de Gabumon");

    let base_sticker = extract_gabumon_sticker(&raw_img);

    let sheet_w = FW * COLS;
    let sheet_h = FH * ROWS;
    let mut sheet: RgbaImage = ImageBuffer::new(sheet_w, sheet_h);

    let tau = std::f32::consts::TAU;

    // 1. IDLE (Fila 1, 16 frames): Respiración viva, balanceo sutil de cuerno/orejas y parpadeo
    println!("Generando Fila 1: Idle (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let breath_scale = 1.0 + angle.sin() * 0.025;
        let breath_dy = angle.sin() * 1.5;
        let sway_dx = (angle * 0.5).cos() * 1.2;

        let mut spr = transform_sprite(&base_sticker, sway_dx, breath_dy, 1.0, breath_scale);

        // Parpadeo en los frames 10 y 11
        if i == 10 || i == 11 {
            // Línea de ojo sonriente cerrado
            for dx in -4i32..=4i32 {
                let px = 82 + dx;
                let py = 42 + (dx.abs() / 2);
                if px >= 0 && px < FW as i32 && py >= 0 && py < FH as i32 {
                    spr.put_pixel(px as u32, py as u32, Rgba([30, 20, 20, 255]));
                }
            }
        }

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, 0);
    }

    // 3. WRITING (Fila 3, 16 frames): Tamborileo enérgico con garras de derecha a izquierda
    println!("Generando Fila 3: Writing (16 frames)...");
    let mut writing_frames = Vec::new();
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let tap_bob = -(angle * 2.0).sin().abs() * 3.5;
        let tap_sway = angle.sin() * 2.5;

        let mut spr = transform_sprite(&base_sticker, tap_sway, tap_bob, 1.02, 0.98);

        // Chispas musicales de tecleo
        let spark_x = 64 + (angle.cos() * 30.0) as i32;
        let spark_y = 110 + (angle.sin().abs() * 8.0) as i32;
        draw_sparkle(&mut spr, spark_x, spark_y, 3, Rgba([255, 215, 0, 220]));

        writing_frames.push(spr.clone());
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (2 * FH) as i64);
    }

    // 2. START_WRITING (Fila 2, 8 frames): Transición suave de reposo a tecleo
    println!("Generando Fila 2: Start Writing (8 frames)...");
    let idle_pose = transform_sprite(&base_sticker, 0.0, 0.0, 1.0, 1.0);
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&idle_pose, &writing_frames[0], t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (1 * FH) as i64);
    }

    // 4. END_WRITING (Fila 4, 8 frames): Transición suave de tecleo a reposo
    println!("Generando Fila 4: End Writing (8 frames)...");
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&writing_frames[15], &idle_pose, t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (3 * FH) as i64);
    }

    // 5. SLEEP (Fila 5, 16 frames): Acurrucado descansando con Zzzz flotando
    println!("Generando Fila 5: Sleep (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let sleep_breath = 1.0 + angle.sin() * 0.015;
        let sleep_dy = 6.0 + angle.sin() * 1.0;

        let mut spr = transform_sprite(&base_sticker, 0.0, sleep_dy, 1.05, 0.90 * sleep_breath);

        // Zzzz flotando sobre el cuerno
        let z_offset_y = (phase * 22.0) as i32;
        let z_offset_x = ((phase * tau * 2.0).sin() * 4.0) as i32;
        draw_z(
            &mut spr,
            85 + z_offset_x,
            30 - z_offset_y,
            7,
            Rgba([100, 180, 255, 230]),
        );
        draw_z(
            &mut spr,
            95 + z_offset_x,
            42 - z_offset_y,
            5,
            Rgba([140, 200, 255, 190]),
        );

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (4 * FH) as i64);
    }

    // 6. HAPPY (Fila 6, 16 frames): Salto de victoria alegre
    println!("Generando Fila 6: Happy (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let jump_y = -(angle.sin().max(0.0) * 8.0);
        let happy_sway = (angle * 2.0).sin() * 2.0;

        let mut spr = transform_sprite(&base_sticker, happy_sway, jump_y, 0.98, 1.04);
        let star_x = 64 + ((i as i32 * 17) % 80 - 40);
        let star_y = 25 + ((i as i32 * 13) % 40);
        draw_sparkle(&mut spr, star_x, star_y, 4, Rgba([255, 230, 80, 240]));

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (5 * FH) as i64);
    }

    // 7. BORING (Fila 7, 8 frames): Bostezando y descansando sentado
    println!("Generando Fila 7: Boring (8 frames)...");
    for i in 0..8 {
        let phase = i as f32 / 8.0;
        let angle = phase * tau;
        let yawn_scale = 1.0 + angle.sin() * 0.02;
        let spr = transform_sprite(&base_sticker, 0.0, 3.0, 1.02, 0.96 * yawn_scale);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (6 * FH) as i64);
    }

    // 8-15. LOOK_* (Filas 8-15, 4 frames c/u): Mirada direccional hacia el ratón
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
            let spr = transform_sprite(&base_sticker, lx, ly + breath, 1.0, 1.0);
            image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (row * FH) as i64);
        }
    }

    // 16. WAKE_UP (Fila 16, 8 frames): Despertar alegre estirándose
    println!("Generando Fila 16: Wake Up (8 frames)...");
    let sleep_pose = transform_sprite(&base_sticker, 0.0, 6.0, 1.05, 0.90);
    for i in 0..8 {
        let t = i as f32 / 7.0;
        let spr = blend_sprites(&sleep_pose, &idle_pose, t);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (15 * FH) as i64);
    }

    // 17. WALK (Fila 17, 16 frames): Caminata continua articulada con zancada fluida
    println!("Generando Fila 17: Walk (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let spr = walk_gabumon_sprite(&base_sticker, phase);
        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (16 * FH) as i64);
    }

    // 18. EAT_RAM / EAT_SNACK (Fila 18, 16 frames): Comiendo snack de carne crujiente con mejillas infladas
    println!("Generando Fila 18: Eat Snack (16 frames)...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let angle = phase * tau;
        let chew = (angle * 3.0).sin().abs();
        let chew_scale_x = 1.0 + chew * 0.05; // Mejillas infladas al masticar
        let chew_scale_y = 1.0 - chew * 0.03;
        let mut spr = transform_sprite(&base_sticker, 0.0, 0.0, chew_scale_x, chew_scale_y);

        let snack_x = 56;
        let snack_y = 80;
        let bite_progress = (phase * 1.5).min(1.0);
        draw_meat_snack(&mut spr, snack_x, snack_y, bite_progress);

        image::imageops::overlay(&mut sheet, &spr, (i * FW) as i64, (17 * FH) as i64);
    }

    // Guardar themes/gabumon/sheet.png
    let out_dir = Path::new("themes/gabumon");
    std::fs::create_dir_all(out_dir).expect("no se pudo crear directorio themes/gabumon");

    let sheet_path = out_dir.join("sheet.png");
    sheet
        .save(&sheet_path)
        .expect("no se pudo guardar sheet.png");
    println!("Guardado exitosamente: {}", sheet_path.display());

    // Generar themes/gabumon/writing.apng
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
    println!("¡Generación del vPet Gabumon completada con éxito!");
}
