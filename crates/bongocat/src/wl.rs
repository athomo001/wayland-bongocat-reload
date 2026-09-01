//! Wayland: rebanadas 1–2 de la Fase 0.5.
//!
//! - R1: crear la layer-surface con `smithay-client-toolkit` y pintarla de un
//!   color sólido, anclada al borde configurado con el alto configurado.
//! - R2: mover el bucle a `calloop` (el mismo que luego llevará Wayland + timer
//!   de animación + input + watcher), `Ctrl+C` limpio y un timer que "late" el
//!   color cada segundo para confirmar el camino timer → redibujado.
//!
//! Portará poco a poco lo que hoy hace `src/platform/wayland.c`.

use std::error::Error;
use std::time::{Duration, Instant};

use bongocat_common::config::{Align, Config, Position};
use bongocat_common::paw::{self, apply_mirror, frame_from_paw_state, FRAME_SLEEPING};
use calloop::signals::{Signal, Signals};
use calloop::timer::{TimeoutAction, Timer};
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use crate::anim::{self, Frames};
use crate::input;

/// Arranca el overlay: conecta a Wayland, crea la layer-surface con la posición
/// y el alto de `config`, y corre el bucle `calloop`. Devuelve cuando el
/// compositor cierra la surface o se recibe SIGINT/SIGTERM.
pub fn run_overlay(config: &Config) -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh: QueueHandle<State> = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("falta wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|e| format!("el compositor no soporta wlr-layer-shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("falta wl_shm: {e}"))?;

    // Alto lógico de la barra; el ancho lo decide el compositor (anclada a los
    // dos lados). Valor inicial de fallback hasta el primer `configure`.
    let height: u32 = config.overlay_height.max(1) as u32;
    let width: u32 = config.screen_width.max(1) as u32;

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Top,
        Some("bongocat"),
        None, // salida: la que elija el compositor (multi-monitor viene después)
    );

    let anchor = match config.overlay_position {
        Position::Top => Anchor::TOP,
        Position::Bottom => Anchor::BOTTOM,
    } | Anchor::LEFT
        | Anchor::RIGHT;
    layer.set_anchor(anchor);
    layer.set_size(0, height);
    layer.set_exclusive_zone(-1); // no reservar espacio: el overlay flota
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let pool = SlotPool::new((width * height * 4) as usize, &shm)?;

    // Rasteriza los 5 fotogramas del gato a la altura configurada.
    let frames = anim::rasterize(config.cat_height.max(1) as u32)?;
    eprintln!(
        "bongocat: {} fotogramas rasterizados a {}x{}",
        5, frames.w, frames.h
    );

    let now = Instant::now();
    let mut state = State {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        qh: qh.clone(),
        shm,
        pool,
        layer,
        config: config.clone(),
        frames,
        frame: config.idle_frame.clamp(0, 4) as u8,
        left_hold_until: now,
        right_hold_until: now,
        last_activity: now,
        width,
        height,
        exit: false,
    };

    // ── Bucle de eventos ────────────────────────────────────────────────────
    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let lh = event_loop.handle();

    WaylandSource::new(conn, event_queue)
        .insert(lh.clone())
        .map_err(|e| format!("no se pudo insertar la fuente Wayland en calloop: {e}"))?;

    // Ctrl+C / kill → salir limpio.
    lh.insert_source(
        Signals::new(&[Signal::SIGINT, Signal::SIGTERM])?,
        |_, _, state| {
            eprintln!("bongocat: señal recibida, saliendo");
            state.exit = true;
        },
    )?;

    // Lector(es) de teclado → canal de bits de pata.
    let (tx, rx) = calloop::channel::channel::<u8>();
    input::spawn_readers(&config.keyboard_devices, &tx);
    lh.insert_source(rx, |ev, _, state| {
        if let calloop::channel::Event::Msg(bit) = ev {
            state.on_paw(bit);
        }
    })?;

    // Tick de animación a ritmo de FPS: recalcula el fotograma y redibuja si
    // cambió. TODO: bajar a un tick lento cuando no hay actividad (idle ~0% CPU).
    let frame_dt = Duration::from_millis(1000 / u64::from(config.fps.clamp(1, 120) as u32));
    lh.insert_source(Timer::from_duration(frame_dt), move |_, _, state| {
        state.tick();
        TimeoutAction::ToDuration(frame_dt)
    })?;

    eprintln!("bongocat: barra {width}x{height} anclada; Ctrl+C para salir");
    while !state.exit {
        event_loop.dispatch(Some(Duration::from_millis(500)), &mut state)?;
    }
    Ok(())
}

struct State {
    registry_state: RegistryState,
    output_state: OutputState,
    qh: QueueHandle<State>,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    config: Config,
    frames: Frames,
    /// Fotograma actual (0–4), lo decide la máquina de estados.
    frame: u8,
    /// Instantes hasta los que cada pata sigue "bajada".
    left_hold_until: Instant,
    right_hold_until: Instant,
    /// Última pulsación (para el reposo por inactividad).
    last_activity: Instant,
    width: u32,
    height: u32,
    exit: bool,
}

impl State {
    /// Posición del gato en la barra, en píxeles (rebanada 3: sin HiDPI todavía).
    /// Porta el cálculo de `draw_bar` de `src/platform/wayland.c`.
    fn cat_origin(&self) -> (i32, i32) {
        let (bw, bh) = (self.width as i32, self.height as i32);
        let (cw, ch) = (self.frames.w as i32, self.frames.h as i32);
        let y = (bh - ch) / 2 + self.config.cat_y_offset;
        let x = match self.config.cat_align {
            Align::Center => (bw - cw) / 2 + self.config.cat_x_offset,
            Align::Left => self.config.cat_x_offset,
            Align::Right => bw - cw - self.config.cat_x_offset,
        };
        (x, y)
    }

    /// Llega un bit de pata del lector de teclado: extiende la ventana de esa
    /// pata y marca actividad. Porta `anim_press_paw` / `anim_take_pending_paws`.
    fn on_paw(&mut self, bit: u8) {
        let bit = apply_mirror(bit, self.config.mirror_x);
        let dur = Duration::from_millis(self.config.keypress_duration.max(0) as u64);
        let now = Instant::now();
        if bit & paw::PAW_LEFT != 0 {
            self.left_hold_until = now + dur;
        }
        if bit & paw::PAW_RIGHT != 0 {
            self.right_hold_until = now + dur;
        }
        self.last_activity = now;
        self.tick(); // respuesta inmediata, no hasta el siguiente tick
    }

    /// Recalcula qué fotograma toca y redibuja si cambió. Porta
    /// `anim_select_frame`. (El reposo por horario `enable_scheduled_sleep`
    /// llega en una rebanada posterior.)
    fn tick(&mut self) {
        let now = Instant::now();
        let idle_sleep = self.config.idle_sleep_timeout_sec > 0
            && now.duration_since(self.last_activity).as_secs()
                >= self.config.idle_sleep_timeout_sec as u64;

        let next = if idle_sleep {
            FRAME_SLEEPING
        } else {
            let left = now < self.left_hold_until;
            let right = now < self.right_hold_until;
            frame_from_paw_state(left, right, self.config.idle_frame.clamp(0, 4) as u8)
        };

        if next != self.frame {
            self.frame = next;
            self.draw();
        }
    }

    /// Limpia el buffer (transparente) y dibuja el fotograma actual del gato.
    fn draw(&mut self) {
        let (w, h) = (self.width.max(1), self.height.max(1));
        let stride = w as i32 * 4;
        let needed = (w * h * 4) as usize;
        // Se calculan antes de tocar el pool (evita conflictos de préstamo).
        let (ox, oy) = self.cat_origin();
        let (fw, fh) = (self.frames.w, self.frames.h);
        let frame = self.frames.frame(self.frame as usize);

        if let Err(e) = self.pool.resize(needed.max(1)) {
            eprintln!("bongocat: no se pudo redimensionar el pool a {needed}: {e}");
            return;
        }
        let (buffer, canvas) =
            match self
                .pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
            {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("bongocat: create_buffer {w}x{h} falló: {e}");
                    return;
                }
            };

        canvas.fill(0); // fondo transparente
        anim::blit_over(canvas, (w, h), frame, (fw, fh), (ox, oy));

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w as i32, h as i32);
        if let Err(e) = buffer.attach_to(surface) {
            eprintln!("bongocat: attach falló: {e}");
        }
        surface.frame(&self.qh, surface.clone());
        self.layer.commit();
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        eprintln!("bongocat: el compositor cerró la surface");
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // El compositor nos da el tamaño definitivo (0 = "elige tú").
        let (cw, ch) = configure.new_size;
        eprintln!("bongocat: configure new_size=({cw},{ch})");
        if cw != 0 {
            self.width = cw;
        }
        if ch != 0 {
            self.height = ch;
        }
        self.draw();
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _c: &Connection,
        _qh: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _new: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _c: &Connection,
        _qh: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _t: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _c: &Connection,
        qh: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Sin animación aún: no repintamos en cada frame.
        let _ = qh;
    }
    fn surface_enter(
        &mut self,
        _c: &Connection,
        _qh: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _o: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _c: &Connection,
        _qh: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _o: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _c: &Connection, _qh: &QueueHandle<Self>, _o: wl_output::WlOutput) {}
    fn update_output(&mut self, _c: &Connection, _qh: &QueueHandle<Self>, _o: wl_output::WlOutput) {
    }
    fn output_destroyed(
        &mut self,
        _c: &Connection,
        _qh: &QueueHandle<Self>,
        _o: wl_output::WlOutput,
    ) {
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_layer!(State);
delegate_registry!(State);
