//! Genera `themes/demo/sheet.png`: una hoja de sprites de juguete para probar
//! `theme_format = 3` (spec 0014) sin arte de terceros. Arte propio, trivial:
//! un blob con ojos que parpadea (idle), mueve las "patas" (writing) y duerme
//! (sleep). Reproducible:  `cargo run -p bongocat --example gen_demo_sheet`.

use std::path::Path;

const FW: u32 = 48;
const FH: u32 = 48;
const COLS: u32 = 4;
const ROWS: u32 = 3;

type Rgba = [u8; 4];
const BODY: Rgba = [0x6c, 0xc0, 0x4a, 0xff]; // verde
const DARK: Rgba = [0x22, 0x33, 0x1c, 0xff]; // ojos / boca
const ZZZ: Rgba = [0x9f, 0xd8, 0xf0, 0xff]; // "z" del sueño

struct Sheet {
    w: u32,
    buf: Vec<u8>,
}

impl Sheet {
    /// Pinta un píxel en el frame de rejilla `(gx, gy)`, coordenadas locales.
    fn put(&mut self, gx: u32, gy: u32, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= FW as i32 || y >= FH as i32 {
            return;
        }
        let px = gx * FW + x as u32;
        let py = gy * FH + y as u32;
        let i = ((py * self.w + px) * 4) as usize;
        self.buf[i..i + 4].copy_from_slice(&c);
    }

    fn body(&mut self, gx: u32, gy: u32) {
        for y in 0..FH as i32 {
            for x in 0..FW as i32 {
                let (dx, dy) = (x - 24, y - 26);
                if dx * dx + dy * dy <= 18 * 18 {
                    self.put(gx, gy, x, y, BODY);
                }
            }
        }
    }

    fn eyes(&mut self, gx: u32, gy: u32, open: bool) {
        for cx in [18, 30] {
            for y in 0..6 {
                for x in 0..4 {
                    if open || y == 3 {
                        self.put(gx, gy, cx + x, 22 + y, DARK);
                    }
                }
            }
        }
    }

    fn paws(&mut self, gx: u32, gy: u32, down: bool) {
        let y = if down { 40 } else { 34 };
        for x0 in [12, 30] {
            for yy in 0..6 {
                for xx in 0..8 {
                    self.put(gx, gy, x0 + xx, y + yy, BODY);
                }
            }
        }
    }
}

fn main() {
    let (w, h) = (COLS * FW, ROWS * FH);
    let mut s = Sheet {
        w,
        buf: vec![0u8; (w * h * 4) as usize],
    };

    // Fila 0 — idle: ojos abiertos / parpadeo.
    for (col, open) in [(0u32, true), (1, false)] {
        s.body(col, 0);
        s.eyes(col, 0, open);
    }
    // Fila 1 — writing: patas arriba / abajo / arriba.
    for (col, down) in [(0u32, false), (1, true), (2, false)] {
        s.body(col, 1);
        s.eyes(col, 1, true);
        s.paws(col, 1, down);
    }
    // Fila 2 — sleep: ojos cerrados + una "z" que sube.
    for (col, zy) in [(0u32, 14), (1, 8)] {
        s.body(col, 2);
        s.eyes(col, 2, false);
        for k in 0..5 {
            s.put(col, 2, 34 + k, zy, ZZZ);
            s.put(col, 2, 34, zy + k, ZZZ);
            s.put(col, 2, 34 + k, zy + 5, ZZZ);
            s.put(col, 2, 38 - k, zy + k, ZZZ);
        }
    }

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/demo");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Hoja de rejilla: idle + sleep (writing sale del APNG de abajo).
    let path = out_dir.join("sheet.png");
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&s.buf)
        .unwrap();
    println!("escrito {} ({w}x{h})", path.display());

    // APNG de `writing`: 3 marcos completos de 48×48 (patas arriba/abajo/arriba),
    // para probar el camino `SheetSource::Frames` (spec 0014 M3).
    let mut frames: Vec<Vec<u8>> = Vec::new();
    for down in [false, true, false] {
        let mut fr = Frame {
            buf: vec![0u8; (FW * FH * 4) as usize],
        };
        fr.body();
        fr.eyes();
        fr.paws(down);
        frames.push(fr.buf);
    }
    let path = out_dir.join("writing.apng");
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), FW, FH);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_animated(frames.len() as u32, 0).unwrap();
    let mut w = enc.write_header().unwrap();
    for f in &frames {
        w.set_frame_delay(1, 10).unwrap();
        w.write_image_data(f).unwrap();
    }
    w.finish().unwrap();
    println!(
        "escrito {} (APNG {}×{}, {} marcos)",
        path.display(),
        FW,
        FH,
        frames.len()
    );
}

/// Un único frame 48×48 (para el APNG de `writing`).
struct Frame {
    buf: Vec<u8>,
}

impl Frame {
    fn put(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= FW as i32 || y >= FH as i32 {
            return;
        }
        let i = ((y as u32 * FW + x as u32) * 4) as usize;
        self.buf[i..i + 4].copy_from_slice(&c);
    }
    fn body(&mut self) {
        for y in 0..FH as i32 {
            for x in 0..FW as i32 {
                let (dx, dy) = (x - 24, y - 26);
                if dx * dx + dy * dy <= 18 * 18 {
                    self.put(x, y, BODY);
                }
            }
        }
    }
    fn eyes(&mut self) {
        for cx in [18, 30] {
            for y in 0..6 {
                for x in 0..4 {
                    self.put(cx + x, 22 + y, DARK);
                }
            }
        }
    }
    fn paws(&mut self, down: bool) {
        let y = if down { 40 } else { 34 };
        for x0 in [12, 30] {
            for yy in 0..6 {
                for xx in 0..8 {
                    self.put(x0 + xx, y + yy, BODY);
                }
            }
        }
    }
}
