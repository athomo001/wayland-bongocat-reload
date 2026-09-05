//! Contador de **teclas por minuto** en ventana deslizante para el estado
//! `happy` de los temas de sprite sheet (spec 0014 §5.7, hito M6).
//!
//! Solo cuenta *eventos*: guarda el instante de cada pulsación y descarta los
//! de hace más de 60 s. No hay identidad de tecla ni marcas de tiempo
//! persistidas — es el "contador entero en ventana" que pide la spec 0013 §1.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Longitud de la ventana: "por minuto".
const WINDOW: Duration = Duration::from_secs(60);
/// Cota dura de memoria (a ~10 000 ppm ya es imposible de teclear; protege ante
/// un dispositivo evdev que dispare eventos en bucle).
const MAX_HITS: usize = 10_000;

/// Ventana deslizante de pulsaciones.
#[derive(Default)]
pub struct Kpm {
    hits: VecDeque<Instant>,
}

impl Kpm {
    #[must_use]
    pub fn new() -> Self {
        Self {
            hits: VecDeque::new(),
        }
    }

    /// Registra una pulsación en `now` y poda lo que ya salió de la ventana.
    pub fn hit(&mut self, now: Instant) {
        self.hits.push_back(now);
        self.trim(now);
    }

    /// Pulsaciones en los últimos 60 s (podando primero por si no ha habido
    /// ninguna pulsación reciente y el conteo debe decaer).
    pub fn per_minute(&mut self, now: Instant) -> usize {
        self.trim(now);
        self.hits.len()
    }

    /// Número de pulsaciones ocurridas en los últimos `window` instantes.
    #[must_use]
    pub fn hits_within(&mut self, window: Duration, now: Instant) -> usize {
        self.trim(now);
        self.hits
            .iter()
            .rev()
            .take_while(|&&t| now.saturating_duration_since(t) <= window)
            .count()
    }

    fn trim(&mut self, now: Instant) {
        while self
            .hits
            .front()
            .is_some_and(|&t| now.saturating_duration_since(t) > WINDOW)
        {
            self.hits.pop_front();
        }
        while self.hits.len() > MAX_HITS {
            self.hits.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuenta_solo_dentro_de_la_ventana() {
        let mut k = Kpm::new();
        let t0 = Instant::now();
        // 5 pulsaciones "hace 90 s" y 3 "ahora".
        for _ in 0..5 {
            k.hit(t0);
        }
        let now = t0 + Duration::from_secs(90);
        for _ in 0..3 {
            k.hit(now);
        }
        assert_eq!(k.per_minute(now), 3, "las de hace 90 s ya no cuentan");
        // Otros 30 s después: las 3 siguen dentro (llevan 30 s).
        assert_eq!(k.per_minute(now + Duration::from_secs(30)), 3);
        // A los 61 s de la última, cero.
        assert_eq!(k.per_minute(now + Duration::from_secs(61)), 0);
    }

    #[test]
    fn cuenta_rafagas_en_ventana_corta() {
        let mut k = Kpm::new();
        let t0 = Instant::now();
        k.hit(t0);
        k.hit(t0 + Duration::from_millis(200));
        assert_eq!(
            k.hits_within(Duration::from_millis(500), t0 + Duration::from_millis(200)),
            2
        );
        assert_eq!(
            k.hits_within(Duration::from_millis(100), t0 + Duration::from_millis(200)),
            1
        );
        assert_eq!(
            k.hits_within(Duration::from_millis(500), t0 + Duration::from_millis(800)),
            0
        );
    }

    #[test]
    fn respeta_la_cota_de_memoria() {
        let mut k = Kpm::new();
        let t = Instant::now();
        for _ in 0..(MAX_HITS + 500) {
            k.hit(t);
        }
        assert!(k.hits.len() <= MAX_HITS);
    }
}
