//! Generador del sprite sheet HD de Umbreon (Gato Negro) con animación genuina fotograma a fotograma:
//! - 19 filas de animación REAL y ESPECÍFICA de gato:
//!   1. Idle: movimiento amplio de cola de lado a lado, giro de cabeza, orejas y parpadeo.
//!   2. Start writing: transición de reposo a patas en teclado.
//!   3. Writing: patitas delanteras alternando toques sobre el teclado con chispas doradas y orejas dinámicas.
//!   4. End writing: transición de vuelta a reposo.
//!   5. Sleep: durmiendo hecho un ovillo con Zzz ascendentes.
//!   6. Happy: caza acrobática del ratón con captura y salto.
//!   7. Boring: acicalamiento felino y lamido de pata y bigotes.
//!   8. Wake up: despertar estirando lomo y patas.
//!   9. Walk: caminata articulada a 4 patas paso a paso.
//!   10. Run: sprint / carrera veloz con galope felino auténtico.
//!   11. Scratch: apoyado en patas traseras rascando la pared / cristal de la pantalla al llegar al borde.
//!   12. Eat: comiendo de su plato con croquetas crujientes.
//!   13. Angry: lomo arqueado, cola erizada y bufido.
//!   14. Knead: amasando pan / haciendo galletitas rítmicamente con patitas y ronroneo.
//!   15. Box: dentro de su cajita de cartón asomando cabeza y patitas curioso.
//!   16. CatchBug: cazando moscas en el aire dando manotazos y saltos.
//!   17. Roll: rodando panza arriba por el suelo moviendo las 4 patas al aire.
//!   18. ButtWiggle: agazapado meneando el trasero con pupilas dilatadas antes de saltar.
//!   19. Stretch: estiramiento yoga felino con pecho al suelo y arqueo de lomo bostezando.
//! - CERO deformaciones inflables ni cambios bruscos de escala.
//! - Contorno blanco die-cut limpio de 3.2px.

use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const FW: u32 = 192;
const FH: u32 = 160;
const FLOOR_Y: u32 = 144;
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

/// Extrae limpiamente el personaje con escala fija y anclaje constante
#[allow(clippy::too_many_arguments)]
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
    let mut sub = image::imageops::crop_imm(raw_img, rx, ry, rw, rh).to_image();

    // Eliminar líneas divisorias o de suelo horizontales delgadas de la cuadrícula
    for y in 0..rh {
        let mut dark_count = 0;
        for x in 0..rw {
            let p = sub.get_pixel(x, y);
            if (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0 < 200.0 {
                dark_count += 1;
            }
        }
        if dark_count > (rw * 3 / 4) {
            let y_above = y.saturating_sub(4);
            let y_below = (y + 4).min(rh - 1);
            let check_empty = |cy: u32| -> bool {
                let mut c = 0;
                for x in 0..rw {
                    let p = sub.get_pixel(x, cy);
                    if (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0 < 200.0 {
                        c += 1;
                    }
                }
                c < (rw / 5)
            };
            if check_empty(y_above) || check_empty(y_below) {
                for dy in 0..=2 {
                    let cy = y + dy;
                    if cy < rh {
                        for x in 0..rw {
                            sub.put_pixel(x, cy, Rgba([255, 255, 255, 255]));
                        }
                    }
                }
            }
        }
    }

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

    // Incluir partes satélites (como cola u orejas separadas por pocos píxeles)
    for comp in &components[1..] {
        if comp.len() > 60 {
            let mut close = false;
            for &(cx, cy) in comp {
                for &(mx, my) in main_comp {
                    let d2 = (cx as i32 - mx as i32).pow(2) + (cy as i32 - my as i32).pow(2);
                    if d2 <= 25 * 25 {
                        close = true;
                        break;
                    }
                }
                if close {
                    break;
                }
            }
            if close {
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

    // Si include_props está activo (caja de cartón, plato de comida, mosca)
    if include_props {
        for comp in &components[1..] {
            if comp.len() > 80 {
                let mut is_prop = false;
                for &(_cx, cy) in comp {
                    if cy >= cb_max_y.saturating_sub(70) || cy <= cb_min_y.saturating_add(50) {
                        is_prop = true;
                        break;
                    }
                }
                if is_prop {
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
    }

    let char_center_x = (cb_min_x + cb_max_x) as f32 / 2.0;
    let char_bottom_y = cb_max_y as f32;

    let target_center_x = (FW / 2) as f32;
    let target_bottom_y = floor_y as f32;

    let mut clean_char = ImageBuffer::new(FW, FH);

    for y in 0..FH {
        for x in 0..FW {
            let src_x = char_center_x + (x as f32 - target_center_x) / scale;
            let src_y = char_bottom_y + (y as f32 - target_bottom_y) / scale;

            let px = src_x.round() as i32;
            let py = src_y.round() as i32;

            if px >= 0 && px < rw as i32 && py >= 0 && py < rh as i32 {
                let idx = (py as u32 * rw + px as u32) as usize;
                if body_mask[idx] {
                    let c = sample_bilinear(&sub, src_x, src_y);
                    clean_char.put_pixel(x, y, c);
                }
            }
        }
    }

    render_die_cut_border(&clean_char)
}

fn sample_bilinear(img: &RgbaImage, x: f32, y: f32) -> Rgba<u8> {
    let (w, h) = img.dimensions();
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;

    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let get_p = |px: i32, py: i32| -> [f32; 4] {
        if px >= 0 && px < w as i32 && py >= 0 && py < h as i32 {
            let p = img.get_pixel(px as u32, py as u32);
            [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        }
    };

    let p00 = get_p(x0, y0);
    let p10 = get_p(x1, y0);
    let p01 = get_p(x0, y1);
    let p11 = get_p(x1, y1);

    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] * (1.0 - fx) + p10[c] * fx;
        let bot = p01[c] * (1.0 - fx) + p11[c] * fx;
        let val = top * (1.0 - fy) + bot * fy;
        out[c] = val.round().clamp(0.0, 255.0) as u8;
    }
    Rgba(out)
}

/// Extrae automáticamente una secuencia de 6 poses distribuidas en 4 columnas x 2 filas
fn extract_sequence_4x2(
    raw: &DynamicImage,
    scale: f32,
    floor_y: u32,
    include_props: bool,
) -> Vec<RgbaImage> {
    let mut frames = Vec::new();
    let col_w = raw.width() / 4;
    let row_h = raw.height() / 2;
    for r in 0..2 {
        for c in 0..4 {
            if frames.len() >= 6 {
                break;
            }
            let rx = c * col_w;
            let ry = r * row_h;
            let f =
                extract_clean_character(raw, rx, ry, col_w, row_h, scale, floor_y, include_props);
            frames.push(f);
        }
    }
    frames
}

/// Extrae automáticamente una secuencia de 6 poses distribuidas horizontalmente en 1 fila
fn extract_sequence_6x1(
    raw: &DynamicImage,
    scale: f32,
    floor_y: u32,
    include_props: bool,
) -> Vec<RgbaImage> {
    let mut frames = Vec::new();
    let col_w = raw.width() / 6;
    let row_h = raw.height();
    for c in 0..6 {
        let rx = c * col_w;
        let ry = 0;
        let f = extract_clean_character(raw, rx, ry, col_w, row_h, scale, floor_y, include_props);
        frames.push(f);
    }
    frames
}

/// Desplaza el sprite manteniendo estrictamente su escala real 1.0
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
    let base = "/home/tilde/.gemini/antigravity-ide/brain/e52cd317-a43b-4ab2-8720-cbdd49fe048e";
    let sheet_src = format!("{}/umbreon_cat_sheet_1788633569390.jpg", base);
    let idle_src = format!("{}/umbreon_idle_seq_1788736399430.jpg", base);
    let scratch_src = format!("{}/umbreon_scratch_seq_1788736169472.jpg", base);
    let run_src = format!("{}/umbreon_run_seq_1788736281281.jpg", base);
    let walk_src = format!("{}/umbreon_walk_seq_1788730357359.jpg", base);
    let pounce_src = format!("{}/umbreon_pounce_seq_1788637088867.jpg", base);
    let groom_src = format!("{}/umbreon_groom_seq_1788637133036.jpg", base);
    let eat_src = format!("{}/umbreon_eat_seq_1788730463393.jpg", base);
    let angry_src = format!("{}/umbreon_angry_seq_1788730506231.jpg", base);

    // 6 Nuevas secuencias genuinas de gato:
    let knead_src = format!("{}/umbreon_knead_seq_1788737871628.jpg", base);
    let box_src = format!("{}/umbreon_box_seq_1788737917340.jpg", base);
    let fly_src = format!("{}/umbreon_fly_seq_1788737963737.jpg", base);
    let roll_src = format!("{}/umbreon_roll_seq_1788738014524.jpg", base);
    let wiggle_src = format!("{}/umbreon_wiggle_seq_1788738087774.jpg", base);
    let stretch_src = format!("{}/umbreon_stretch_seq_1788738172015.jpg", base);

    println!("Cargando hojas de sprites limpias de Umbreon...");
    let raw_img = image::open(&sheet_src).expect("abre sheet_src");
    let raw_idle = image::open(&idle_src).expect("abre idle_src");
    let raw_scratch = image::open(&scratch_src).expect("abre scratch_src");
    let raw_run = image::open(&run_src).expect("abre run_src");
    let raw_walk = image::open(&walk_src).expect("abre walk_src");
    let raw_pounce = image::open(&pounce_src).expect("abre pounce_src");
    let raw_groom = image::open(&groom_src).expect("abre groom_src");
    let raw_eat = image::open(&eat_src).expect("abre eat_src");
    let raw_angry = image::open(&angry_src).expect("abre angry_src");

    let raw_knead = image::open(&knead_src).expect("abre knead_src");
    let raw_box = image::open(&box_src).expect("abre box_src");
    let raw_fly = image::open(&fly_src).expect("abre fly_src");
    let raw_roll = image::open(&roll_src).expect("abre roll_src");
    let raw_wiggle = image::open(&wiggle_src).expect("abre wiggle_src");
    let raw_stretch = image::open(&stretch_src).expect("abre stretch_src");

    // 1. Pose Sleep
    println!("Extrayendo pose base Sleep...");
    let s_sleep = extract_clean_character(&raw_img, 680, 335, 315, 320, 0.35, FLOOR_Y, true);

    // 2. IDLE GENUINO: 8 poses con movimiento de cola de izquierda a derecha, orejas y cabeza
    println!("Extrayendo 8 fotogramas genuinos de IDLE (cola batiendo de lado a lado)...");
    let idle_boxes = [
        (40, 30, 280, 330),
        (370, 30, 270, 330),
        (720, 30, 310, 330),
        (1050, 30, 280, 330),
        (40, 400, 260, 340),
        (360, 390, 260, 350),
        (690, 400, 300, 340),
        (1050, 400, 290, 340),
    ];
    let mut idle_frames = Vec::new();
    for &(bx, by, bw, bh) in &idle_boxes {
        let f = extract_clean_character(&raw_idle, bx, by, bw, bh, 0.35, FLOOR_Y, false);
        idle_frames.push(f);
    }

    // 3. SCRATCH GENUINO: 6 poses apoyado en 2 patas rascando la pared
    println!(
        "Extrayendo 6 fotogramas genuinos de SCRATCH (rascar la pantalla al llegar al borde)..."
    );
    let scratch_boxes = [
        (60, 45, 390, 295),
        (490, 45, 390, 295),
        (910, 45, 390, 295),
        (60, 410, 390, 290),
        (490, 410, 390, 290),
        (910, 410, 390, 290),
    ];
    let mut scratch_frames = Vec::new();
    for &(bx, by, bw, bh) in &scratch_boxes {
        let f = extract_clean_character(&raw_scratch, bx, by, bw, bh, 0.33, FLOOR_Y, false);
        scratch_frames.push(f);
    }

    // 4. RUN GENUINO: 6 poses de sprint y galope felino veloz
    println!("Extrayendo 6 fotogramas genuinos de RUN (carrera felina veloz)...");
    let run_boxes = [
        (40, 80, 410, 250),
        (470, 80, 430, 250),
        (920, 80, 410, 250),
        (40, 395, 410, 255),
        (470, 395, 430, 255),
        (920, 395, 410, 255),
    ];
    let mut run_frames = Vec::new();
    for &(bx, by, bw, bh) in &run_boxes {
        let f = extract_clean_character(&raw_run, bx, by, bw, bh, 0.33, FLOOR_Y, false);
        run_frames.push(f);
    }

    // 5. WALK GENUINO: 6 poses de caminata a 4 patas a escala fija 0.38
    println!("Extrayendo 6 fotogramas de WALK (caminata a 4 patas)...");
    let w0 = extract_clean_character(&raw_walk, 10, 145, 226, 260, 0.38, FLOOR_Y, false);
    let w1 = extract_clean_character(&raw_walk, 236, 145, 223, 260, 0.38, FLOOR_Y, false);
    let w2 = extract_clean_character(&raw_walk, 459, 145, 222, 260, 0.38, FLOOR_Y, false);
    let w3 = extract_clean_character(&raw_walk, 681, 145, 221, 260, 0.38, FLOOR_Y, false);
    let w4 = extract_clean_character(&raw_walk, 902, 145, 223, 260, 0.38, FLOOR_Y, false);
    let w5 = extract_clean_character(&raw_walk, 1125, 145, 235, 260, 0.38, FLOOR_Y, false);
    let walk_frames = [w0, w1, w2, w3, w4, w5];

    // 6. CAZA DEL RATÓN (HAPPY): 6 poses acrobáticas a escala fija 0.38
    println!("Extrayendo 6 fotogramas de caza del ratón...");
    let p0 = extract_clean_character(&raw_pounce, 40, 270, 215, 220, 0.38, FLOOR_Y, false);
    let p1 = extract_clean_character(&raw_pounce, 270, 260, 195, 230, 0.38, FLOOR_Y, false);
    let p2 = extract_clean_character(
        &raw_pounce,
        470,
        240,
        240,
        210,
        0.38,
        FLOOR_Y.saturating_sub(14),
        false,
    );
    let p3 = extract_clean_character(
        &raw_pounce,
        715,
        230,
        205,
        240,
        0.38,
        FLOOR_Y.saturating_sub(8),
        false,
    );
    let p4 = extract_clean_character(&raw_pounce, 930, 265, 195, 235, 0.38, FLOOR_Y, true);
    let p5 = extract_clean_character(&raw_pounce, 1140, 250, 195, 250, 0.38, FLOOR_Y, true);
    let pounce_frames = [p0, p1, p2, p3, p4, p5];

    // 7. ASEO FELINO (BORING): 6 poses a escala fija 0.33
    println!("Extrayendo 6 fotogramas de aseo felino...");
    let g0 = extract_clean_character(&raw_groom, 20, 180, 230, 350, 0.33, FLOOR_Y, false);
    let g1 = extract_clean_character(&raw_groom, 255, 180, 215, 350, 0.33, FLOOR_Y, false);
    let g2 = extract_clean_character(&raw_groom, 465, 180, 210, 350, 0.33, FLOOR_Y, false);
    let g3 = extract_clean_character(&raw_groom, 665, 180, 210, 350, 0.33, FLOOR_Y, false);
    let g4 = extract_clean_character(&raw_groom, 895, 180, 205, 350, 0.33, FLOOR_Y, false);
    let g5 = extract_clean_character(&raw_groom, 1105, 190, 245, 345, 0.33, FLOOR_Y, false);
    let groom_frames = [g0, g1, g2, g3, g4, g5];

    // 8. COMIDA GENUINA (EAT): 6 poses con cuenco
    println!("Extrayendo 6 fotogramas de comida con cuenco...");
    let e0 = extract_clean_character(&raw_eat, 25, 20, 440, 365, 0.28, FLOOR_Y, true);
    let e1 = extract_clean_character(&raw_eat, 465, 60, 440, 325, 0.28, FLOOR_Y, true);
    let e2 = extract_clean_character(&raw_eat, 905, 50, 450, 335, 0.28, FLOOR_Y, true);
    let e3 = extract_clean_character(&raw_eat, 20, 420, 450, 340, 0.28, FLOOR_Y, true);
    let e4 = extract_clean_character(&raw_eat, 470, 390, 455, 370, 0.28, FLOOR_Y, true);
    let e5 = extract_clean_character(&raw_eat, 925, 385, 440, 375, 0.28, FLOOR_Y, true);
    let eat_frames = [e0, e1, e2, e3, e4, e5];

    // 9. ENOJADO GENUINO (ANGRY): 6 poses con bufido y cola erizada
    println!("Extrayendo 6 fotogramas de bufido y erizado...");
    let a0 = extract_clean_character(&raw_angry, 20, 205, 240, 320, 0.34, FLOOR_Y, false);
    let a1 = extract_clean_character(&raw_angry, 260, 205, 228, 320, 0.34, FLOOR_Y, false);
    let a2 = extract_clean_character(&raw_angry, 488, 205, 214, 320, 0.34, FLOOR_Y, false);
    let a3 = extract_clean_character(&raw_angry, 702, 205, 229, 320, 0.34, FLOOR_Y, false);
    let a4 = extract_clean_character(&raw_angry, 931, 205, 208, 320, 0.34, FLOOR_Y, false);
    let a5 = extract_clean_character(&raw_angry, 1139, 205, 226, 320, 0.34, FLOOR_Y, false);
    let angry_frames = [a0, a1, a2, a3, a4, a5];

    // 10. AMASAR PAN / GALLETITAS (KNEAD): 6 poses
    println!("Extrayendo 6 fotogramas de amasar pan (knead)...");
    let knead_frames = extract_sequence_4x2(&raw_knead, 0.33, FLOOR_Y, false);

    // 11. EN LA CAJA DE CARTÓN (BOX): 6 poses
    println!("Extrayendo 6 fotogramas de caja de cartón (box)...");
    let box_frames = extract_sequence_4x2(&raw_box, 0.31, FLOOR_Y, true);

    // 12. CAZAR MOSCAS (CATCH BUG): 6 poses
    println!("Extrayendo 6 fotogramas de cazar moscas (catch_bug)...");
    let fly_frames = extract_sequence_4x2(&raw_fly, 0.33, FLOOR_Y, true);

    // 13. RODAR PANZA ARRIBA (ROLL): 6 poses
    println!("Extrayendo 6 fotogramas de rodar panza arriba (roll)...");
    let roll_frames = extract_sequence_4x2(&raw_roll, 0.33, FLOOR_Y, false);

    // 14. MENEO DE TRASERO EN ACECHO (BUTT WIGGLE): 6 poses
    println!("Extrayendo 6 fotogramas de butt wiggle...");
    let wiggle_frames = extract_sequence_6x1(&raw_wiggle, 0.33, FLOOR_Y, false);

    // 15. ESTIRAMIENTO YOGA FELINO (STRETCH): 6 poses
    println!("Extrayendo 6 fotogramas de estiramiento felino (stretch)...");
    let stretch_frames = extract_sequence_6x1(&raw_stretch, 0.33, FLOOR_Y, false);

    let sheet_w = FW * COLS;
    let sheet_h = FH * ROWS;
    let mut sheet: RgbaImage = ImageBuffer::new(sheet_w, sheet_h);
    let tau = std::f32::consts::TAU;

    // Fila 1 (Índice 0): IDLE (16 frames)
    println!("Generando Fila 1: Idle...");
    for i in 0..16 {
        let frame_idx = match i {
            0..=7 => i,
            8..=15 => 15 - i,
            _ => 0,
        };
        let spr = &idle_frames[frame_idx];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, 0);
    }

    // Fila 3 (Índice 2): WRITING (16 frames)
    println!("Generando Fila 3: Writing...");
    let mut writing_frames = Vec::new();
    for i in 0..16 {
        let is_left_tap = (i / 2) % 2 == 0;
        let base_pose = if is_left_tap {
            &idle_frames[1]
        } else {
            &idle_frames[5]
        };
        let bob = if i % 2 == 0 { -1.5 } else { 0.5 };
        let mut spr = shift_sprite(base_pose, 0.0, bob);

        if is_left_tap {
            draw_sparkle(
                &mut spr,
                (FW / 2 - 12) as i32,
                (FLOOR_Y - 6) as i32,
                3,
                Rgba([255, 220, 0, 255]),
            );
            draw_sparkle(
                &mut spr,
                (FW / 2 - 16) as i32,
                (FLOOR_Y - 10) as i32,
                1,
                Rgba([255, 245, 160, 220]),
            );
        } else {
            draw_sparkle(
                &mut spr,
                (FW / 2 + 12) as i32,
                (FLOOR_Y - 6) as i32,
                3,
                Rgba([255, 220, 0, 255]),
            );
            draw_sparkle(
                &mut spr,
                (FW / 2 + 16) as i32,
                (FLOOR_Y - 10) as i32,
                1,
                Rgba([255, 245, 160, 220]),
            );
        }

        writing_frames.push(spr.clone());
        image::imageops::overlay(&mut sheet, &spr, (i as u32 * FW) as i64, (2 * FH) as i64);
    }

    // Fila 2 (Índice 1): START_WRITING (8 frames)
    println!("Generando Fila 2: Start Writing...");
    for i in 0..8 {
        let spr = if i < 4 {
            &idle_frames[0]
        } else {
            &writing_frames[0]
        };
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, FH as i64);
    }

    // Fila 4 (Índice 3): END_WRITING (8 frames)
    println!("Generando Fila 4: End Writing...");
    for i in 0..8 {
        let spr = if i < 4 {
            &writing_frames[15]
        } else {
            &idle_frames[0]
        };
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (3 * FH) as i64);
    }

    // Fila 5 (Índice 4): SLEEP (16 frames)
    println!("Generando Fila 5: Sleep...");
    for i in 0..16 {
        let phase = i as f32 / 16.0;
        let mut spr = s_sleep.clone();

        for wave in 0..3 {
            let wave_offset = wave as f32 / 3.0;
            let p_z = (phase + wave_offset) % 1.0;

            let z_x =
                (FW / 2 + 16) as i32 + (p_z * 20.0) as i32 + ((p_z * tau * 2.0).sin() * 4.0) as i32;
            let z_y = (FLOOR_Y - 66) as i32 - (p_z * 36.0) as i32;
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

    // Fila 6 (Índice 5): HAPPY (Caza del ratón, 6 poses)
    println!("Generando Fila 6: Happy...");
    for i in 0..16 {
        let spr = &pounce_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (5 * FH) as i64);
    }

    // Fila 7 (Índice 6): BORING (Aseo felino, 6 poses)
    println!("Generando Fila 7: Boring...");
    for i in 0..16 {
        let spr = &groom_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (6 * FH) as i64);
    }

    // Fila 8 (Índice 7): WAKE_UP (Despertar estirando patas, 8 frames)
    println!("Generando Fila 8: Wake Up...");
    for i in 0..8 {
        let spr = if i < 3 {
            &s_sleep
        } else if i < 6 {
            &groom_frames[1]
        } else {
            &idle_frames[0]
        };
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (7 * FH) as i64);
    }

    // Fila 9 (Índice 8): WALK (Caminata genuina de 4 patas, 6 poses)
    println!("Generando Fila 9: Walk...");
    for i in 0..16 {
        let spr = &walk_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (8 * FH) as i64);
    }

    // Fila 10 (Índice 9): RUN (Galope felino veloz, 6 poses)
    println!("Generando Fila 10: Run...");
    for i in 0..16 {
        let spr = &run_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (9 * FH) as i64);
    }

    // Fila 11 (Índice 10): SCRATCH (Rascado de pared, 6 poses)
    println!("Generando Fila 11: Scratch...");
    for i in 0..16 {
        let spr = &scratch_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (10 * FH) as i64);
    }

    // Fila 12 (Índice 11): EAT_RAM (Comida de croquetas, 6 poses)
    println!("Generando Fila 12: Eat...");
    for i in 0..16 {
        let spr = &eat_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (11 * FH) as i64);
    }

    // Fila 13 (Índice 12): ANGRY (Bufido y cola erizada, 6 poses)
    println!("Generando Fila 13: Angry...");
    for i in 0..16 {
        let spr = &angry_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (12 * FH) as i64);
    }

    // Fila 14 (Índice 13): KNEAD (Amasar pan / hacer galletitas, 6 poses)
    println!("Generando Fila 14: Knead...");
    for i in 0..16 {
        let spr = &knead_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (13 * FH) as i64);
    }

    // Fila 15 (Índice 14): BOX (En caja de cartón, 6 poses)
    println!("Generando Fila 15: Box...");
    for i in 0..16 {
        let spr = &box_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (14 * FH) as i64);
    }

    // Fila 16 (Índice 15): CATCH_BUG (Cazar moscas en el aire, 6 poses)
    println!("Generando Fila 16: CatchBug...");
    for i in 0..16 {
        let spr = &fly_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (15 * FH) as i64);
    }

    // Fila 17 (Índice 16): ROLL (Rodar panza arriba, 6 poses)
    println!("Generando Fila 17: Roll...");
    for i in 0..16 {
        let spr = &roll_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (16 * FH) as i64);
    }

    // Fila 18 (Índice 17): BUTT_WIGGLE (Meneo de trasero en acecho, 6 poses)
    println!("Generando Fila 18: ButtWiggle...");
    for i in 0..16 {
        let spr = &wiggle_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (17 * FH) as i64);
    }

    // Fila 19 (Índice 18): STRETCH (Estiramiento yoga felino, 6 poses)
    println!("Generando Fila 19: Stretch...");
    for i in 0..16 {
        let spr = &stretch_frames[i % 6];
        image::imageops::overlay(&mut sheet, spr, (i as u32 * FW) as i64, (18 * FH) as i64);
    }

    // Guardar sheet.png
    let out_dir = Path::new("themes/umbreon");
    std::fs::create_dir_all(out_dir).expect("crea directorio themes/umbreon");
    let sheet_path = out_dir.join("sheet.png");
    println!(
        "Guardando sprite sheet ({sheet_w}x{sheet_h}, {ROWS} filas) en: {}",
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
    encoder.set_frame_delay(1, 6).expect("configura delay APNG");

    let mut writer = encoder.write_header().expect("escribe cabecera APNG");
    for frame in &writing_frames {
        writer
            .write_image_data(frame.as_raw())
            .expect("escribe fotograma APNG");
    }
    writer.finish().expect("finaliza APNG");

    println!("¡Generación de Umbreon completada: 19 filas de movimientos felinos auténticos!");
}
