//! Procesa y ensambla el sprite sheet HD de Hatsune Miku en el estilo sticker chibi
//! con borde blanco limpio (die-cut sticker) y animaciones relajadas, suaves y naturales
//! (`themes/miku/sheet.png` y `writing.apng`).

use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::path::Path;

const FW: u32 = 128;
const FH: u32 = 128;
const COLS: u32 = 6;
const ROWS: u32 = 16;

/// Extrae limpiamente el personaje Miku de una celda de la cuadrícula eliminando sombras y ruido exterior,
/// y genera un borde de sticker blanco puro (#FFFFFF) de 4.5px de grosor sin ningún artefacto oscuro.
fn extract_clean_miku_sticker(
    raw_img: &DynamicImage,
    col: u32,
    row: u32,
    total_cols: u32,
    total_rows: u32,
) -> RgbaImage {
    let (iw, ih) = raw_img.dimensions();
    let cell_w = iw / total_cols;
    let cell_h = ih / total_rows;

    let x0 = col * cell_w;
    let y0 = row * cell_h;

    let sub = image::imageops::crop_imm(raw_img, x0, y0, cell_w, cell_h).to_image();

    // 1. Identificar características del personaje (color saturado o líneas de dibujo oscuras)
    let mut is_feature = vec![false; (cell_w * cell_h) as usize];
    for y in 0..cell_h {
        for x in 0..cell_w {
            let p = sub.get_pixel(x, y);
            let r = p[0] as f32;
            let g = p[1] as f32;
            let b = p[2] as f32;
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);

            // Color del pelo (cyan/turquesa), piel, clips rojos, ram verde, o líneas negras interiores
            let has_color = (max_c - min_c) >= 15.0;
            let is_dark_line = max_c < 110.0;

            if has_color || is_dark_line {
                is_feature[(y * cell_w + x) as usize] = true;
            }
        }
    }

    // 2. Encontrar el componente conectado principal del personaje (el más cercano al centro)
    let cx = cell_w as i32 / 2;
    let cy = cell_h as i32 / 2;
    let mut visited_feat = vec![false; (cell_w * cell_h) as usize];
    let mut components: Vec<Vec<(u32, u32)>> = Vec::new();

    for y in 0..cell_h {
        for x in 0..cell_w {
            let idx = (y * cell_w + x) as usize;
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
                        if nx >= 0 && nx < cell_w as i32 && ny >= 0 && ny < cell_h as i32 {
                            let nidx = (ny as u32 * cell_w + nx as u32) as usize;
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

    // El componente principal del personaje es el que tiene mayor masa cerca del centro
    components.sort_by_key(|c| {
        let size = c.len() as i32;
        let avg_x = c.iter().map(|&(x, _)| x as i32).sum::<i32>() / size.max(1);
        let avg_y = c.iter().map(|&(_, y)| y as i32).sum::<i32>() / size.max(1);
        let dist_center = (avg_x - cx).pow(2) + (avg_y - cy).pow(2);
        // Puntuación: mayor tamaño y menor distancia al centro
        -(size * 1000 - dist_center)
    });

    let main_comp = &components[0];
    let mut char_mask = vec![false; (cell_w * cell_h) as usize];
    for &(x, y) in main_comp {
        char_mask[(y * cell_w + x) as usize] = true;
    }

    // Incluir también componentes secundarios cercanos (accesorios, ram, clips que estén a < 15px)
    for comp in &components[1..] {
        let is_near = comp.iter().any(|&(x, y)| {
            main_comp.iter().any(|&(mx, my)| {
                let dx = (x as i32 - mx as i32).abs();
                let dy = (y as i32 - my as i32).abs();
                dx * dx + dy * dy <= 225 // dentro de 15px
            })
        });
        if is_near && comp.len() > 10 {
            for &(x, y) in comp {
                char_mask[(y * cell_w + x) as usize] = true;
            }
        }
    }

    // 3. Rellenar huecos interiores (ojos, camisa blanca, lazos, brillos) mediante flood-fill desde los bordes exteriores
    let mut outside = vec![false; (cell_w * cell_h) as usize];
    let mut q_out = VecDeque::new();

    for x in 0..cell_w {
        q_out.push_back((x, 0));
        q_out.push_back((x, cell_h - 1));
    }
    for y in 0..cell_h {
        q_out.push_back((0, y));
        q_out.push_back((cell_w - 1, y));
    }

    while let Some((x, y)) = q_out.pop_front() {
        let idx = (y * cell_w + x) as usize;
        if outside[idx] {
            continue;
        }
        outside[idx] = true;

        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx >= 0 && nx < cell_w as i32 && ny >= 0 && ny < cell_h as i32 {
                let nidx = (ny as u32 * cell_w + nx as u32) as usize;
                if !outside[nidx] && !char_mask[nidx] {
                    q_out.push_back((nx as u32, ny as u32));
                }
            }
        }
    }

    // Toda la silueta del personaje es lo que no fue alcanzado desde el exterior
    let mut body_mask = vec![false; (cell_w * cell_h) as usize];
    let mut min_x = cell_w;
    let mut max_x = 0;
    let mut min_y = cell_h;
    let mut max_y = 0;

    for y in 0..cell_h {
        for x in 0..cell_w {
            let idx = (y * cell_w + x) as usize;
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

    // 4. Crear imagen aislada del personaje con transparencia perfecta (sin fondo ni sombras)
    let bw = max_x - min_x + 1;
    let bh = max_y - min_y + 1;
    let mut char_img: RgbaImage = ImageBuffer::new(bw, bh);

    for y in 0..bh {
        for x in 0..bw {
            let sx = min_x + x;
            let sy = min_y + y;
            let idx = (sy * cell_w + sx) as usize;
            if body_mask[idx] {
                let p = sub.get_pixel(sx, sy);
                char_img.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
            } else {
                char_img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }

    // 5. Escalar el personaje para que quepa en FW x FH dejando espacio para el borde blanco del sticker
    let target_w = FW - 14;
    let target_h = FH - 14;
    let scale = (target_w as f32 / bw as f32).min(target_h as f32 / bh as f32);
    let nw = ((bw as f32 * scale).round() as u32).max(1);
    let nh = ((bh as f32 * scale).round() as u32).max(1);

    let resized_char =
        image::imageops::resize(&char_img, nw, nh, image::imageops::FilterType::Lanczos3);

    // Centrar horizontalmente y alinear hacia abajo
    let ox = (FW - nw) / 2;
    let oy = FH.saturating_sub(nh + 5);

    let mut char_canvas: RgbaImage = ImageBuffer::new(FW, FH);
    image::imageops::overlay(&mut char_canvas, &resized_char, ox as i64, oy as i64);

    // 6. Generar el borde blanco puro de sticker (#FFFFFF) mediante distancia Euclidiana
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
                // Blanco 100% puro para el borde del sticker
                out.put_pixel(x, y, Rgba([255, 255, 255, a_byte]));
            }
        }
    }

    // Superponer el personaje original nítido sobre el borde blanco puro
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

/// Muestrea de forma bilineal y premultiplicada una imagen RGBA.
/// Garantiza transiciones de subpíxel suaves y NUNCA produce huecos ni renglones vacíos.
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

/// Transforma suavemente un sprite aplicando traslación y respiración vertical mediante
/// **muestreo inverso bilineal**. Garantiza cobertura del 100% de píxeles sin saltarse
/// renglones, eliminando cualquier línea horizontal o hueco transparente.
fn transform_sprite(src: &RgbaImage, dx: f32, dy: f32, scale_y: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let base_y = FH as f32 - 5.0;

    for py in 0..FH {
        let sy = (py as f32 - dy - base_y) / scale_y + base_y;
        for px in 0..FW {
            let sx = px as f32 - dx;
            let p = sample_bilinear(src, sx, sy);
            if p[3] > 0 {
                out.put_pixel(px, py, p);
            }
        }
    }
    out
}

/// Genera un fotograma del ciclo de caminata articulando piernas, balanceo del cuerpo y coletas
/// sin saltarse jamás píxeles, mediante muestreo inverso continuo.
fn walk_stride_sprite(src: &RgbaImage, phase: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let tau = std::f32::consts::TAU;
    let angle = phase * tau;

    // Movimiento cíclico de piernas: paso adelante/atrás y elevación de rodilla
    let stride_amp = 3.2f32;
    let lift_amp = 2.4f32;

    let left_dx = angle.sin() * stride_amp;
    let left_dy = -angle.sin().max(0.0) * lift_amp;

    let right_dx = -angle.sin() * stride_amp;
    let right_dy = -(-angle.sin()).max(0.0) * lift_amp;

    // Oscilación y rebote del cuerpo (2 rebotes por ciclo de dos pasos)
    let body_bob = (angle * 2.0).sin().abs() * 1.6;
    let body_tilt = angle.sin() * 1.0;
    let hair_sway = (angle + 0.5).sin() * 2.2;

    for py in 0..FH {
        for px in 0..FW {
            let (sx, sy) = if py >= 94 {
                // Zona de piernas/pies
                if px < 63 {
                    (px as f32 - left_dx, py as f32 - left_dy)
                } else {
                    (px as f32 - right_dx, py as f32 - right_dy)
                }
            } else if px < 46 {
                // Coleta izquierda (balanceo con inercia)
                (px as f32 - hair_sway, py as f32 + body_bob)
            } else if px > 82 {
                // Coleta derecha (balanceo con inercia)
                (px as f32 - hair_sway, py as f32 + body_bob)
            } else {
                // Cabeza y torso
                (px as f32 - body_tilt, py as f32 + body_bob)
            };

            let p = sample_bilinear(src, sx, sy);
            if p[3] > 0 {
                out.put_pixel(px, py, p);
            }
        }
    }
    out
}

/// Dibuja una nota musical pequeña y estilizada con borde blanco (#FFFFFF)
fn draw_music_note(dst: &mut RgbaImage, cx: i32, cy: i32, is_double: bool) {
    let note_color = Rgba([57, 197, 187, 255]); // Turquesa Miku #39C5BB
    let outline_color = Rgba([255, 255, 255, 255]); // Borde sticker blanco

    let mut points: Vec<(i32, i32)> = Vec::new();
    // Cabeza de la nota
    for dy in -2..=2 {
        for dx in -3..=3 {
            if dx * dx + dy * dy <= 7 {
                points.push((cx + dx, cy + dy));
            }
        }
    }
    // Plica vertical
    for dy in -8..=0 {
        points.push((cx + 2, cy + dy));
        points.push((cx + 3, cy + dy));
    }
    // Corchete
    if is_double {
        for dy in -2..=2 {
            for dx in -3..=3 {
                if dx * dx + dy * dy <= 7 {
                    points.push((cx + 8 + dx, cy - 2 + dy));
                }
            }
        }
        for dy in -10..=-2 {
            points.push((cx + 10, cy + dy));
            points.push((cx + 11, cy + dy));
        }
        for dx in 2..=11 {
            points.push((cx + dx, cy - 9));
            points.push((cx + dx, cy - 8));
        }
    } else {
        points.push((cx + 4, cy - 8));
        points.push((cx + 5, cy - 7));
        points.push((cx + 5, cy - 6));
    }

    // Dibujar borde blanco alrededor
    for &(x, y) in &points {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let nx = x + dx;
                let ny = y + dy;
                if nx >= 0 && nx < FW as i32 && ny >= 0 && ny < FH as i32 {
                    dst.put_pixel(nx as u32, ny as u32, outline_color);
                }
            }
        }
    }
    // Dibujar nota de color
    for &(x, y) in &points {
        if x >= 0 && x < FW as i32 && y >= 0 && y < FH as i32 {
            dst.put_pixel(x as u32, y as u32, note_color);
        }
    }
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("write_header");
    w.write_image_data(rgba).expect("write_image_data");
    drop(w);
    out
}

fn encode_apng(w: u32, h: u32, frames: &[Vec<u8>], fps: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_animated(frames.len() as u32, 0)
        .expect("set_animated");
    enc.set_frame_delay(1, fps as u16).expect("set_frame_delay");
    let mut w = enc.write_header().expect("write_header");
    for f in frames {
        w.write_image_data(f).expect("write_frame");
    }
    drop(w);
    out
}

fn main() {
    let src_path = "/home/tilde/.gemini/antigravity-ide/brain/bace10a5-1365-4937-b1cc-c297d4e091e1/miku_chibi_sprites_1788584484036.jpg";
    println!("Cargando sprite sheet sticker de: {}", src_path);

    let raw_img = image::open(src_path).expect("No se pudo abrir miku_chibi_sprites");

    // Extrae y procesa los sprites individuales con silueta limpia y borde blanco puro
    let s_idle0 = extract_clean_miku_sticker(&raw_img, 0, 0, 4, 4); // Ojos abiertos tiernos
    let s_idle_smile = extract_clean_miku_sticker(&raw_img, 1, 0, 4, 4); // Ojos cerrados sonriendo
    let s_boring = extract_clean_miku_sticker(&raw_img, 3, 0, 4, 4); // Acurrucada somnolienta

    let _s_write0 = extract_clean_miku_sticker(&raw_img, 0, 1, 4, 4);
    let s_write1 = extract_clean_miku_sticker(&raw_img, 2, 1, 4, 4); // Tocando sintetizador
    let _s_write2 = extract_clean_miku_sticker(&raw_img, 3, 1, 4, 4);

    let s_sleep0 = extract_clean_miku_sticker(&raw_img, 0, 2, 4, 4); // Acostada en almohada
    let s_sleep1 = extract_clean_miku_sticker(&raw_img, 1, 2, 4, 4);
    let s_sleep2 = extract_clean_miku_sticker(&raw_img, 2, 2, 4, 4);
    let _s_sleep3 = extract_clean_miku_sticker(&raw_img, 3, 2, 4, 4);

    let s_happy0 = extract_clean_miku_sticker(&raw_img, 0, 3, 4, 4); // Agitando puerro negi
    let _s_happy1 = extract_clean_miku_sticker(&raw_img, 3, 3, 4, 4);

    let s_ram0 = extract_clean_miku_sticker(&raw_img, 1, 3, 4, 4); // Sosteniendo módulo RAM
    let s_ram1 = extract_clean_miku_sticker(&raw_img, 2, 3, 4, 4); // Mordisco a la RAM

    let sheet_w = COLS * FW;
    let sheet_h = ROWS * FH;
    let mut final_sheet = RgbaImage::new(sheet_w, sheet_h);

    let mut place = |col: u32, row: u32, sprite: &RgbaImage| {
        let ox = col * FW;
        let oy = row * FH;
        image::imageops::overlay(&mut final_sheet, sprite, ox as i64, oy as i64);
    };

    // ── Fila 0 (1 en ini): Idle (6 frames de respiración suave continua, ojos abiertos tiernos) ───
    place(0, 0, &transform_sprite(&s_idle0, 0.0, 0.0, 1.0));
    place(1, 0, &transform_sprite(&s_idle0, 0.0, -0.7, 1.007));
    place(2, 0, &transform_sprite(&s_idle0, 0.0, -1.4, 1.014));
    place(3, 0, &transform_sprite(&s_idle0, 0.0, -2.0, 1.020));
    place(4, 0, &transform_sprite(&s_idle0, 0.0, -1.4, 1.014));
    place(5, 0, &transform_sprite(&s_idle0, 0.0, -0.7, 1.007));

    // ── Fila 1 (2 en ini): Writing (6 frames fluidos tocando el sintetizador con notas musicales) ─
    let w0 = transform_sprite(&s_write1, 0.0, 0.0, 1.0);
    let w1 = transform_sprite(&s_write1, -0.6, 0.6, 0.994);
    let mut w2 = transform_sprite(&s_write1, -0.3, -1.2, 1.012);
    let w3 = transform_sprite(&s_write1, 0.6, 0.6, 0.994);
    let mut w4 = transform_sprite(&s_write1, 0.3, -1.2, 1.012);
    let mut w5 = transform_sprite(&s_write1, 0.0, -0.5, 1.005);
    draw_music_note(&mut w2, 38, 30, false);
    draw_music_note(&mut w4, 90, 26, true);
    draw_music_note(&mut w5, 94, 20, false);

    place(0, 1, &w0);
    place(1, 1, &w1);
    place(2, 1, &w2);
    place(3, 1, &w3);
    place(4, 1, &w4);
    place(5, 1, &w5);

    // ── Fila 2 (3 en ini): Sleep (6 frames acostada durmiendo plácidamente en almohada con Zzzz) ──
    place(0, 2, &transform_sprite(&s_sleep0, 0.0, 0.0, 1.0));
    place(1, 2, &transform_sprite(&s_sleep1, 0.0, -0.6, 1.006));
    place(2, 2, &transform_sprite(&s_sleep2, 0.0, -1.2, 1.012));
    place(3, 2, &transform_sprite(&s_sleep2, 0.0, -1.5, 1.015));
    place(4, 2, &transform_sprite(&s_sleep1, 0.0, -0.9, 1.009));
    place(5, 2, &transform_sprite(&s_sleep0, 0.0, -0.3, 1.003));

    // ── Fila 3 (4 en ini): Happy (6 frames agitando el puerro negi alegremente) ──────────
    let h0 = transform_sprite(&s_happy0, -1.2, 0.0, 1.0);
    let h1 = transform_sprite(&s_happy0, -0.5, -1.5, 1.015);
    let mut h2 = transform_sprite(&s_happy0, 0.4, -2.4, 1.024);
    let mut h3 = transform_sprite(&s_happy0, 1.4, -1.8, 1.018);
    let h4 = transform_sprite(&s_happy0, 0.6, -0.8, 1.008);
    let h5 = transform_sprite(&s_happy0, -0.4, 0.0, 1.0);
    draw_music_note(&mut h2, 102, 25, false);
    draw_music_note(&mut h3, 106, 20, true);

    place(0, 3, &h0);
    place(1, 3, &h1);
    place(2, 3, &h2);
    place(3, 3, &h3);
    place(4, 3, &h4);
    place(5, 3, &h5);

    // ── Fila 4 (5 en ini): Boring (4 frames somnolienta y acurrucada con respiración lenta) ──
    place(0, 4, &transform_sprite(&s_boring, 0.0, 0.0, 1.0));
    place(1, 4, &transform_sprite(&s_boring, 0.0, -1.0, 1.010));
    place(2, 4, &transform_sprite(&s_boring, 0.0, -1.5, 1.015));
    place(3, 4, &transform_sprite(&s_boring, 0.0, -0.6, 1.006));

    // ── Filas 5..12 (6..13 en ini): Look_* (8 direcciones de mirada con ojos abiertos) ─
    let dirs: [(f32, f32); 8] = [
        (-4.0, 0.0),  // look_left
        (4.0, 0.0),   // look_right
        (0.0, -3.0),  // look_up
        (0.0, 3.0),   // look_down
        (-3.0, -2.5), // look_up_left
        (3.0, -2.5),  // look_up_right
        (-3.0, 2.5),  // look_down_left
        (3.0, 2.5),   // look_down_right
    ];
    for (idx, (dx, dy)) in dirs.iter().enumerate() {
        let row = 5 + idx as u32;
        place(0, row, &transform_sprite(&s_idle0, *dx, *dy, 1.0));
        place(
            1,
            row,
            &transform_sprite(&s_idle0, *dx * 0.85, *dy * 0.85, 1.005),
        );
    }

    // ── Fila 13 (14 en ini): Wake Up (4 frames despertando alegremente) ───────────
    place(0, 13, &transform_sprite(&s_boring, 0.0, 0.0, 1.0));
    place(1, 13, &transform_sprite(&s_boring, 0.0, -1.0, 1.01));
    place(2, 13, &transform_sprite(&s_idle_smile, 0.0, -1.5, 1.015));
    place(3, 13, &transform_sprite(&s_idle0, 0.0, 0.0, 1.0));

    // ── Fila 14 (15 en ini): Walk (6 frames ciclo de pasos articulado con balanceo natural) ──
    for col in 0..6 {
        let phase = col as f32 / 6.0;
        place(col, 14, &walk_stride_sprite(&s_idle0, phase));
    }

    // ── Fila 15 (16 en ini): Eat RAM (6 frames comiendo RAM de forma continua) ────
    place(0, 15, &transform_sprite(&s_ram0, 0.0, 0.0, 1.0));
    place(1, 15, &transform_sprite(&s_ram0, 0.0, -1.0, 1.010));
    place(2, 15, &transform_sprite(&s_ram1, 0.0, -1.6, 1.016));
    place(3, 15, &transform_sprite(&s_ram1, 0.0, -1.2, 1.012));
    place(4, 15, &transform_sprite(&s_idle_smile, 0.0, -0.6, 1.006));
    place(5, 15, &transform_sprite(&s_idle0, 0.0, 0.0, 1.0));

    let out_dir = Path::new("themes/miku");
    std::fs::create_dir_all(out_dir).expect("create_dir_all");

    let png_bytes = encode_png(sheet_w, sheet_h, final_sheet.as_raw());
    let sheet_path = out_dir.join("sheet.png");
    std::fs::write(&sheet_path, &png_bytes).expect("write sheet.png");
    println!(
        "Guardado: {} ({}x{} px)",
        sheet_path.display(),
        sheet_w,
        sheet_h
    );

    let mut writing_frames = Vec::new();
    for col in 0..6 {
        let mut f_buf = vec![0u8; (FW * FH * 4) as usize];
        for y in 0..FH {
            for x in 0..FW {
                let src_px = col * FW + x;
                let src_py = FH + y;
                let p = final_sheet.get_pixel(src_px, src_py);
                let dst_i = ((y * FW + x) * 4) as usize;
                f_buf[dst_i..dst_i + 4].copy_from_slice(&p.0);
            }
        }
        writing_frames.push(f_buf);
    }
    let apng_bytes = encode_apng(FW, FH, &writing_frames, 6);
    let apng_path = out_dir.join("writing.apng");
    std::fs::write(&apng_path, &apng_bytes).expect("write writing.apng");
    println!("Guardado: {} (APNG animación)", apng_path.display());

    println!("¡Generación de Hatsune Miku Sticker HD completada con éxito!");
}
