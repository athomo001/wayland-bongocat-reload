//! Máquina de estados de animación para temas de sprite sheet
//! (`theme_format = 3`, spec 0014 M2).
//!
//! Para el `classic` (5 SVG) el "estado" es solo un índice 0–4 que elige
//! `paw::frame_from_paw_state`; eso no cambia. Aquí se modela lo que un pet de
//! wayland-vpets necesita de más: **estados de bucle** (`Idle`, `Writing`,
//! `Sleep`, `Happy`…) cuyo fotograma avanza a los fps del estado, y **one-shots**
//! (`start_writing`, `end_writing`, `wake_up`) que se reproducen una vez y saltan
//! a su destino.
//!
//! Todo el tiempo entra por parámetro (`Instant`): la lógica es determinista y
//! testeable sin reloj real.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use bongocat_common::sheet::{InputModel, SheetTheme};

/// Estados conducibles de la animación (spec 0014 §5.2). Los `working`/`moving`
/// de wayland-vpets no tienen disparador en la v1: no se modelan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateId {
    Idle,
    Boring,
    StartWriting,
    Writing,
    EndWriting,
    Happy,
    Sleep,
    WakeUp,
    /// Poses de actividad del modelo "con manos" (bongo cat sobre sprite sheet).
    ActiveLeft,
    ActiveRight,
    ActiveBoth,
}

impl StateId {
    /// Traduce un nombre de estado del `theme.ini` (los 15 de wayland-vpets, más
    /// alias) a un `StateId`. `None` = estado que la v1 no conduce.
    #[must_use]
    pub fn from_theme_name(name: &str) -> Option<Self> {
        Some(match name {
            "idle" => Self::Idle,
            "boring" => Self::Boring,
            "start_writing" => Self::StartWriting,
            "writing" | "active" => Self::Writing,
            "end_writing" => Self::EndWriting,
            "happy" => Self::Happy,
            "sleep" | "asleep" => Self::Sleep,
            "wake_up" | "wakeup" => Self::WakeUp,
            "active_left" | "left_down" | "left-down" => Self::ActiveLeft,
            "active_right" | "right_down" | "right-down" => Self::ActiveRight,
            "active_both" | "both_down" | "both-down" => Self::ActiveBoth,
            _ => return None,
        })
    }

    /// Nombre corto y estable para diagnóstico (IPC `STATE`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Boring => "boring",
            Self::StartWriting => "start_writing",
            Self::Writing => "writing",
            Self::EndWriting => "end_writing",
            Self::Happy => "happy",
            Self::Sleep => "sleep",
            Self::WakeUp => "wake_up",
            Self::ActiveLeft => "active_left",
            Self::ActiveRight => "active_right",
            Self::ActiveBoth => "active_both",
        }
    }

    /// ¿Se reproduce una sola vez y luego transiciona?
    #[must_use]
    pub fn is_oneshot(self) -> bool {
        matches!(self, Self::StartWriting | Self::EndWriting | Self::WakeUp)
    }

    /// Estado al que salta un one-shot al terminar.
    #[must_use]
    fn oneshot_dest(self) -> StateId {
        match self {
            Self::StartWriting => Self::Writing,
            Self::EndWriting | Self::WakeUp => Self::Idle,
            other => other,
        }
    }

    /// ¿Cuenta como "hay actividad" (para decidir el puente one-shot)?
    #[must_use]
    fn is_active_like(self) -> bool {
        matches!(
            self,
            Self::Writing | Self::Happy | Self::ActiveLeft | Self::ActiveRight | Self::ActiveBoth
        )
    }

    /// Cadena de candidatos para la "regla de oro" (§5.2): si el estado pedido no
    /// está, se prueba el siguiente.
    #[must_use]
    fn fallback_chain(self) -> &'static [StateId] {
        use StateId::*;
        match self {
            Idle => &[Idle, Boring, Writing],
            Boring => &[Boring, Idle],
            Writing => &[Writing, Idle],
            ActiveLeft => &[ActiveLeft, Writing, Idle],
            ActiveRight => &[ActiveRight, Writing, Idle],
            ActiveBoth => &[ActiveBoth, Writing, Idle],
            Happy => &[Happy, Writing, Idle],
            Sleep => &[Sleep, Boring, Idle],
            StartWriting => &[StartWriting],
            EndWriting => &[EndWriting],
            WakeUp => &[WakeUp],
        }
    }
}

/// Cursor de la animación de un tema de sprite sheet + su caché de fotogramas.
pub struct SheetAnim {
    /// Fotogramas BGRA premultiplicados por estado, a la altura física actual.
    cache: BTreeMap<StateId, Vec<Vec<u8>>>,
    model: InputModel,
    fps: BTreeMap<StateId, u32>,
    default_fps: u32,
    /// Estado actual — **siempre** presente en `cache` (lo garantiza `enter`).
    state: StateId,
    frame: usize,
    last_advance: Instant,
    /// El one-shot actual ya llegó a su último fotograma.
    oneshot_done: bool,
}

impl SheetAnim {
    /// Construye la animación desde una caché con claves por **nombre** (la que
    /// devuelve `anim::build_sheet_cache`) y el `theme.ini` parseado.
    #[must_use]
    pub fn from_cache(
        cache_by_name: BTreeMap<String, Vec<Vec<u8>>>,
        sheet: &SheetTheme,
        now: Instant,
    ) -> Self {
        let mut cache: BTreeMap<StateId, Vec<Vec<u8>>> = BTreeMap::new();
        let mut fps: BTreeMap<StateId, u32> = BTreeMap::new();
        for (name, frames) in cache_by_name {
            let Some(id) = StateId::from_theme_name(&name) else {
                continue; // working/moving y demás: se ignoran en la v1
            };
            if frames.is_empty() {
                continue;
            }
            if let Some(st) = sheet.state(&name) {
                fps.insert(id, st.fps.max(1));
            }
            cache.insert(id, frames);
        }

        let mut anim = Self {
            cache,
            model: sheet.input_model,
            fps,
            default_fps: sheet.default_fps.max(1),
            state: StateId::Idle,
            frame: 0,
            last_advance: now,
            oneshot_done: false,
        };
        // Arranca en `Idle` (con su cadena de reserva). Si ningún estado del tema
        // es conducible en la v1 la caché queda vacía: el llamante lo detecta con
        // `is_empty()` y cae al gato embebido; no tocamos el cursor.
        if !anim.cache.is_empty() {
            anim.enter(StateId::Idle, now);
        }
        anim
    }

    /// ¿Tiene la caché algún fotograma? (el llamante ya lo comprueba, pero
    /// `enter` lo necesita para el último recurso).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Bytes BGRA del fotograma a pintar ahora mismo.
    #[must_use]
    pub fn current(&self) -> &[u8] {
        // `state` está en `cache` y `frame` acotado por `advance`.
        let frames = &self.cache[&self.state];
        &frames[self.frame.min(frames.len().saturating_sub(1))]
    }

    /// Estado y fotograma actuales, para el IPC `STATE`.
    #[must_use]
    pub fn debug_pos(&self) -> (&'static str, usize) {
        (self.state.label(), self.frame)
    }

    /// Cursor actual `(estado, fotograma)`, para trasladarlo a una animación
    /// recién reconstruida (ver [`SheetAnim::restore_cursor`]).
    #[must_use]
    pub fn cursor(&self) -> (StateId, usize) {
        (self.state, self.frame)
    }

    /// Cuándo hace falta el próximo `tick()` para no perder un cambio de
    /// fotograma: `None` si el estado actual es un **bucle de un solo
    /// fotograma** (nada que animar hasta una entrada externa — tecla, sueño,
    /// `happy`…); si no, el instante en que se cumple `1/fps` desde el último
    /// avance (bucles multi-fotograma **y** one-shots en curso, que necesitan
    /// esa misma llamada para completar su transición). Lo usa el llamante
    /// para bajar el ritmo de sondeo cuando no hace falta más (spec: CPU en
    /// reposo).
    #[must_use]
    pub fn next_wake(&self) -> Option<Instant> {
        let len = self.cache.get(&self.state).map_or(0, Vec::len);
        if !self.state.is_oneshot() && len <= 1 {
            return None;
        }
        let fps = self
            .fps
            .get(&self.state)
            .copied()
            .unwrap_or(self.default_fps)
            .max(1);
        let per = Duration::from_nanos(1_000_000_000 / u64::from(fps));
        Some(self.last_advance + per)
    }

    /// Reposiciona el cursor tras un re-rasterizado (cambio de `cat_height` /
    /// escala HiDPI / recarga de config): el pixel-art cambia de tamaño pero la
    /// máquina de estados no debería "saltar" a idle. Si `state` ya no existe en
    /// la nueva caché se resuelve por su cadena de reserva; el fotograma se
    /// acota. No revive un one-shot a medias: si el estado destino es one-shot,
    /// se deja marcado como terminado para que el siguiente `tick` transicione.
    pub fn restore_cursor(&mut self, state: StateId, frame: usize, now: Instant) {
        self.state = self.resolve(state);
        let len = self.cache[&self.state].len().max(1);
        self.frame = frame.min(len - 1);
        self.last_advance = now;
        self.oneshot_done = self.state.is_oneshot();
    }

    /// Resuelve `want` por su cadena de reserva al primer estado con fotogramas;
    /// si ninguno está, al primer estado de la caché. Devuelve un estado que
    /// **seguro** está en `cache`.
    #[must_use]
    fn resolve(&self, want: StateId) -> StateId {
        for &cand in want.fallback_chain() {
            if self.cache.contains_key(&cand) {
                return cand;
            }
        }
        *self
            .cache
            .keys()
            .next()
            .expect("caché de sprite sheet vacía")
    }

    /// Entra en `want` (tras resolver su reserva): fija estado, fotograma 0 y
    /// reinicia el temporizador.
    fn enter(&mut self, want: StateId, now: Instant) {
        self.state = self.resolve(want);
        self.frame = 0;
        self.last_advance = now;
        self.oneshot_done = false;
    }

    /// Estado de bucle deseado según las entradas (sin contar one-shots).
    #[must_use]
    fn desired_loop(
        &self,
        sleeping: bool,
        left: bool,
        right: bool,
        happy: bool,
        boring: bool,
    ) -> StateId {
        if sleeping {
            return StateId::Sleep;
        }
        // Actividad sostenida con KPM alto → `Happy` (spec 0014 §5.3), por
        // encima de las poses de tecleo; solo si el tema define ese estado.
        if happy && self.cache.contains_key(&StateId::Happy) {
            return StateId::Happy;
        }
        if left || right {
            return match self.model {
                // `left || right` es cierto aquí; el caso `(false, false)` no se da.
                InputModel::Hands if left && right => StateId::ActiveBoth,
                InputModel::Hands if left => StateId::ActiveLeft,
                InputModel::Hands => StateId::ActiveRight,
                InputModel::Activity => StateId::Writing,
            };
        }
        // Inactividad prolongada antes del sueño (spec §5.2): `boring` si el
        // tema lo trae, si no `idle` (la cadena de reserva lo resuelve igual).
        if boring && self.cache.contains_key(&StateId::Boring) {
            return StateId::Boring;
        }
        StateId::Idle
    }

    /// Avanza el cursor de fotograma si ha pasado el tiempo del estado. `wrap`:
    /// en bucle vuelve a 0; en one-shot se queda en el último y marca
    /// `oneshot_done`. Devuelve `true` si el fotograma visible cambió.
    fn advance_frame(&mut self, now: Instant, wrap: bool) -> bool {
        let fps = self
            .fps
            .get(&self.state)
            .copied()
            .unwrap_or(self.default_fps)
            .max(1);
        let per = Duration::from_nanos(1_000_000_000 / u64::from(fps));
        if now.duration_since(self.last_advance) < per {
            return false;
        }
        self.last_advance = now;
        let len = self.cache[&self.state].len().max(1);
        if len == 1 {
            // Un solo fotograma: nada que avanzar, pero un one-shot ya "terminó".
            self.oneshot_done = true;
            return false;
        }
        if wrap {
            self.frame = (self.frame + 1) % len;
        } else if self.frame + 1 < len {
            self.frame += 1;
        } else {
            self.oneshot_done = true;
            return false;
        }
        true
    }

    /// Avanza la máquina de estados un tick. `now` = reloj; el resto son las
    /// entradas ya calculadas por el llamante. Devuelve `true` si hay que
    /// redibujar (cambió estado o fotograma).
    pub fn tick(
        &mut self,
        now: Instant,
        sleeping: bool,
        left: bool,
        right: bool,
        happy: bool,
        boring: bool,
    ) -> bool {
        let before = (self.state, self.frame);

        if self.state.is_oneshot() {
            self.advance_frame(now, false);
            if self.oneshot_done {
                // Al terminar, salta a su destino y deja que el siguiente tick
                // re-evalúe las entradas.
                let dest = self.state.oneshot_dest();
                self.enter(dest, now);
            }
            return before != (self.state, self.frame);
        }

        let want = self.desired_loop(sleeping, left, right, happy, boring);
        if want != self.state && self.resolve(want) != self.state {
            // Cambio de estado de bucle: intenta un puente one-shot.
            let bridge = if self.state == StateId::Sleep {
                Some(StateId::WakeUp)
            } else if want.is_active_like() && !self.state.is_active_like() {
                Some(StateId::StartWriting)
            } else if !want.is_active_like() && self.state.is_active_like() {
                Some(StateId::EndWriting)
            } else {
                None
            };
            match bridge {
                // El puente solo se usa si el tema lo define; si no, corte directo.
                Some(b) if self.cache.contains_key(&b) => self.enter(b, now),
                _ => self.enter(want, now),
            }
        } else {
            self.advance_frame(now, true);
        }

        before != (self.state, self.frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_common::sheet::parse_sheet_ini;

    /// Caché de juguete: cada fotograma es un `Vec<u8>` de 1 byte con un valor
    /// distinto, para seguir la pista de qué se está mostrando.
    fn cache(spec: &[(&str, &[u8])]) -> BTreeMap<String, Vec<Vec<u8>>> {
        spec.iter()
            .map(|(name, bytes)| {
                (
                    (*name).to_string(),
                    bytes.iter().map(|b| vec![*b]).collect(),
                )
            })
            .collect()
    }

    const INI: &str = "\
frame_w = 4
frame_h = 4
default_fps = 10
input_model = activity
state_idle_row = 1
state_idle_frames = 2
state_writing_row = 2
state_writing_frames = 3
state_writing_fps = 20
state_sleep_row = 3
state_sleep_frames = 1
state_start_writing_row = 4
state_start_writing_frames = 1
";

    fn advance(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn bucle_idle_hace_wrap_a_su_fps() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        let mut a = SheetAnim::from_cache(
            cache(&[("idle", &[0, 1]), ("writing", &[10, 11, 12])]),
            &sheet,
            t0,
        );
        assert_eq!(a.current(), &[0]);
        // idle a 10 fps -> 100 ms/frame. Antes de 100 ms no cambia.
        assert!(!a.tick(advance(t0, 50), false, false, false, false, false));
        assert_eq!(a.current(), &[0]);
        assert!(a.tick(advance(t0, 120), false, false, false, false, false));
        assert_eq!(a.current(), &[1]);
        assert!(a.tick(advance(t0, 240), false, false, false, false, false));
        assert_eq!(a.current(), &[0], "wrap");
    }

    #[test]
    fn one_shot_start_writing_se_reproduce_y_salta_a_writing() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        let mut a = SheetAnim::from_cache(
            cache(&[
                ("idle", &[0, 1]),
                ("start_writing", &[5]),
                ("writing", &[10, 11, 12]),
            ]),
            &sheet,
            t0,
        );
        // Actividad: entra en el puente start_writing.
        a.tick(advance(t0, 10), false, true, false, false, false);
        assert_eq!(a.debug_pos().0, "start_writing");
        assert_eq!(a.current(), &[5]);
        // start_writing tiene 1 frame -> al siguiente tick con tiempo, termina y
        // salta a writing.
        a.tick(advance(t0, 200), false, true, false, false, false);
        assert_eq!(a.debug_pos().0, "writing");
        assert_eq!(a.current(), &[10]);
        // writing a 20 fps -> 50 ms/frame; hace bucle.
        a.tick(advance(t0, 260), false, true, false, false, false);
        assert_eq!(a.current(), &[11]);
    }

    #[test]
    fn sin_puente_va_directo_al_bucle() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        let mut a = SheetAnim::from_cache(
            cache(&[("idle", &[0, 1]), ("writing", &[10, 11])]),
            &sheet,
            t0,
        );
        a.tick(advance(t0, 10), false, true, false, false, false);
        assert_eq!(
            a.debug_pos().0,
            "writing",
            "no hay start_writing -> directo"
        );
    }

    #[test]
    fn dormir_manda_sobre_la_actividad_y_cae_al_fallback() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        // Sin estado 'sleep' ni 'boring': Sleep -> fallback -> idle.
        let mut a = SheetAnim::from_cache(
            cache(&[("idle", &[0, 1]), ("writing", &[10, 11])]),
            &sheet,
            t0,
        );
        a.tick(advance(t0, 10), true, true, true, false, false);
        assert_eq!(a.debug_pos().0, "idle", "Sleep ausente -> idle");
    }

    #[test]
    fn modelo_activity_ignora_izquierda_derecha() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI); // input_model = activity
        let mut a = SheetAnim::from_cache(
            cache(&[
                ("idle", &[0]),
                ("writing", &[10, 11]),
                ("active_left", &[20]),
            ]),
            &sheet,
            t0,
        );
        a.tick(advance(t0, 10), false, true, false, false, false);
        assert_eq!(
            a.debug_pos().0,
            "writing",
            "activity: pata izq -> writing, no active_left"
        );
    }

    #[test]
    fn restore_cursor_conserva_estado_y_acota_frame() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        let mut a = SheetAnim::from_cache(
            cache(&[("idle", &[0, 1]), ("writing", &[10, 11, 12])]),
            &sheet,
            t0,
        );
        // Llévalo a writing, frame 2.
        a.tick(advance(t0, 10), false, true, false, false, false);
        a.tick(advance(t0, 60), false, true, false, false, false);
        a.tick(advance(t0, 120), false, true, false, false, false);
        assert_eq!(a.debug_pos(), ("writing", 2));

        // Simula el re-rasterizado: caché nueva (writing ahora con 2 frames).
        let mut b =
            SheetAnim::from_cache(cache(&[("idle", &[0]), ("writing", &[20, 21])]), &sheet, t0);
        let (st, fr) = a.cursor();
        b.restore_cursor(st, fr, t0);
        assert_eq!(b.debug_pos(), ("writing", 1), "frame 2 -> acotado a len-1");

        // Estado ausente en la caché nueva -> cadena de reserva.
        let mut c = SheetAnim::from_cache(cache(&[("idle", &[9])]), &sheet, t0);
        c.restore_cursor(StateId::Writing, 5, t0);
        assert_eq!(c.debug_pos(), ("idle", 0), "writing ausente -> idle");
    }

    #[test]
    fn modelo_hands_usa_las_poses_de_pata() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(&INI.replace("input_model = activity", "input_model = hands"));
        let mut a = SheetAnim::from_cache(
            cache(&[
                ("idle", &[0]),
                ("active_left", &[20]),
                ("active_right", &[21]),
                ("active_both", &[22]),
            ]),
            &sheet,
            t0,
        );
        a.tick(advance(t0, 10), false, true, false, false, false);
        assert_eq!(a.current(), &[20], "hands: pata izq -> active_left");
        a.tick(advance(t0, 20), false, true, true, false, false);
        assert_eq!(a.current(), &[22], "ambas -> active_both");
    }

    #[test]
    fn happy_manda_sobre_el_tecleo_si_el_tema_lo_trae() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        let mut a = SheetAnim::from_cache(
            cache(&[("idle", &[0]), ("writing", &[10, 11]), ("happy", &[30, 31])]),
            &sheet,
            t0,
        );
        // Tecleando (pata izq) pero con `happy` -> gana `happy`.
        a.tick(advance(t0, 10), false, true, false, true, false);
        assert_eq!(
            a.debug_pos().0,
            "happy",
            "KPM alto -> happy aunque se teclee"
        );
        // Sin `happy`, el mismo tecleo -> writing.
        a.tick(advance(t0, 20), false, true, false, false, false);
        assert_eq!(a.debug_pos().0, "writing");
    }

    #[test]
    fn happy_sin_estado_en_el_tema_cae_a_la_reserva() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI); // sin `happy`
        let mut a =
            SheetAnim::from_cache(cache(&[("idle", &[0]), ("writing", &[10, 11])]), &sheet, t0);
        a.tick(advance(t0, 10), false, false, false, true, false);
        assert_eq!(
            a.debug_pos().0,
            "idle",
            "sin estado happy, `happy=true` no hace nada"
        );
    }

    #[test]
    fn boring_entra_por_inactividad_si_el_tema_lo_trae() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        let mut a = SheetAnim::from_cache(
            cache(&[("idle", &[0]), ("writing", &[10]), ("boring", &[7, 8])]),
            &sheet,
            t0,
        );
        // Sin actividad y sin `boring` → idle.
        a.tick(advance(t0, 10), false, false, false, false, false);
        assert_eq!(a.debug_pos().0, "idle");
        // `boring` activo → boring; el tecleo lo saca.
        a.tick(advance(t0, 20), false, false, false, false, true);
        assert_eq!(a.debug_pos().0, "boring");
        a.tick(advance(t0, 30), false, true, false, false, true);
        assert_eq!(a.debug_pos().0, "writing", "el tecleo manda sobre boring");

        // Un tema sin estado `boring`: `boring=true` no cambia nada (idle).
        let mut b = SheetAnim::from_cache(cache(&[("idle", &[0]), ("writing", &[10])]), &sheet, t0);
        b.tick(advance(t0, 10), false, false, false, false, true);
        assert_eq!(b.debug_pos().0, "idle");
    }

    #[test]
    fn next_wake_none_si_el_bucle_es_de_un_solo_fotograma() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        // `idle` con un único fotograma: nada que animar sin una entrada externa.
        let a = SheetAnim::from_cache(cache(&[("idle", &[0]), ("writing", &[10])]), &sheet, t0);
        assert_eq!(a.next_wake(), None);
    }

    #[test]
    fn next_wake_bucle_multi_fotograma_es_1_sobre_fps() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI); // idle sin fps propio -> default_fps = 10
        let a = SheetAnim::from_cache(
            cache(&[("idle", &[0, 1]), ("writing", &[10, 11])]),
            &sheet,
            t0,
        );
        assert_eq!(
            a.next_wake(),
            Some(t0 + Duration::from_millis(100)),
            "10 fps -> 100 ms desde el último avance (= la entrada en el estado)"
        );
    }

    #[test]
    fn next_wake_one_shot_en_curso_no_se_pierde() {
        let t0 = Instant::now();
        let sheet = parse_sheet_ini(INI);
        let mut a = SheetAnim::from_cache(
            cache(&[
                ("idle", &[0, 1]),
                ("start_writing", &[5]),
                ("writing", &[10, 11, 12]),
            ]),
            &sheet,
            t0,
        );
        let t1 = advance(t0, 10);
        a.tick(t1, false, true, false, false, false);
        assert_eq!(a.debug_pos().0, "start_writing");
        // Aunque tenga un solo fotograma, es un one-shot: hace falta una
        // llamada más para completar la transición a `writing`.
        assert_eq!(
            a.next_wake(),
            Some(t1 + Duration::from_millis(100)),
            "start_writing sin fps propio -> default_fps = 10 -> 100 ms"
        );
    }
}
