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
}
