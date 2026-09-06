//! Reposo programado (`enable_scheduled_sleep`): decide si "ahora" cae dentro de
//! la franja horaria de reposo. Lógica pura; la hora local la aporta quien
//! llama (el overlay, con `localtime_r`).

/// ¿La hora local, en minutos desde medianoche (`0..1440`), cae dentro de la
/// franja de reposo `[begin, end)`?
///
/// Porta `anim_is_sleep_time` de `src/graphics/animation.c`:
///   * `begin == end`  → siempre (franja "vacía"; la validación de config ya la
///     desactiva, se mantiene por paridad).
///   * `begin <  end`  → franja diurna: `now` en `[begin, end)`.
///   * `begin >  end`  → franja que cruza medianoche: `now >= begin || now < end`.
///
/// El inicio es inclusivo y el fin exclusivo: a las `sleep_begin` en punto ya se
/// duerme; a las `sleep_end` en punto ya se está despierto.
#[must_use]
pub fn is_scheduled_sleep(now_minutes: i32, begin_minutes: i32, end_minutes: i32) -> bool {
    if begin_minutes == end_minutes {
        return true;
    }
    if begin_minutes < end_minutes {
        now_minutes >= begin_minutes && now_minutes < end_minutes
    } else {
        now_minutes >= begin_minutes || now_minutes < end_minutes
    }
}

#[cfg(test)]
mod tests {
    use super::is_scheduled_sleep;

    const H: fn(i32, i32) -> i32 = |h, m| h * 60 + m;

    #[test]
    fn franja_diurna() {
        let (b, e) = (H(10, 0), H(22, 0));
        assert!(!is_scheduled_sleep(H(9, 59), b, e));
        assert!(is_scheduled_sleep(H(10, 0), b, e), "inicio inclusivo");
        assert!(is_scheduled_sleep(H(15, 30), b, e));
        assert!(!is_scheduled_sleep(H(22, 0), b, e), "fin exclusivo");
        assert!(!is_scheduled_sleep(H(23, 0), b, e));
    }

    #[test]
    fn franja_nocturna_cruza_medianoche() {
        let (b, e) = (H(22, 0), H(6, 0));
        assert!(is_scheduled_sleep(H(22, 0), b, e), "inicio inclusivo");
        assert!(is_scheduled_sleep(H(23, 30), b, e));
        assert!(is_scheduled_sleep(H(0, 0), b, e));
        assert!(is_scheduled_sleep(H(5, 59), b, e));
        assert!(!is_scheduled_sleep(H(6, 0), b, e), "fin exclusivo");
        assert!(!is_scheduled_sleep(H(12, 0), b, e));
    }

    #[test]
    fn begin_igual_a_end_es_siempre() {
        assert!(is_scheduled_sleep(0, H(3, 0), H(3, 0)));
        assert!(is_scheduled_sleep(H(15, 0), H(3, 0), H(3, 0)));
    }
}
