//! Procesa y ensambla el sprite sheet HD de Hatsune Miku en el estilo sticker chibi
//! con borde blanco limpio (die-cut sticker) y animaciones relajadas, suaves y naturales
//! (`themes/miku/sheet.png` y `writing.apng`).

use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::path::Path;

const FW: u32 = 128;
const FH: u32 = 128;
const COLS: u32 = 16;
const ROWS: u32 = 18;

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

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Genera un fotograma del ciclo de caminata articulando piernas, balanceo del cuerpo y coletas
/// mediante deformación continua con pesos suaves (smoothstep) que previene cualquier corte en el pelo,
/// la cintura o duplicación de piernas.
fn walk_stride_sprite(src: &RgbaImage, phase: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let tau = std::f32::consts::TAU;
    let angle = phase * tau;

    // 1. Dinámica global de caminata:
    // Rebote rítmico hacia abajo en cada paso (2 rebotes por ciclo de 2 pasos)
    let body_bob = -(angle * 2.0).sin().abs() * 3.0;
    // Inclinación suave del cuerpo al transferir el peso
    let body_sway = angle.sin() * 1.5;

    // 2. Movimiento de zancada de las piernas
    let stride_amp = 3.8f32;
    let lift_amp = 3.0f32;

    let left_leg_dx = angle.sin() * stride_amp;
    let left_leg_dy = -angle.sin().max(0.0) * lift_amp;

    let right_leg_dx = -angle.sin() * stride_amp;
    let right_leg_dy = -(-angle.sin()).max(0.0) * lift_amp;

    // 3. Balanceo inercial continuo de las coletas (flotando elásticamente)
    let hair_sway = (angle + 0.6).sin() * 4.2;
    let hair_lift = -(angle * 2.0 + 0.3).cos() * 2.0;

    for py in 0..FH {
        let y_f = py as f32;
        // Peso vertical para zona de piernas: 0 arriba en la cintura (y < 92), transición suave a 1 en los pies
        let leg_mask_y = smoothstep(92.0, 114.0, y_f);

        // Peso vertical para coletas: flotan desde los clips hacia las puntas
        let hair_mask_y = smoothstep(42.0, 112.0, y_f);

        for px in 0..FW {
            let x_f = px as f32;

            // Coleta izquierda: sólo en el lado izquierdo (x < 50), 0 en el cuerpo/piernas
            let hair_l_weight = smoothstep(56.0, 36.0, x_f) * hair_mask_y;
            // Coleta derecha: sólo en el lado derecho (x > 78), 0 en el cuerpo/piernas
            let hair_r_weight = smoothstep(72.0, 92.0, x_f) * hair_mask_y;

            // Pierna izquierda: centrada en x ≈ 56, 0 en el pelo lateral y 0 en la pierna derecha
            let leg_l_weight =
                smoothstep(46.0, 54.0, x_f) * smoothstep(65.0, 58.0, x_f) * leg_mask_y;
            // Pierna derecha: centrada en x ≈ 72, 0 en el pelo lateral y 0 en la pierna izquierda
            let leg_r_weight =
                smoothstep(63.0, 70.0, x_f) * smoothstep(82.0, 74.0, x_f) * leg_mask_y;

            // Desplazamiento compuesto totalmente continuo:
            let mut dx = body_sway;
            let mut dy = body_bob;

            // Balanceo de coletas en los laterales sin afectar a las piernas
            dx += hair_l_weight * hair_sway;
            dy += hair_l_weight * hair_lift;

            dx += hair_r_weight * hair_sway;
            dy += hair_r_weight * hair_lift;

            // Movimiento de pierna izquierda
            dx += leg_l_weight * left_leg_dx;
            dy += leg_l_weight * left_leg_dy;

            // Movimiento de pierna derecha
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

/// Mezcla e interpola suavemente dos sprites con suavizado smoothstep para transiciones fluidas.
fn blend_sprites(a: &RgbaImage, b: &RgbaImage, t: f32) -> RgbaImage {
    let mut out = ImageBuffer::new(FW, FH);
    let t = t.clamp(0.0, 1.0);
    let ease = t * t * (3.0 - 2.0 * t); // Curva cúbica smoothstep
    for y in 0..FH {
        for x in 0..FW {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);
            let wa = (pa[3] as f32 / 255.0) * (1.0 - ease);
            let wb = (pb[3] as f32 / 255.0) * ease;
            let w_sum = wa + wb;
            if w_sum < 0.005 {
                continue;
            }
            let r = (pa[0] as f32 * wa + pb[0] as f32 * wb) / w_sum;
            let g = (pa[1] as f32 * wa + pb[1] as f32 * wb) / w_sum;
            let b_col = (pa[2] as f32 * wa + pb[2] as f32 * wb) / w_sum;
            let alpha = (w_sum * 255.0).clamp(0.0, 255.0) as u8;
            out.put_pixel(
                x,
                y,
                Rgba([
                    r.round().clamp(0.0, 255.0) as u8,
                    g.round().clamp(0.0, 255.0) as u8,
                    b_col.round().clamp(0.0, 255.0) as u8,
                    alpha,
                ]),
            );
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

    // Extrae y procesa los 16 sprites individuales con silueta limpia y borde blanco puro
    // Fila 0: Expresiones base de pie
    let s_idle_open = extract_clean_miku_sticker(&raw_img, 0, 0, 4, 4); // (0,0) Ojos abiertos mirando al frente
    let s_idle_smile = extract_clean_miku_sticker(&raw_img, 1, 0, 4, 4); // (1,0) Ojos cerrados sonriendo tierna
    let s_idle_wink = extract_clean_miku_sticker(&raw_img, 2, 0, 4, 4); // (2,0) Guiño de ojo coqueto
    let s_idle_sigh = extract_clean_miku_sticker(&raw_img, 3, 0, 4, 4); // (3,0) Ojos cerrados suspirando / relajada

    // Fila 1: Actividades / Manos
    let _s_act_pad = extract_clean_miku_sticker(&raw_img, 0, 1, 4, 4); // (0,1) Libreta y lápiz
    let _s_act_book = extract_clean_miku_sticker(&raw_img, 1, 1, 4, 4); // (1,1) Leyendo libro
    let s_act_synth = extract_clean_miku_sticker(&raw_img, 2, 1, 4, 4); // (2,1) Tocando sintetizador
    let s_act_pen = extract_clean_miku_sticker(&raw_img, 3, 1, 4, 4); // (3,1) De pie con lápiz y guiño alegre

    // Fila 2: Descanso y sueño
    let s_sleep_flat = extract_clean_miku_sticker(&raw_img, 0, 2, 4, 4); // (0,2) Almohada acostada
    let s_sleep_snug = extract_clean_miku_sticker(&raw_img, 1, 2, 4, 4); // (1,2) Almohada durmiendo profundo
    let s_sleep_zzz = extract_clean_miku_sticker(&raw_img, 2, 2, 4, 4); // (2,2) Almohada roncando
    let s_sit_curl = extract_clean_miku_sticker(&raw_img, 3, 2, 4, 4); // (3,2) Sentada abrazando rodillas

    // Fila 3: Acciones especiales y golosinas
    let s_happy_negi = extract_clean_miku_sticker(&raw_img, 0, 3, 4, 4); // (0,3) Bailando agitando puerro negi
    let s_ram_hold = extract_clean_miku_sticker(&raw_img, 1, 3, 4, 4); // (1,3) Sosteniendo módulo RAM
    let s_ram_bite = extract_clean_miku_sticker(&raw_img, 2, 3, 4, 4); // (2,3) Mordisco "nom nom" a la RAM
    let s_both_trophy = extract_clean_miku_sticker(&raw_img, 3, 3, 4, 4); // (3,3) Puerro + RAM triunfante

    let sheet_w = COLS * FW;
    let sheet_h = ROWS * FH;
    let mut final_sheet = RgbaImage::new(sheet_w, sheet_h);

    let mut place = |col: u32, row: u32, sprite: &RgbaImage| {
        let ox = col * FW;
        let oy = row * FH;
        image::imageops::overlay(&mut final_sheet, sprite, ox as i64, oy as i64);
    };

    let tau = std::f32::consts::TAU;

    // ── Fila 0 (1 en ini): Idle (16 frames: parpadeo suave, guiño coqueto, respiración viva y coletas) ──
    for col in 0..16 {
        let phase = (col as f32 / 16.0) * tau;
        let dy = -phase.sin() * 2.8;
        let scale_y = 1.0 + phase.sin() * 0.025;
        let hair_dx = (phase + 0.8).cos() * 3.5;

        let base_sprite = match col {
            0..=2 => &s_idle_open,
            3 => &blend_sprites(&s_idle_open, &s_idle_smile, 0.6),
            4 => &s_idle_smile, // Parpadeo tierno con ojos cerrados
            5 => &blend_sprites(&s_idle_smile, &s_idle_open, 0.6),
            6..=8 => &s_idle_open,
            9 => &blend_sprites(&s_idle_open, &s_idle_wink, 0.6),
            10 => &s_idle_wink, // Guiño coqueto precioso
            11 => &blend_sprites(&s_idle_wink, &s_idle_open, 0.6),
            12..=13 => &s_idle_open,
            14 => &s_idle_sigh, // Suspiro relajado
            15 => &blend_sprites(&s_idle_sigh, &s_idle_open, 0.5),
            _ => &s_idle_open,
        };

        place(
            col,
            0,
            &transform_sprite(base_sprite, hair_dx * 0.4, dy, scale_y),
        );
    }

    // ── Fila 1 (2 en ini): Start Writing (8 frames: transición orgánica sacando el sintetizador) ──
    for col in 0..8 {
        let t = (col as f32 + 1.0) / 9.0;
        let blended = if t < 0.5 {
            let sub_t = t * 2.0;
            blend_sprites(&s_idle_open, &s_act_pen, sub_t)
        } else {
            let sub_t = (t - 0.5) * 2.0;
            blend_sprites(&s_act_pen, &s_act_synth, sub_t)
        };
        let bob = (t * std::f32::consts::PI).sin() * 2.0;
        place(col, 1, &transform_sprite(&blended, 0.0, -bob, 1.0));
    }

    // ── Fila 2 (3 en ini): Writing (16 frames tocando sintetizador con balanceo rítmico y notas musicales) ──
    for col in 0..16 {
        let phase = (col as f32 / 16.0) * tau;
        // Balanceo rítmico dinámico tocando teclas
        let dx = phase.sin() * 2.2;
        let dy = -(phase * 4.0).cos() * 1.8;
        let scale_y = 1.0 + (phase * 4.0).cos() * 0.02;
        let mut w = transform_sprite(&s_act_synth, dx, dy, scale_y);

        // Notas musicales turquesa ascendiendo al ritmo
        if col == 2 || col == 3 {
            draw_music_note(&mut w, 36, 30 - (col as i32 - 2) * 5, false);
        } else if col == 6 || col == 7 {
            draw_music_note(&mut w, 88, 28 - (col as i32 - 6) * 5, true);
        } else if col == 10 || col == 11 {
            draw_music_note(&mut w, 96, 24 - (col as i32 - 10) * 5, false);
        } else if col == 14 || col == 15 {
            draw_music_note(&mut w, 40, 26 - (col as i32 - 14) * 5, true);
        }

        place(col, 2, &w);
    }

    // ── Fila 3 (4 en ini): End Writing (8 frames: transición orgánica guardando el sintetizador) ──
    for col in 0..8 {
        let t = (col as f32 + 1.0) / 9.0;
        let blended = if t < 0.5 {
            let sub_t = t * 2.0;
            blend_sprites(&s_act_synth, &s_act_pen, sub_t)
        } else {
            let sub_t = (t - 0.5) * 2.0;
            blend_sprites(&s_act_pen, &s_idle_open, sub_t)
        };
        let bob = (t * std::f32::consts::PI).sin() * 1.6;
        place(col, 3, &transform_sprite(&blended, 0.0, -bob, 1.0));
    }

    // ── Fila 4 (5 en ini): Sleep (16 frames acostada durmiendo con respiración y Zzzz en olas) ──
    for col in 0..16 {
        let phase = (col as f32 / 16.0) * tau;
        let dy = -phase.sin() * 1.8;
        let scale_y = 1.0 + phase.sin() * 0.016;
        let base = if col < 5 {
            &s_sleep_flat
        } else if col < 11 {
            &s_sleep_snug
        } else {
            &s_sleep_zzz
        };
        place(col, 4, &transform_sprite(base, 0.0, dy, scale_y));
    }

    // ── Fila 5 (6 en ini): Happy (16 frames baile enérgico con puerro y RAM) ──
    for col in 0..16 {
        let phase = (col as f32 / 16.0) * tau;
        let dx = phase.sin() * 3.0;
        let dy = -phase.cos().abs() * 3.6;
        let scale_y = 1.0 + phase.cos().abs() * 0.035;

        let base = if (6..=9).contains(&col) {
            &s_both_trophy
        } else {
            &s_happy_negi
        };

        let mut h = transform_sprite(base, dx, dy, scale_y);
        if col == 3 || col == 4 {
            draw_music_note(&mut h, 104, 26 - (col as i32 - 3) * 5, false);
        } else if col == 11 || col == 12 {
            draw_music_note(&mut h, 108, 22 - (col as i32 - 11) * 5, true);
        }
        place(col, 5, &h);
    }

    // ── Fila 6 (7 en ini): Boring (8 frames sentada acurrucada abrazando rodillas) ──
    for col in 0..8 {
        let phase = (col as f32 / 8.0) * tau;
        let dy = -phase.sin() * 2.0;
        let scale_y = 1.0 + phase.sin() * 0.02;
        place(col, 6, &transform_sprite(&s_sit_curl, 0.0, dy, scale_y));
    }

    // ── Filas 7..14 (8..15 en ini): Look_* (8 direcciones con seguimiento de ojos y parpadeo) ──
    let dirs: [(f32, f32); 8] = [
        (-4.5, 0.0),  // look_left
        (4.5, 0.0),   // look_right
        (0.0, -3.5),  // look_up
        (0.0, 3.5),   // look_down
        (-3.5, -2.8), // look_up_left
        (3.5, -2.8),  // look_up_right
        (-3.5, 2.8),  // look_down_left
        (3.5, 2.8),   // look_down_right
    ];
    for (idx, (dx, dy)) in dirs.iter().enumerate() {
        let row = 7 + idx as u32;
        // Frame 0: mirando con ojos abiertos
        place(0, row, &transform_sprite(&s_idle_open, *dx, *dy, 1.0));
        // Frame 1: mirando con inclinación suave
        place(
            1,
            row,
            &transform_sprite(&s_idle_open, *dx * 0.9, *dy * 0.9, 1.005),
        );
        // Frame 2: parpadeo dulce mirando hacia la dirección
        place(
            2,
            row,
            &transform_sprite(&s_idle_smile, *dx * 0.8, *dy * 0.8, 1.0),
        );
        // Frame 3: abriendo ojos de nuevo
        place(
            3,
            row,
            &transform_sprite(&s_idle_open, *dx * 0.95, *dy * 0.95, 1.003),
        );
    }

    // ── Fila 15 (16 en ini): Wake Up (8 frames despertando desde acurrucada hasta de pie sonriente) ──
    for col in 0..8 {
        let sprite = match col {
            0..=1 => &s_sit_curl,
            2..=3 => &s_idle_sigh,
            4..=5 => &s_idle_smile,
            _ => &s_idle_open,
        };
        let t = (col as f32) / 7.0;
        let lift = (1.0 - t) * -2.0;
        place(col, 15, &transform_sprite(sprite, 0.0, lift, 1.0));
    }

    // ── Fila 16 (17 en ini): Walk (16 frames zancadas articuladas con rebote y coletas volando) ──
    for col in 0..16 {
        let phase = col as f32 / 16.0;
        let sprite = if col % 8 < 4 {
            &s_idle_open
        } else {
            &s_idle_smile
        };
        place(col, 16, &walk_stride_sprite(sprite, phase));
    }

    // ── Fila 17 (18 en ini): Eat RAM (16 frames merendando RAM: sacar, morder crujiente, masticar y celebrar) ──
    for col in 0..16 {
        let (sprite, dy) = match col {
            0..=2 => (&s_ram_hold, 0.0),
            3 => (&blend_sprites(&s_ram_hold, &s_ram_bite, 0.7), -1.0),
            4..=6 => (&s_ram_bite, -2.5), // ¡Mordisco crujiente!
            7 => (&blend_sprites(&s_ram_bite, &s_ram_hold, 0.7), -1.5),
            8..=10 => (&s_ram_hold, -0.8),     // Masticando
            11..=13 => (&s_both_trophy, -3.0), // ¡Celebración deliciosa!
            _ => (&s_idle_smile, 0.0),         // Relamiéndose satisfecha
        };
        let phase = (col as f32 / 16.0) * tau;
        let bounce = dy - (phase * 2.0).sin().abs() * 1.0;
        place(col, 17, &transform_sprite(sprite, 0.0, bounce, 1.01));
    }

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
    for col in 0..16 {
        let mut f_buf = vec![0u8; (FW * FH * 4) as usize];
        for y in 0..FH {
            for x in 0..FW {
                let src_px = col * FW + x;
                let src_py = 2 * FH + y; // Fila 2 = writing
                let p = final_sheet.get_pixel(src_px, src_py);
                let dst_i = ((y * FW + x) * 4) as usize;
                f_buf[dst_i..dst_i + 4].copy_from_slice(&p.0);
            }
        }
        writing_frames.push(f_buf);
    }
    let apng_bytes = encode_apng(FW, FH, &writing_frames, 12);
    let apng_path = out_dir.join("writing.apng");
    std::fs::write(&apng_path, &apng_bytes).expect("write writing.apng");
    println!("Guardado: {} (APNG animación)", apng_path.display());

    println!("¡Generación de Hatsune Miku Sticker HD completada con éxito!");
}
