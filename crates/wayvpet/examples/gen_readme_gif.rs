//! Genera `assets/readme-vpets.gif` para el README: un bucle que recorre la
//! animación *idle* de los vpets de sprite sheet (`vpets/miku|umbreon|gabumon`),
//! uno tras otro, sobre fondo transparente.
//!
//!   cargo run -p wayvpet --example gen_readme_gif
//!
//! Independiente del resto del crate: solo lee los `sheet.png` + cuatro claves
//! de cada `theme.ini`. `image` es dev-dependency.

use std::fs;
use std::path::Path;

use image::codecs::gif::{GifEncoder, Repeat};
use image::imageops::{crop_imm, overlay, resize, FilterType};
use image::{Delay, Frame, RgbaImage};

/// vpets a incluir y cuántos fotogramas de *idle* tomar como máximo (los sheets
/// grandes —umbreon: 60— se recortan para que el GIF no pese de más).
const VPETS: &[(&str, u32)] = &[("miku", 24), ("umbreon", 40), ("gabumon", 24)];

/// Alto final de cada fotograma en el GIF (px). El ancho sale del aspecto.
const OUT_H: u32 = 128;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out_path = root.join("assets/readme-vpets.gif");
    fs::create_dir_all(out_path.parent().unwrap()).unwrap();

    // Lienzo común = el frame más grande entre los vpets elegidos, con margen.
    let metas: Vec<Meta> = VPETS
        .iter()
        .map(|&(name, cap)| Meta::load(&root, name, cap))
        .collect();
    let cw = metas.iter().map(|m| m.fw).max().unwrap() + 16;
    let chh = metas.iter().map(|m| m.fh).max().unwrap() + 16;
    let scale = f64::from(OUT_H) / f64::from(chh);
    let (gw, gh) = ((f64::from(cw) * scale) as u32, OUT_H);

    let file = fs::File::create(&out_path).unwrap();
    let mut enc = GifEncoder::new_with_speed(file, 10);
    enc.set_repeat(Repeat::Infinite).unwrap();

    let mut total = 0usize;
    for m in &metas {
        let delay = Delay::from_numer_denom_ms(1000 / m.fps.max(1), 1);
        for i in 0..m.frames {
            // Recorta el fotograma `i` de la fila idle y lo centra en el lienzo.
            let sub = crop_imm(&m.sheet, i * m.fw, m.row_y, m.fw, m.fh).to_image();
            let mut canvas = RgbaImage::new(cw, chh);
            let ox = i64::from((cw - m.fw) / 2);
            let oy = i64::from((chh - m.fh) / 2);
            overlay(&mut canvas, &sub, ox, oy);
            let small = resize(&canvas, gw, gh, FilterType::Lanczos3);
            enc.encode_frame(Frame::from_parts(small, 0, 0, delay))
                .unwrap();
            total += 1;
        }
    }

    drop(enc);
    let kib = fs::metadata(&out_path).map(|m| m.len() / 1024).unwrap_or(0);
    println!(
        "wayvpet: {} → {total} fotogramas, {kib} KiB",
        out_path.display()
    );
}

struct Meta {
    sheet: RgbaImage,
    fw: u32,
    fh: u32,
    row_y: u32,
    fps: u32,
    frames: u32,
}

impl Meta {
    fn load(root: &Path, name: &str, cap: u32) -> Self {
        let dir = root.join("vpets").join(name);
        let ini = fs::read_to_string(dir.join("theme.ini")).unwrap();
        let g = |k: &str, d: u32| -> u32 {
            ini.lines()
                .find_map(|l| {
                    let (lk, lv) = l.split_once('=')?;
                    (lk.trim() == k).then(|| lv.trim().parse().ok())?
                })
                .unwrap_or(d)
        };
        let (fw, fh) = (g("frame_w", 128), g("frame_h", 128));
        let row = g("state_idle_row", 1).max(1);
        let frames = g("state_idle_frames", 8).min(cap).max(1);
        let sheet = image::open(dir.join("sheet.png")).unwrap().to_rgba8();
        Meta {
            sheet,
            fw,
            fh,
            row_y: (row - 1) * fh,
            fps: g("state_idle_fps", 12),
            frames,
        }
    }
}
