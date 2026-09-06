//! Lógica pura de la entrada de ratón (spec 0012). El proceso lector de input
//! traduce la actividad del ratón a un **bit de pata** con esto; ni la posición
//! del cursor ni los deltas se guardan ni se registran (solo "hubo actividad").

use crate::config::MousePaw;
use crate::paw::{PAW_LEFT, PAW_RIGHT};

/// ¿Toca emitir **un** golpecito por movimiento de ratón? Devuelve `true` cuando
/// ha pasado `interval_ms` desde el último golpecito de movimiento y hubo
/// desplazamiento acumulado (`|Δx|+|Δy|`) desde entonces.
///
/// Nunca un golpecito por evento `REL` (un ratón genera cientos por segundo):
/// el acumulador + intervalo los absorbe.
#[must_use]
pub fn mouse_motion_tick(accum_delta: i64, elapsed_ms: u64, interval_ms: u64) -> bool {
    accum_delta > 0 && elapsed_ms >= interval_ms
}

/// Bit de pata para un golpecito de ratón. `coin` solo se usa con
/// `MousePaw::Random` (lo aporta quien llama; `true` → derecha).
#[must_use]
pub fn mouse_paw_bit(paw: MousePaw, coin: bool) -> u8 {
    match paw {
        MousePaw::Left => PAW_LEFT,
        MousePaw::Right => PAW_RIGHT,
        MousePaw::Random => {
            if coin {
                PAW_RIGHT
            } else {
                PAW_LEFT
            }
        }
    }
}

/// Dirección de la mirada de la mascota hacia el cursor del ratón.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GazeDirection {
    #[default]
    Center,
    Left,
    Right,
    Up,
    Down,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

/// Marca (bit 7) que distingue un byte de mirada de un byte de pata.
pub const GAZE_FLAG: u8 = 0x80;

/// Convierte una dirección de mirada a su byte representativo en la tubería.
#[must_use]
pub fn gaze_to_byte(gaze: GazeDirection) -> u8 {
    GAZE_FLAG
        | match gaze {
            GazeDirection::Center => 0,
            GazeDirection::Left => 1,
            GazeDirection::Right => 2,
            GazeDirection::Up => 3,
            GazeDirection::Down => 4,
            GazeDirection::UpLeft => 5,
            GazeDirection::UpRight => 6,
            GazeDirection::DownLeft => 7,
            GazeDirection::DownRight => 8,
        }
}

/// Decodifica un byte recibido de la tubería como dirección de mirada, si lo es.
#[must_use]
pub fn gaze_from_byte(byte: u8) -> Option<GazeDirection> {
    if byte & GAZE_FLAG == 0 {
        return None;
    }
    Some(match byte & 0x7F {
        0 => GazeDirection::Center,
        1 => GazeDirection::Left,
        2 => GazeDirection::Right,
        3 => GazeDirection::Up,
        4 => GazeDirection::Down,
        5 => GazeDirection::UpLeft,
        6 => GazeDirection::UpRight,
        7 => GazeDirection::DownLeft,
        8 => GazeDirection::DownRight,
        _ => GazeDirection::Center,
    })
}

/// Calcula la dirección de mirada a partir de los desplazamientos acumulados `(dx, dy)`.
/// `threshold` define la zona muerta central antes de desviar la mirada.
#[must_use]
pub fn gaze_direction_from_delta(dx: i64, dy: i64, threshold: i64) -> GazeDirection {
    let t = threshold.max(1);
    let left = dx < -t;
    let right = dx > t;
    let up = dy < -t;
    let down = dy > t;

    match (left, right, up, down) {
        (true, _, true, _) => GazeDirection::UpLeft,
        (_, true, true, _) => GazeDirection::UpRight,
        (true, _, _, true) => GazeDirection::DownLeft,
        (_, true, _, true) => GazeDirection::DownRight,
        (true, _, _, _) => GazeDirection::Left,
        (_, true, _, _) => GazeDirection::Right,
        (_, _, true, _) => GazeDirection::Up,
        (_, _, _, true) => GazeDirection::Down,
        _ => GazeDirection::Center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_tick_respeta_intervalo_y_movimiento() {
        assert!(!mouse_motion_tick(0, 999, 50), "sin movimiento, no");
        assert!(!mouse_motion_tick(10, 40, 50), "aún no toca el intervalo");
        assert!(mouse_motion_tick(1, 50, 50), "intervalo justo + movimiento");
        assert!(mouse_motion_tick(200, 120, 50));
    }

    #[test]
    fn paw_bit_por_configuracion() {
        assert_eq!(mouse_paw_bit(MousePaw::Left, true), PAW_LEFT);
        assert_eq!(mouse_paw_bit(MousePaw::Right, false), PAW_RIGHT);
        assert_eq!(mouse_paw_bit(MousePaw::Random, true), PAW_RIGHT);
        assert_eq!(mouse_paw_bit(MousePaw::Random, false), PAW_LEFT);
    }

    #[test]
    fn gaze_direction_calculo_y_zona_muerta() {
        assert_eq!(gaze_direction_from_delta(0, 0, 30), GazeDirection::Center);
        assert_eq!(
            gaze_direction_from_delta(20, -20, 30),
            GazeDirection::Center
        );
        assert_eq!(gaze_direction_from_delta(-50, 0, 30), GazeDirection::Left);
        assert_eq!(gaze_direction_from_delta(50, 0, 30), GazeDirection::Right);
        assert_eq!(gaze_direction_from_delta(0, -50, 30), GazeDirection::Up);
        assert_eq!(gaze_direction_from_delta(0, 50, 30), GazeDirection::Down);
        assert_eq!(
            gaze_direction_from_delta(-50, -50, 30),
            GazeDirection::UpLeft
        );
        assert_eq!(
            gaze_direction_from_delta(50, -50, 30),
            GazeDirection::UpRight
        );
        assert_eq!(
            gaze_direction_from_delta(-50, 50, 30),
            GazeDirection::DownLeft
        );
        assert_eq!(
            gaze_direction_from_delta(50, 50, 30),
            GazeDirection::DownRight
        );
    }

    #[test]
    fn gaze_serializacion_ida_y_vuelta() {
        for gaze in [
            GazeDirection::Center,
            GazeDirection::Left,
            GazeDirection::Right,
            GazeDirection::Up,
            GazeDirection::Down,
            GazeDirection::UpLeft,
            GazeDirection::UpRight,
            GazeDirection::DownLeft,
            GazeDirection::DownRight,
        ] {
            let byte = gaze_to_byte(gaze);
            assert_eq!(gaze_from_byte(byte), Some(gaze));
        }
        assert_eq!(gaze_from_byte(PAW_LEFT), None);
        assert_eq!(gaze_from_byte(PAW_RIGHT), None);
    }
}
