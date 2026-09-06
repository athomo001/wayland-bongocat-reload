//! Generador **procedural** del sprite sheet de Umbreon (gato negro con anillos
//! amarillos). Sin ficheros externos — reproducible:
//!
//!   cargo run -p wayvpet --example gen_umbreon_sheet
//!
//! Dibuja un "muñeco" articulado (torso, cabeza, 2 orejas bífidas, 4 patas de 2
//! segmentos, cola curva, anillos, ojos rojos) a partir de una `Pose` (offsets +
//! ángulos de articulación) y **interpola entre poses clave**, así que cada
//! fotograma es distinto y el movimiento es fluido. 19 filas × 16 columnas,
//! 128×128 por celda (misma matriz que antes; primero se mejora el movimiento,
//! ensancharla es otro paso).

use std::f32::consts::PI;
use std::path::Path;

const FW: i32 = 128;
const FH: i32 = 128;
const COLS: u32 = 16;
const ROWS: u32 = 19;
const GROUND: f32 = 120.0; // línea de suelo (pies) dentro del frame

type Col = [f32; 4]; // RGBA lineal-ish, alfa recto 0..1

const FUR: Col = [0.105, 0.098, 0.120, 1.0]; // negro azulado
const FUR_LO: Col = [0.058, 0.054, 0.075, 1.0]; // sombra / partes traseras
const FUR_HI: Col = [0.165, 0.160, 0.195, 1.0]; // vientre / brillo
const RING: Col = [0.957, 0.788, 0.157, 1.0]; // anillos amarillos
const RING_LO: Col = [0.78, 0.60, 0.09, 1.0];
const EYE: Col = [0.90, 0.24, 0.24, 1.0]; // ojos rojos
const EYE_HI: Col = [1.0, 0.85, 0.85, 1.0];
const MOUTH: Col = [0.10, 0.05, 0.06, 1.0];
const TONGUE: Col = [0.92, 0.47, 0.55, 1.0];
const RIM: Col = [0.97, 0.97, 0.98, 0.92]; // contorno die-cut suave
const SHADOW: Col = [0.0, 0.0, 0.0, 0.22];
const ZZZ: Col = [0.96, 0.88, 0.35, 0.95];
const SPARK: Col = [1.0, 0.92, 0.35, 0.90];
const ANGER: Col = [1.0, 0.30, 0.30, 0.92];

// ───────────────────────── lienzo + primitivas ─────────────────────────

struct Canvas {
    px: Vec<Col>, // FW*FH, alfa recto
}

impl Canvas {
    fn new() -> Self {
        Self {
            px: vec![[0.0; 4]; (FW * FH) as usize],
        }
    }

    /// `src` sobre destino con cobertura `cov` (0..1). Alfa recto.
    fn blend(&mut self, x: i32, y: i32, mut src: Col, cov: f32) {
        if x < 0 || y < 0 || x >= FW || y >= FH || cov <= 0.0 {
            return;
        }
        src[3] *= cov.clamp(0.0, 1.0);
        if src[3] <= 0.0 {
            return;
        }
        let d = &mut self.px[(y * FW + x) as usize];
        let a = src[3] + d[3] * (1.0 - src[3]);
        if a <= 0.0 {
            return;
        }
        for c in 0..3 {
            d[c] = (src[c] * src[3] + d[c] * d[3] * (1.0 - src[3])) / a;
        }
        d[3] = a;
    }

    /// Elipse rellena con borde suave (~1 px de antialias).
    fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Col) {
        let (rx, ry) = (rx.max(0.5), ry.max(0.5));
        let x0 = (cx - rx - 2.0).floor() as i32;
        let x1 = (cx + rx + 2.0).ceil() as i32;
        let y0 = (cy - ry - 2.0).floor() as i32;
        let y1 = (cy + ry + 2.0).ceil() as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let nx = (x as f32 + 0.5 - cx) / rx;
                let ny = (y as f32 + 0.5 - cy) / ry;
                let d = (nx * nx + ny * ny).sqrt(); // 1.0 = borde
                let edge = 1.0 / rx.min(ry).max(1.0); // grosor AA en unidades normalizadas
                let cov = smoothstep(1.0 + edge, 1.0 - edge, d);
                self.blend(x, y, c, cov);
            }
        }
    }

    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: Col) {
        self.ellipse(cx, cy, r, r, c);
    }

    /// Cápsula (segmento grueso con extremos redondeados): patas, cola, orejas.
    fn capsule(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: Col) {
        let r = r.max(0.5);
        let minx = (x0.min(x1) - r - 2.0).floor() as i32;
        let maxx = (x0.max(x1) + r + 2.0).ceil() as i32;
        let miny = (y0.min(y1) - r - 2.0).floor() as i32;
        let maxy = (y0.max(y1) + r + 2.0).ceil() as i32;
        for y in miny..=maxy {
            for x in minx..=maxx {
                let d = dist_seg(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1);
                let cov = smoothstep(r + 0.75, r - 0.75, d);
                self.blend(x, y, c, cov);
            }
        }
    }

    fn to_rgba8(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.px.len() * 4];
        for (i, p) in self.px.iter().enumerate() {
            for c in 0..4 {
                out[i * 4 + c] = (p[c].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        out
    }

    /// Añade un contorno blanco die-cut de radio `r` alrededor de todo lo opaco.
    fn add_rim(&mut self, r: f32) {
        let solid: Vec<bool> = self.px.iter().map(|p| p[3] > 0.5).collect();
        let ri = r.ceil() as i32;
        let mut rim = vec![0.0f32; self.px.len()];
        for y in 0..FH {
            for x in 0..FW {
                if solid[(y * FW + x) as usize] {
                    continue;
                }
                let mut best = f32::MAX;
                for dy in -ri..=ri {
                    for dx in -ri..=ri {
                        let (nx, ny) = (x + dx, y + dy);
                        if (0..FW).contains(&nx)
                            && (0..FH).contains(&ny)
                            && solid[(ny * FW + nx) as usize]
                        {
                            let dd = ((dx * dx + dy * dy) as f32).sqrt();
                            if dd < best {
                                best = dd;
                            }
                        }
                    }
                }
                if best <= r {
                    rim[(y * FW + x) as usize] = smoothstep(r + 0.5, r - 0.9, best);
                }
            }
        }
        // Pinta el rim **detrás** de lo ya dibujado (destino-sobre).
        for (d, &rv) in self.px.iter_mut().zip(rim.iter()) {
            if rv <= 0.0 {
                continue;
            }
            let cov = rv * RIM[3];
            let a = d[3] + cov * (1.0 - d[3]);
            if a <= 0.0 {
                continue;
            }
            for c in 0..3 {
                d[c] = (d[c] * d[3] + RIM[c] * cov * (1.0 - d[3])) / a;
            }
            d[3] = a;
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn dist_seg(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 1e-6 {
        0.0
    } else {
        (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (x0 + t * dx, y0 + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

// ───────────────────────── pose del muñeco ─────────────────────────

#[derive(Clone)]
struct Pose {
    /// 0 = de frente, 1 = de perfil (mirando a la derecha).
    facing: f32,
    /// Desplazamiento global del cuerpo (salto, bob…).
    body_dx: f32,
    body_dy: f32,
    /// Estiramiento vertical del torso (respiración: 1.0 neutro).
    body_sy: f32,
    /// Inclinación del lomo en radianes (arqueo de `angry`, estiramiento).
    spine: f32,
    /// Cabeza: desplazamiento e inclinación.
    head_dx: f32,
    head_dy: f32,
    head_tilt: f32,
    /// Orejas: ángulo respecto a la vertical (rad). + = hacia fuera/atrás.
    ear: f32,
    /// Cola: ángulo de base (rad) y curvatura (rad por segmento).
    tail_base: f32,
    tail_curl: f32,
    /// Patas: (ángulo de cadera, flexión de rodilla) para
    /// [frontal-izq, frontal-der, trasera-izq, trasera-der].
    legs: [(f32, f32); 4],
    /// Mirada de los ojos (offset en px) y estado del párpado.
    gaze: (f32, f32),
    eyes: Eyes,
    mouth: Mouth,
    /// Postura sentada (0) vs de pie/andando (1): sube el torso, saca patas.
    stand: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Eyes {
    Open,
    Half,
    Closed,
    Angry,
    Happy,
}
#[derive(Clone, Copy, PartialEq)]
enum Mouth {
    None,
    Smile,
    Open,
}

impl Default for Pose {
    fn default() -> Self {
        // Gato **sentado** de frente, en reposo.
        Pose {
            facing: 0.12,
            body_dx: 0.0,
            body_dy: 0.0,
            body_sy: 1.0,
            spine: 0.0,
            head_dx: 0.0,
            head_dy: 0.0,
            head_tilt: 0.0,
            ear: 0.30,
            tail_base: -0.55,
            tail_curl: 0.55,
            legs: [(0.0, 0.05), (0.0, 0.05), (0.9, 1.5), (0.9, 1.5)],
            gaze: (0.0, 0.0),
            eyes: Eyes::Open,
            mouth: Mouth::None,
            stand: 0.0,
        }
    }
}

fn lf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_pose(a: &Pose, b: &Pose, t: f32) -> Pose {
    let s = smoothstep(0.0, 1.0, t); // suaviza el inbetween
    let mut legs = a.legs;
    for (l, (&la, &lb)) in legs.iter_mut().zip(a.legs.iter().zip(b.legs.iter())) {
        l.0 = lf(la.0, lb.0, s);
        l.1 = lf(la.1, lb.1, s);
    }
    Pose {
        facing: lf(a.facing, b.facing, s),
        body_dx: lf(a.body_dx, b.body_dx, s),
        body_dy: lf(a.body_dy, b.body_dy, s),
        body_sy: lf(a.body_sy, b.body_sy, s),
        spine: lf(a.spine, b.spine, s),
        head_dx: lf(a.head_dx, b.head_dx, s),
        head_dy: lf(a.head_dy, b.head_dy, s),
        head_tilt: lf(a.head_tilt, b.head_tilt, s),
        ear: lf(a.ear, b.ear, s),
        tail_base: lf(a.tail_base, b.tail_base, s),
        tail_curl: lf(a.tail_curl, b.tail_curl, s),
        legs,
        gaze: (lf(a.gaze.0, b.gaze.0, s), lf(a.gaze.1, b.gaze.1, s)),
        eyes: if s < 0.5 { a.eyes } else { b.eyes },
        mouth: if s < 0.5 { a.mouth } else { b.mouth },
        stand: lf(a.stand, b.stand, s),
    }
}

/// Muestrea una lista de poses clave en `n` fotogramas repartidos por igual.
/// `loop_` cierra el ciclo volviendo a la primera.
fn sample(keys: &[Pose], n: usize, loop_: bool) -> Vec<Pose> {
    let mut out = Vec::with_capacity(n);
    let segs = if loop_ { keys.len() } else { keys.len() - 1 };
    for i in 0..n {
        let u = i as f32 / n as f32 * segs as f32;
        let k = (u.floor() as usize).min(keys.len() - 1);
        let t = u - k as f32;
        let a = &keys[k];
        let b = &keys[(k + 1) % keys.len()];
        out.push(lerp_pose(a, b, t));
    }
    out
}

// ───────────────────────── dibujo del muñeco ─────────────────────────

fn draw_umbreon(cv: &mut Canvas, p: &Pose) {
    let cx = 64.0 + p.body_dx;
    // Altura del torso: sentado bajo, de pie más arriba.
    let torso_y = GROUND - lf(27.0, 37.0, p.stand) + p.body_dy;
    let torso_rx = lf(23.0, 28.0, p.facing);
    let torso_ry = lf(14.5, 11.5, p.facing) * p.body_sy;

    // Sombra en el suelo.
    cv.ellipse(cx, GROUND + 1.0, torso_rx * 0.95, 4.0, SHADOW);

    // ---- patas traseras (se dibujan primero: quedan detrás) ----
    let hip_back = [
        (cx - lf(12.0, 20.0, p.facing), torso_y + 4.0),
        (cx + lf(12.0, 6.0, p.facing), torso_y + 4.0),
    ];
    draw_leg(cv, hip_back[0], p.legs[2], p.spine, FUR_LO, true);
    draw_leg(cv, hip_back[1], p.legs[3], p.spine, FUR_LO, true);

    // ---- cola (detrás del cuerpo) ----
    draw_tail(
        cv,
        cx + lf(16.0, -22.0, p.facing),
        torso_y,
        p.tail_base,
        p.tail_curl,
    );

    // ---- torso ----
    // arqueo del lomo: desplaza el centro hacia arriba con `spine`
    let arch = p.spine.abs() * 8.0;
    cv.ellipse(cx, torso_y - arch, torso_rx, torso_ry + arch * 0.4, FUR);
    // vientre más claro
    cv.ellipse(
        cx + lf(0.0, 6.0, p.facing),
        torso_y + torso_ry * 0.35,
        torso_rx * 0.7,
        torso_ry * 0.55,
        FUR_HI,
    );

    // ---- patas delanteras ----
    let hip_front = [
        (cx - lf(9.0, -2.0, p.facing), torso_y + torso_ry * 0.5),
        (cx + lf(9.0, 14.0, p.facing), torso_y + torso_ry * 0.5),
    ];
    draw_leg(cv, hip_front[0], p.legs[0], 0.0, FUR, false);
    draw_leg(cv, hip_front[1], p.legs[1], 0.0, FUR, false);

    // ---- cabeza ----
    let hx = cx + lf(0.0, 22.0, p.facing) + p.head_dx;
    let hy = torso_y - lf(13.0, 17.0, p.facing) + p.head_dy;
    let hr = 15.0;
    cv.disc(hx, hy, hr, FUR);
    // hocico ligeramente más claro
    cv.ellipse(hx + lf(0.0, 8.0, p.facing), hy + 6.0, 9.0, 7.0, FUR_HI);

    // orejas bífidas
    draw_ear(
        cv,
        hx - hr * 0.55,
        hy - hr * 0.6,
        -p.ear + p.head_tilt,
        true,
    );
    draw_ear(
        cv,
        hx + hr * 0.55,
        hy - hr * 0.6,
        p.ear + p.head_tilt,
        false,
    );

    // anillo de la frente
    cv.disc(hx + lf(0.0, 4.0, p.facing), hy - hr * 0.35, 3.6, RING);

    // ojos rojos
    draw_eyes(cv, hx, hy, p.facing, p.gaze, p.eyes);

    // boca
    match p.mouth {
        Mouth::None => {}
        Mouth::Smile => {
            let mx = hx + lf(0.0, 7.0, p.facing);
            let my = hy + 9.0;
            for i in -4..=4 {
                let t = i as f32 / 4.0;
                cv.blend(
                    (mx + t * 4.0) as i32,
                    (my + t.abs() * 1.6) as i32,
                    MOUTH,
                    0.9,
                );
            }
        }
        Mouth::Open => {
            let mx = hx + lf(0.0, 9.0, p.facing);
            let my = hy + 8.0;
            cv.ellipse(mx, my, 4.0, 4.5, MOUTH);
            cv.ellipse(mx, my + 1.5, 2.6, 2.4, TONGUE);
        }
    }
}

fn draw_leg(
    cv: &mut Canvas,
    hip: (f32, f32),
    (hipang, knee): (f32, f32),
    spine: f32,
    col: Col,
    back: bool,
) {
    let thigh = 13.5;
    let shin = 13.5;
    // Ángulo base: hacia abajo (PI/2) + oscilación de cadera.
    let a1 = PI / 2.0 + hipang + spine * 0.4;
    let kx = hip.0 + a1.cos() * thigh;
    let ky = hip.1 + a1.sin() * thigh;
    let a2 = a1 + knee; // flexión de rodilla hacia atrás
    let mut px = kx + a2.cos() * shin;
    let mut py = ky + a2.sin() * shin;
    // no atraviesa el suelo
    if py > GROUND {
        let k = (GROUND - ky) / (py - ky).max(0.01);
        px = kx + (px - kx) * k;
        py = GROUND;
    }
    let r = if back { 3.4 } else { 3.9 };
    cv.capsule(hip.0, hip.1, kx, ky, r, col);
    cv.capsule(kx, ky, px, py, r * 0.9, col);
    cv.disc(px, py, r * 0.95, col); // patita
                                    // anillo amarillo del tobillo
    let tx = kx + (px - kx) * 0.55;
    let ty = ky + (py - ky) * 0.55;
    cv.capsule(
        tx - 2.5,
        ty,
        tx + 2.5,
        ty,
        3.4,
        if back { RING_LO } else { RING },
    );
}

fn draw_tail(cv: &mut Canvas, bx: f32, by: f32, base: f32, curl: f32) {
    let seg = 9.0;
    let mut x = bx;
    let mut y = by;
    let mut ang = base; // 0 = derecha, -PI/2 = arriba
    for i in 0..4 {
        let nx = x + ang.cos() * seg;
        let ny = y + ang.sin() * seg;
        let r = 4.6 - i as f32 * 0.7;
        cv.capsule(x, y, nx, ny, r.max(2.0), FUR);
        if i == 3 {
            // anillo de la punta
            let mx = x + (nx - x) * 0.5;
            let my = y + (ny - y) * 0.5;
            cv.capsule(mx - 2.2, my, mx + 2.2, my, 3.2, RING);
        }
        x = nx;
        y = ny;
        ang -= curl;
    }
}

fn draw_ear(cv: &mut Canvas, bx: f32, by: f32, ang: f32, _left: bool) {
    // oreja alargada: cápsula gruesa que se afina + banda amarilla en la base
    let len = 22.0;
    let tx = bx + ang.sin() * 6.0;
    let ty = by - len;
    // dos cápsulas superpuestas dan una punta más fina
    cv.capsule(bx, by, lf(bx, tx, 0.5), lf(by, ty, 0.5), 5.2, FUR);
    cv.capsule(lf(bx, tx, 0.4), lf(by, ty, 0.4), tx, ty, 3.0, FUR);
    // banda amarilla ~1/3 desde la base
    let rx = lf(bx, tx, 0.32);
    let ry = lf(by, ty, 0.32);
    cv.capsule(rx - 3.5, ry, rx + 3.5, ry, 3.6, RING);
}

fn draw_eyes(cv: &mut Canvas, hx: f32, hy: f32, facing: f32, gaze: (f32, f32), eyes: Eyes) {
    // De perfil solo se ve un ojo.
    let spread = lf(7.5, 3.0, facing);
    let shift = lf(0.0, 6.0, facing);
    let centers = if facing > 0.6 {
        vec![(hx + shift + spread, hy - 1.0)]
    } else {
        vec![
            (hx - spread, hy - 1.0),
            (hx + spread + shift * 0.3, hy - 1.0),
        ]
    };
    for (ex, ey) in centers {
        let (gx, gy) = (gaze.0 * 0.6, gaze.1 * 0.6);
        match eyes {
            Eyes::Open => {
                cv.ellipse(ex, ey, 2.7, 3.8, EYE);
                cv.disc(ex + gx, ey + gy - 0.6, 1.1, EYE_HI);
            }
            Eyes::Half => {
                cv.ellipse(ex, ey + 1.0, 3.2, 2.0, EYE);
            }
            Eyes::Closed => {
                for i in -3i32..=3 {
                    cv.blend(
                        (ex + i as f32) as i32,
                        (ey + 1.0 + (i.abs() as f32) * 0.4) as i32,
                        MOUTH,
                        0.85,
                    );
                }
            }
            Eyes::Angry => {
                // ojo entornado con ceja: línea diagonal
                for i in -3i32..=3 {
                    let t = i as f32;
                    cv.blend(
                        (ex + t) as i32,
                        (ey - 1.5 + t * 0.55 * facing_sign(facing)) as i32,
                        MOUTH,
                        0.9,
                    );
                }
                cv.ellipse(ex, ey + 1.5, 3.0, 2.2, EYE);
            }
            Eyes::Happy => {
                // ojo cerrado en arco alegre ^
                for i in -3i32..=3 {
                    let t = i as f32;
                    cv.blend(
                        (ex + t) as i32,
                        (ey + 1.0 + t.abs() * -0.55) as i32,
                        MOUTH,
                        0.9,
                    );
                }
            }
        }
    }
}

fn facing_sign(f: f32) -> f32 {
    if f > 0.5 {
        1.0
    } else {
        -1.0
    }
}

// ───────────────────────── extras por fotograma ─────────────────────────

fn draw_z(cv: &mut Canvas, cx: f32, cy: f32, size: f32, alpha: f32) {
    let s = size.max(3.0);
    let mut c = ZZZ;
    c[3] *= alpha.clamp(0.0, 1.0);
    let put = |cv: &mut Canvas, x: f32, y: f32| {
        for dy in 0..2 {
            for dx in 0..2 {
                cv.blend(x as i32 + dx, y as i32 + dy, c, 0.9);
            }
        }
    };
    for k in 0..(s as i32) {
        let t = k as f32;
        put(cv, cx + t, cy);
        put(cv, cx + s - t, cy + t);
        put(cv, cx + t, cy + s);
    }
}

fn draw_spark(cv: &mut Canvas, cx: f32, cy: f32, r: f32, c: Col) {
    for dy in -(r as i32)..=(r as i32) {
        for dx in -(r as i32)..=(r as i32) {
            if (dx.abs() + dy.abs()) as f32 <= r {
                cv.blend(cx as i32 + dx, cy as i32 + dy, c, 0.85);
            }
        }
    }
}

fn draw_bowl(cv: &mut Canvas, cx: f32, cy: f32) {
    cv.ellipse(cx, cy, 12.0, 4.5, [0.30, 0.55, 0.75, 1.0]);
    cv.ellipse(cx, cy - 1.0, 9.0, 3.0, [0.55, 0.38, 0.22, 1.0]); // croquetas
}

fn draw_toy_mouse(cv: &mut Canvas, cx: f32, cy: f32) {
    cv.ellipse(cx, cy, 5.0, 3.5, [0.55, 0.55, 0.60, 1.0]);
    cv.disc(cx - 4.5, cy - 1.0, 1.6, [0.55, 0.55, 0.60, 1.0]); // cabeza
    cv.capsule(
        cx + 4.0,
        cy,
        cx + 9.0,
        cy - 2.0,
        0.8,
        [0.55, 0.55, 0.60, 1.0],
    ); // cola
}

// ───────────────────────── construcción de poses por estado ─────────────────────────

fn p_seated() -> Pose {
    Pose::default()
}

fn p_seated_breath(inhale: f32) -> Pose {
    let mut p = p_seated();
    p.body_sy = 1.0 + 0.045 * inhale;
    p.body_dy = -1.6 * inhale;
    p.ear = 0.30 - 0.05 * inhale;
    p.tail_base = -0.55 + 0.12 * (inhale - 0.5);
    p
}

fn p_hunched() -> Pose {
    // encorvado sobre el teclado
    let mut p = p_seated();
    p.facing = 0.35;
    p.head_dy = 6.0;
    p.head_dx = 4.0;
    p.head_tilt = 0.18;
    p.body_dy = 2.0;
    p.ear = 0.5;
    p.legs[0] = (-0.7, 0.35); // patas delanteras levantadas a "teclear"
    p.legs[1] = (-0.7, 0.35);
    p.eyes = Eyes::Half;
    p
}

fn p_curled() -> Pose {
    let mut p = p_seated();
    p.facing = 0.55;
    p.body_sy = 0.92;
    p.body_dy = 6.0;
    p.head_dx = -6.0;
    p.head_dy = 12.0;
    p.head_tilt = -0.5;
    p.ear = -0.15; // orejas planas
    p.tail_base = -2.7; // cola envolviendo por delante
    p.tail_curl = 0.9;
    p.legs = [(1.4, 1.7), (1.4, 1.7), (1.6, 1.9), (1.6, 1.9)];
    p.eyes = Eyes::Closed;
    p
}

fn p_stand_profile() -> Pose {
    let mut p = p_seated();
    p.facing = 0.92;
    p.stand = 1.0;
    p.tail_base = -0.2;
    p.tail_curl = 0.35;
    p.ear = 0.15;
    p.legs = [(0.0, 0.1), (0.0, 0.1), (0.0, 0.1), (0.0, 0.1)];
    p
}

/// Un fotograma del ciclo de marcha (fase 0..1).
fn p_walk(phase: f32) -> Pose {
    let mut p = p_stand_profile();
    let a = phase * 2.0 * PI;
    // pares diagonales: FL+BR y FR+BL en contrafase
    let d1 = a.sin();
    let d2 = (a + PI).sin();
    let swing = 0.55;
    let lift = 0.8;
    p.legs[0] = (d1 * swing, 0.15 + (d1.max(0.0)) * lift); // frontal-izq
    p.legs[3] = (d1 * swing, 0.15 + (d1.max(0.0)) * lift); // trasera-der
    p.legs[1] = (d2 * swing, 0.15 + (d2.max(0.0)) * lift); // frontal-der
    p.legs[2] = (d2 * swing, 0.15 + (d2.max(0.0)) * lift); // trasera-izq
    p.body_dy = -(a * 2.0).cos().abs() * 1.6; // bob del cuerpo (2 por ciclo)
    p.head_dy = -(a * 2.0).cos() * 1.0;
    p.tail_base = -0.2 + a.sin() * 0.25; // contrafase
    p.ear = 0.15 + (a * 2.0).sin() * 0.08;
    p
}

fn p_arch() -> Pose {
    let mut p = p_stand_profile();
    p.spine = -0.9; // lomo arqueado hacia arriba
    p.body_dy = -6.0;
    p.head_dy = 4.0;
    p.head_tilt = -0.3;
    p.ear = -0.35; // planas hacia atrás
    p.tail_base = -1.5; // recta hacia arriba
    p.tail_curl = 0.05;
    p.legs = [(-0.15, 0.15), (-0.15, 0.15), (0.15, 0.15), (0.15, 0.15)];
    p.eyes = Eyes::Angry;
    p.mouth = Mouth::Open;
    p
}

// ───────────────────────── filas del sheet ─────────────────────────

type Extra = Box<dyn Fn(&mut Canvas, usize, usize)>; // (canvas, frame_i, n)

struct Row {
    frames: Vec<Pose>,
    extra: Option<Extra>,
}

fn build_rows() -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();

    // 1 · idle ×16 — respiración + vaivén de cola + parpadeo
    {
        let keys = [
            p_seated_breath(0.0),
            p_seated_breath(1.0),
            p_seated_breath(0.0),
        ];
        let mut fr = sample(&keys, 16, true);
        for (i, f) in fr.iter_mut().enumerate() {
            f.gaze.0 = ((i as f32 / 16.0 * 2.0 * PI).sin()) * 1.2;
            f.eyes = match i {
                9 | 11 => Eyes::Half,
                10 => Eyes::Closed,
                _ => Eyes::Open,
            };
            if i == 6 {
                f.ear += 0.25; // tic de oreja
            }
        }
        rows.push(Row {
            frames: fr,
            extra: None,
        });
    }

    // 2 · start_writing ×8 — de sentado a encorvado
    rows.push(Row {
        frames: sample(&[p_seated(), p_hunched()], 8, false),
        extra: None,
    });

    // 3 · writing ×16 — patitas alternadas + chispa
    {
        let mut fr = Vec::new();
        for i in 0..16 {
            let mut p = p_hunched();
            let ph = i as f32 / 16.0 * 2.0 * PI;
            p.body_dy += (ph * 2.0).sin().abs() * -1.5;
            let up = (i / 2) % 2 == 0;
            p.legs[0] = (if up { -0.95 } else { -0.55 }, 0.4);
            p.legs[1] = (if up { -0.55 } else { -0.95 }, 0.4);
            if i % 8 == 0 {
                p.ear += 0.3;
            }
            fr.push(p);
        }
        rows.push(Row {
            frames: fr,
            extra: Some(Box::new(|cv, i, _| {
                if i % 4 < 2 {
                    let ph = i as f32 / 16.0 * 2.0 * PI;
                    draw_spark(cv, 64.0 + ph.cos() * 14.0, GROUND - 6.0, 3.0, SPARK);
                }
            })),
        });
    }

    // 4 · end_writing ×8 — de encorvado a sentado
    rows.push(Row {
        frames: sample(&[p_hunched(), p_seated()], 8, false),
        extra: None,
    });

    // 5 · sleep ×16 — acurrucado, respiración lenta + Zzz en olas
    {
        let mut a = p_curled();
        a.body_sy = 0.90;
        let mut b = p_curled();
        b.body_sy = 0.96;
        b.body_dy += 1.0;
        let fr = sample(&[a.clone(), b, a], 16, true);
        rows.push(Row {
            frames: fr,
            extra: Some(Box::new(|cv, i, n| {
                let ph = i as f32 / n as f32;
                for w in 0..3 {
                    let pz = (ph + w as f32 / 3.0) % 1.0;
                    let zx = 80.0 + pz * 16.0 + (pz * 6.0).sin() * 3.0;
                    let zy = 46.0 - pz * 32.0;
                    let al = if pz < 0.15 {
                        pz / 0.15
                    } else if pz > 0.8 {
                        (1.0 - pz) / 0.2
                    } else {
                        1.0
                    };
                    draw_z(cv, zx, zy, 3.0 + pz * 5.0, al);
                }
            })),
        });
    }

    // 6 · happy ×16 — agazapa → salta → aire → cae → celebra
    {
        let mut crouch = p_seated();
        crouch.facing = 0.6;
        crouch.body_dy = 5.0;
        crouch.head_dy = 6.0;
        crouch.legs = [(0.5, 0.9), (0.5, 0.9), (1.1, 1.6), (1.1, 1.6)];
        crouch.tail_base = -1.3;
        let mut spring = p_stand_profile();
        spring.body_dy = -10.0;
        spring.legs = [(-0.5, 0.1), (-0.5, 0.1), (-0.7, 0.2), (-0.7, 0.2)];
        let mut air = p_stand_profile();
        air.body_dy = -22.0;
        air.body_dx = 4.0;
        air.legs = [(0.6, 1.2), (0.6, 1.2), (0.7, 1.3), (0.7, 1.3)];
        air.tail_base = -0.9;
        let mut land = p_stand_profile();
        land.body_dy = -2.0;
        land.legs = [(-0.7, 0.3), (-0.7, 0.3), (0.6, 0.9), (0.6, 0.9)];
        let mut cheer = p_seated();
        cheer.eyes = Eyes::Happy;
        cheer.mouth = Mouth::Smile;
        cheer.tail_base = -1.4;
        let fr = sample(
            &[
                crouch.clone(),
                crouch,
                spring,
                air,
                land,
                cheer.clone(),
                cheer,
            ],
            16,
            false,
        );
        rows.push(Row {
            frames: fr,
            extra: Some(Box::new(|cv, i, _| {
                if i < 4 || (11..14).contains(&i) {
                    draw_toy_mouse(cv, 96.0, GROUND - 3.0);
                }
            })),
        });
    }

    // 7 · boring ×16 — aseo: lame pata → cara → oreja → sacude → presume
    {
        let mut sit = p_seated();
        sit.eyes = Eyes::Half;
        let mut lick = p_seated();
        lick.eyes = Eyes::Half;
        lick.head_dy = 8.0;
        lick.head_tilt = 0.35;
        lick.legs[0] = (-1.4, 0.6); // pata a la boca
        lick.mouth = Mouth::Open;
        let mut face = lick.clone();
        face.head_tilt = -0.2;
        face.legs[0] = (-1.9, 0.4);
        face.mouth = Mouth::None;
        let mut ear = face.clone();
        ear.legs[0] = (-2.4, 0.2); // pata sobre la oreja
        ear.head_tilt = -0.5;
        ear.ear = -0.2;
        let mut shake = p_seated();
        shake.legs[0] = (-0.6, 0.9);
        let mut proud = p_seated();
        proud.eyes = Eyes::Happy;
        proud.mouth = Mouth::Smile;
        let fr = sample(
            &[
                sit.clone(),
                sit,
                lick.clone(),
                lick,
                face,
                ear.clone(),
                ear,
                shake,
                proud,
            ],
            16,
            false,
        );
        rows.push(Row {
            frames: fr,
            extra: None,
        });
    }

    // 8-15 · look_* ×4 — mirada direccional (aunque este tema use 'activity')
    let dirs = [
        (-6.0f32, 0.0f32),
        (6.0, 0.0),
        (0.0, -5.0),
        (0.0, 5.0),
        (-5.0, -4.0),
        (5.0, -4.0),
        (-5.0, 4.0),
        (5.0, 4.0),
    ];
    for &(dx, dy) in &dirs {
        let mut a = p_seated();
        a.gaze = (0.0, 0.0);
        let mut b = p_seated();
        b.gaze = (dx, dy);
        b.head_dx = dx * 0.35;
        b.head_dy = dy * 0.35;
        b.ear = 0.30 + dx.signum() * 0.06;
        rows.push(Row {
            frames: sample(&[a, b], 4, false),
            extra: None,
        });
    }

    // 16 · wake_up ×8 — de acurrucado a estiramiento a sentado
    {
        let mut stretch = p_stand_profile();
        stretch.spine = 0.7; // lomo hacia abajo, culo arriba
        stretch.body_dx = -4.0;
        stretch.head_dy = 10.0;
        stretch.legs = [(-1.2, 0.1), (-1.2, 0.1), (0.9, 0.2), (0.9, 0.2)];
        stretch.eyes = Eyes::Closed;
        rows.push(Row {
            frames: sample(&[p_curled(), stretch, p_seated()], 8, false),
            extra: None,
        });
    }

    // 17 · walk ×16 — ciclo de 4 patas
    {
        let fr: Vec<Pose> = (0..16).map(|i| p_walk(i as f32 / 16.0)).collect();
        rows.push(Row {
            frames: fr,
            extra: None,
        });
    }

    // 18 · eat_ram ×16 — cabeza al plato / mastica / sube
    {
        let mut down = p_stand_profile();
        down.head_dy = 16.0;
        down.head_dx = 6.0;
        down.spine = 0.25;
        let mut chew = p_stand_profile();
        chew.head_dy = 6.0;
        chew.head_dx = 4.0;
        let mut up = p_stand_profile();
        up.head_dy = -2.0;
        let fr = sample(
            &[
                down.clone(),
                down.clone(),
                chew.clone(),
                chew.clone(),
                chew.clone(),
                up,
                chew,
                down,
            ],
            16,
            true,
        );
        rows.push(Row {
            frames: fr,
            extra: Some(Box::new(|cv, i, _| {
                draw_bowl(cv, 94.0, GROUND - 2.0);
                if i % 3 == 0 {
                    draw_spark(cv, 90.0, GROUND - 8.0, 2.0, [0.85, 0.6, 0.25, 0.8]);
                }
            })),
        });
    }

    // 19 · angry ×16 — lomo arqueado, bufido, coletazo
    {
        let a = p_arch();
        let mut b = p_arch();
        b.tail_base = -1.2;
        b.body_dx = 1.5;
        let mut c = p_arch();
        c.body_dx = 4.0; // pequeño amago hacia delante
        c.head_dx = 3.0;
        let fr = sample(&[a.clone(), a, b.clone(), b.clone(), c, b], 16, true);
        rows.push(Row {
            frames: fr,
            extra: Some(Box::new(|cv, i, _| {
                if i % 2 == 0 {
                    draw_spark(cv, 92.0, 44.0, 2.0, ANGER);
                    draw_spark(cv, 100.0, 50.0, 1.5, ANGER);
                }
            })),
        });
    }

    rows
}

// ───────────────────────── main ─────────────────────────

fn render_frame(p: &Pose, extra: Option<(&Extra, usize, usize)>) -> Canvas {
    let mut cv = Canvas::new();
    draw_umbreon(&mut cv, p);
    cv.add_rim(1.8);
    if let Some((f, i, n)) = extra {
        f(&mut cv, i, n);
    }
    cv
}

fn main() {
    let rows = build_rows();
    let sw = COLS * FW as u32;
    let sh = ROWS * FH as u32;
    let mut sheet = vec![0u8; (sw * sh * 4) as usize];

    for (r, row) in rows.iter().enumerate() {
        let n = row.frames.len();
        for (i, pose) in row.frames.iter().enumerate() {
            let ex = row.extra.as_ref().map(|e| (e, i, n));
            let cv = render_frame(pose, ex);
            let fr = cv.to_rgba8();
            let ox = i as u32 * FW as u32;
            let oy = r as u32 * FH as u32;
            for y in 0..FH as u32 {
                for x in 0..FW as u32 {
                    let si = ((y * FW as u32 + x) * 4) as usize;
                    let di = (((oy + y) * sw + ox + x) * 4) as usize;
                    sheet[di..di + 4].copy_from_slice(&fr[si..si + 4]);
                }
            }
        }
    }

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/umbreon");
    std::fs::create_dir_all(&out_dir).unwrap();

    let path = out_dir.join("sheet.png");
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), sw, sh);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&sheet)
        .unwrap();
    println!("escrito {} ({sw}x{sh})", path.display());

    // writing.apng = la fila 3 (índice 2)
    let wrow = &rows[2];
    let n = wrow.frames.len();
    let path = out_dir.join("writing.apng");
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), FW as u32, FH as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_animated(n as u32, 0).unwrap();
    let mut wr = enc.write_header().unwrap();
    for (i, pose) in wrow.frames.iter().enumerate() {
        let ex = wrow.extra.as_ref().map(|e| (e, i, n));
        wr.set_frame_delay(1, 12).unwrap();
        wr.write_image_data(&render_frame(pose, ex).to_rgba8())
            .unwrap();
    }
    wr.finish().unwrap();
    println!(
        "escrito {} (APNG {}×{}, {n} marcos)",
        path.display(),
        FW,
        FH
    );
}
