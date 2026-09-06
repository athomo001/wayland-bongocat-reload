//! Estado de patas y su traducción a fotograma.
//!
//! El proceso lector de input traduce cada tecla a un bit de pata y lo descarta
//! (el keycode nunca sube más arriba). El hilo de animación consume esos bits.
//! Portado de `include/graphics/paw_frame.h`.

/// Bit de la pata izquierda.
pub const PAW_LEFT: u8 = 1;
/// Bit de la pata derecha.
pub const PAW_RIGHT: u8 = 2;
/// Ambas patas.
pub const PAW_BOTH: u8 = PAW_LEFT | PAW_RIGHT;
/// Marca "el evento vino de una **tecla**" (no del ratón). El lector aislado la
/// activa en el byte de cada pulsación; el padre la usa para contar
/// teclas/minuto (`happy_kpm`, spec 0014 M6) sin conocer la tecla (spec 0013).
pub const PAW_KEY: u8 = 4;

/// Índices de fotograma (coinciden con `BONGOCAT_FRAME_*` de `include/core/bongocat.h`).
pub const FRAME_BOTH_UP: u8 = 0;
pub const FRAME_LEFT_DOWN: u8 = 1;
pub const FRAME_RIGHT_DOWN: u8 = 2;
pub const FRAME_BOTH_DOWN: u8 = 3;
pub const FRAME_SLEEPING: u8 = 4;

/// Teclas de la mitad izquierda del teclado (keycodes de Linux `input-event-codes`).
/// El resto se considera "mano derecha". Es una partición gruesa
/// izquierda/derecha: no revela qué tecla se pulsó, solo de qué lado.
const LEFT_KEYS: [i32; 29] = [
    1, 2, 3, 4, 5, 6, 7, 15, 16, 17, 18, 19, 20, 29, 30, 31, 32, 33, 34, 41, 42, 44, 45, 46, 47,
    48, 56, 58, 125,
];

/// Devuelve el bit de pata para un keycode: `PAW_LEFT` si la tecla es de la
/// mitad izquierda, `PAW_RIGHT` en caso contrario.
#[must_use]
pub fn paw_for_keycode(keycode: i32) -> u8 {
    if LEFT_KEYS.contains(&keycode) {
        PAW_LEFT
    } else {
        PAW_RIGHT
    }
}

/// Aplica el volteo horizontal del gato (`mirror_x`) al conjunto de bits de pata:
/// izquierda ↔ derecha. Sin volteo, devuelve `paws` tal cual.
#[must_use]
pub fn apply_mirror(paws: u8, mirror: bool) -> u8 {
    if !mirror {
        return paws;
    }
    let mut out = 0u8;
    if paws & PAW_LEFT != 0 {
        out |= PAW_RIGHT;
    }
    if paws & PAW_RIGHT != 0 {
        out |= PAW_LEFT;
    }
    out
}

/// Deriva el índice de fotograma a partir de qué patas están "vivas" (dentro de
/// su ventana `keypress_duration`). Si ninguna lo está, devuelve `idle_frame`
/// (que la configuración puede fijar a cualquier valor 0–4).
#[must_use]
pub fn frame_from_paw_state(left_live: bool, right_live: bool, idle_frame: u8) -> u8 {
    match (left_live, right_live) {
        (true, true) => FRAME_BOTH_DOWN,
        (true, false) => FRAME_LEFT_DOWN,
        (false, true) => FRAME_RIGHT_DOWN,
        (false, false) => idle_frame,
    }
}

#[cfg(test)]
mod tests {
    // Casos portados verbatim de `tests/test_paw_frame.c` (fuente independiente).
    use super::*;

    #[test]
    fn combinaciones_de_patas_a_fotograma() {
        let idle = FRAME_BOTH_UP;
        assert_eq!(frame_from_paw_state(false, false, idle), FRAME_BOTH_UP);
        assert_eq!(frame_from_paw_state(true, false, idle), FRAME_LEFT_DOWN);
        assert_eq!(frame_from_paw_state(false, true, idle), FRAME_RIGHT_DOWN);
        assert_eq!(frame_from_paw_state(true, true, idle), FRAME_BOTH_DOWN);
    }

    #[test]
    fn mapeo_de_teclas_y_volteo() {
        assert_eq!(paw_for_keycode(30), PAW_LEFT); // 'A'
        assert_eq!(paw_for_keycode(38), PAW_RIGHT); // 'L'
        assert_eq!(apply_mirror(PAW_LEFT, true), PAW_RIGHT);
        assert_eq!(apply_mirror(PAW_RIGHT, true), PAW_LEFT);
        assert_eq!(apply_mirror(PAW_BOTH, true), PAW_BOTH);
        assert_eq!(apply_mirror(PAW_LEFT, false), PAW_LEFT); // sin volteo, intacto
    }

    #[test]
    fn idle_frame_configurable() {
        assert_eq!(
            frame_from_paw_state(false, false, FRAME_SLEEPING),
            FRAME_SLEEPING
        );
    }
}
