//! Genera `themes/demo/sheet.png`: una hoja de sprites para el vpet `demo`
//! con soporte de seguimiento de mirada con los ojos (`look_*`), parpadeo natural,
//! animación de sueño con "Zzzz" flotantes (`sleep`), aburrimiento (`boring`),
//! felicidad (`happy`) y tecleo (`writing`).
//!
//! Reproducible: `cargo run -p wayvpet --example gen_demo_sheet`.

use std::path::Path;

const FW: u32 = 48;
const FH: u32 = 48;
const COLS: u32 = 4;
const ROWS: u32 = 14;

type Rgba = [u8; 4];
const BODY: Rgba = [0x6c, 0xc0, 0x4a, 0xff]; // verde
const DARK: Rgba = [0x22, 0x33, 0x1c, 0xff]; // ojos / boca
const ZZZ: Rgba = [0x9f, 0xd8, 0xf0, 0xff]; // "z" del sueño (celeste suave)
const ZZZ_HI: Rgba = [0xd6, 0xf0, 0xff, 0xff]; // "z" brillante

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
        self.body_offset(gx, gy, 0, 0);
    }

    fn body_offset(&mut self, gx: u32, gy: u32, off_x: i32, off_y: i32) {
        for y in 0..FH as i32 {
            for x in 0..FW as i32 {
                let (dx, dy) = (x - (24 + off_x), y - (26 + off_y));
                if dx * dx + dy * dy <= 18 * 18 {
                    self.put(gx, gy, x, y, BODY);
                }
            }
        }
    }

    /// Ojos posicionados con desplazamiento (para seguimiento del ratón).
    fn eyes(&mut self, gx: u32, gy: u32, off_x: i32, off_y: i32, open: bool) {
        for cx in [18, 30] {
            for y in 0..6 {
                for x in 0..4 {
                    if open || y == 3 {
                        self.put(gx, gy, cx + off_x + x, 22 + off_y + y, DARK);
                    }
                }
            }
        }
    }

    /// Ojos cerrados plácidamente para dormir (curva suave hacia arriba).
    fn sleeping_eyes(&mut self, gx: u32, gy: u32) {
        for cx in [17, 29] {
            self.put(gx, gy, cx, 24, DARK);
            self.put(gx, gy, cx + 1, 25, DARK);
            self.put(gx, gy, cx + 2, 25, DARK);
            self.put(gx, gy, cx + 3, 25, DARK);
            self.put(gx, gy, cx + 4, 24, DARK);
        }
    }

    /// Ojos somnolientos/entornados para `boring`.
    fn droopy_eyes(&mut self, gx: u32, gy: u32) {
        for cx in [18, 30] {
            for x in 0..4 {
                self.put(gx, gy, cx + x, 24, DARK);
                self.put(gx, gy, cx + x, 25, DARK);
            }
        }
    }

    /// Dibuja una letra "Z" de tamaño `size` con color `c`.
    fn draw_z(&mut self, gx: u32, gy: u32, x: i32, y: i32, size: i32, c: Rgba) {
        for k in 0..size {
            self.put(gx, gy, x + k, y, c);
            self.put(gx, gy, x + size - 1 - k, y + k, c);
            self.put(gx, gy, x + k, y + size - 1, c);
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

    /// Boca: `w` píxeles de ancho centrada, arco simple hacia abajo (sonrisa).
    fn smile(&mut self, gx: u32, gy: u32, w: i32) {
        let cx = 24;
        for i in 0..w {
            let x = cx - w / 2 + i;
            let dip = ((i - w / 2).abs() < w / 4) as i32; // hunde el centro
            self.put(gx, gy, x, 33 + dip, DARK);
            self.put(gx, gy, x, 34 + dip, DARK);
        }
    }
}

fn main() {
    let (w, h) = (COLS * FW, ROWS * FH);
    let mut s = Sheet {
        w,
        buf: vec![0u8; (w * h * 4) as usize],
    };

    // Fila 0 (1) — idle: centro (ojos abiertos / parpadeo).
    for (col, open) in [(0u32, true), (1, false)] {
        s.body(col, 0);
        s.eyes(col, 0, 0, 0, open);
    }

    // Fila 1 (2) — writing: patas arriba / abajo / arriba.
    for (col, down) in [(0u32, false), (1, true), (2, false)] {
        s.body(col, 1);
        s.eyes(col, 1, 0, 0, true);
        s.paws(col, 1, down);
    }

    // Fila 2 (3) — sleep: 4 fotogramas con ojos cerrados y "Zzz" flotantes continuas.
    // Frame 0: z pequeña saliendo
    s.body(0, 2);
    s.sleeping_eyes(0, 2);
    s.draw_z(0, 2, 33, 17, 3, ZZZ);

    // Frame 1: z pequeña sube + Z mediana
    s.body(1, 2);
    s.sleeping_eyes(1, 2);
    s.draw_z(1, 2, 34, 12, 3, ZZZ);
    s.draw_z(1, 2, 36, 18, 4, ZZZ_HI);

    // Frame 2: z alta + Z mediana sube + Z grande abajo
    s.body(2, 2);
    s.sleeping_eyes(2, 2);
    s.draw_z(2, 2, 34, 6, 3, ZZZ);
    s.draw_z(2, 2, 37, 12, 4, ZZZ_HI);
    s.draw_z(2, 2, 32, 19, 5, ZZZ);

    // Frame 3: Z grande sube + Z mediana alta
    s.body(3, 2);
    s.sleeping_eyes(3, 2);
    s.draw_z(3, 2, 37, 7, 4, ZZZ);
    s.draw_z(3, 2, 34, 14, 5, ZZZ_HI);

    // Fila 3 (4) — happy: ojos abiertos + sonrisa que crece (KPM alto).
    for (col, w) in [(0u32, 12), (1, 18)] {
        s.body(col, 3);
        s.eyes(col, 3, 0, 0, true);
        s.smile(col, 3, w);
    }

    // Fila 4 (5) — boring: ojos entornados somnolientos, se balancea un poco.
    for (col, off) in [(0u32, -1), (1, 1)] {
        s.body_offset(col, 4, off, 0);
        s.droopy_eyes(col, 4);
        for i in 0..6 {
            s.put(col, 4, 21 + i + off, 34, DARK);
        }
    }

    // Filas 5–12: Miradas direccionales hacia el ratón (con parpadeo natural en frame 1).
    let look_directions = [
        (5u32, -2, 0), // look_left (Fila 6)
        (6, 2, 0),     // look_right (Fila 7)
        (7, 0, -2),    // look_up (Fila 8)
        (8, 0, 2),     // look_down (Fila 9)
        (9, -2, -2),   // look_up_left (Fila 10)
        (10, 2, -2),   // look_up_right (Fila 11)
        (11, -2, 2),   // look_down_left (Fila 12)
        (12, 2, 2),    // look_down_right (Fila 13)
    ];

    for (row, dx, dy) in look_directions {
        for (col, open) in [(0u32, true), (1, false)] {
            s.body(col, row);
            s.eyes(col, row, dx, dy, open);
        }
    }

    // Fila 13 (14) — wake_up: despertar alegre con ojos abiertos y sonrisa.
    for (col, open) in [(0u32, false), (1, true)] {
        s.body(col, 13);
        s.eyes(col, 13, 0, 0, open);
        if open {
            s.smile(col, 13, 10);
        }
    }

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/demo");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Hoja de rejilla PNG
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

    // APNG de `writing`: 3 marcos completos de 48×48 (patas arriba/abajo/arriba)
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
    let mut wr = enc.write_header().unwrap();
    for f in &frames {
        wr.set_frame_delay(1, 10).unwrap();
        wr.write_image_data(f).unwrap();
    }
    wr.finish().unwrap();
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
