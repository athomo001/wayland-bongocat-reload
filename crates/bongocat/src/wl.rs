//! Wayland: el overlay (rebanadas 1–6 de la Fase 0.5).
//!
//! Layer-surface con `smithay-client-toolkit`, bucle `calloop`, región de
//! entrada vacía (click-through), rasterizado del gato, lector de teclado,
//! máquina de estados, fondo con `overlay_opacity`, espejo, y `--watch-config`
//! (recarga en caliente).
//!
//! Auto-ocultar en pantalla completa vía `zwlr_foreign_toplevel_management_v1`
//! (wlroots, KWin) o `zcosmic_toplevel_info_v1` (COSMIC) — ver `cosmic.rs`.
//! HiDPI con `wp_viewporter` + `wp_fractional_scale_v1` (búfer físico → capa
//! lógica). Pendiente: multi-monitor. Porta `src/platform/wayland.c`.

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
    delegate_noop, event_created_child,
    globals::registry_queue_init,
    protocol::{wl_output, wl_region, wl_shm, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self as ftl_handle, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self as ftl_mgr, ZwlrForeignToplevelManagerV1},
};

use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;

use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self as ext_handle, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self as ext_list, ExtForeignToplevelListV1},
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self as fs_v1, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use bongocat_common::scale::{scale_offset_120, scale_size_120};

use crate::anim::{self, Frames};
use crate::cosmic::{
    zcosmic_toplevel_handle_v1::{self as cosmic_handle, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{self as cosmic_info, ZcosmicToplevelInfoV1},
};
use crate::{input, input_child, watch};

/// Valores del enum `state` de `zwlr_foreign_toplevel_handle_v1` (y del enum
/// homónimo de `zcosmic_toplevel_handle_v1`: mismos números).
const TOPLEVEL_STATE_ACTIVATED: u32 = 2;
const TOPLEVEL_STATE_FULLSCREEN: u32 = 3;

/// Interpreta el array `state` de un toplevel (u32 en orden nativo) y devuelve
/// `(fullscreen, activated)`. Común a los protocolos wlr y COSMIC.
fn parse_toplevel_state(arr: &[u8]) -> (bool, bool) {
    let mut fullscreen = false;
    let mut activated = false;
    for chunk in arr.chunks_exact(4) {
        let v = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        activated |= v == TOPLEVEL_STATE_ACTIVATED;
        fullscreen |= v == TOPLEVEL_STATE_FULLSCREEN;
    }
    (fullscreen, activated)
}

/// Estado de un toplevel que nos interesa para ocultar el gato.
#[derive(Default)]
struct TopInfo {
    fullscreen: bool,
    activated: bool,
}

/// Sonda de salidas: cola de eventos aparte para resolver `--monitor NOMBRE`
/// antes de crear la layer-surface (SCTK reclama `wl_output` en la principal).
#[derive(Default)]
struct OutputProbe {
    outputs: Vec<(wl_output::WlOutput, Option<String>)>,
}

impl Dispatch<wl_output::WlOutput, ()> for OutputProbe {
    fn event(
        state: &mut Self,
        proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            if let Some(slot) = state.outputs.iter_mut().find(|(o, _)| o == proxy) {
                slot.1 = Some(name);
            }
        }
    }
}

/// Anclaje de la layer-surface para una posición del overlay (siempre a los dos
/// lados horizontales).
fn anchor_for(pos: Position) -> Anchor {
    let edge = match pos {
        Position::Top => Anchor::TOP,
        Position::Bottom => Anchor::BOTTOM,
    };
    edge | Anchor::LEFT | Anchor::RIGHT
}

/// Duración entre fotogramas a partir de los FPS configurados.
fn frame_dt_from_fps(fps: i32) -> Duration {
    Duration::from_millis(1000 / u64::from(fps.clamp(1, 120) as u32))
}

/// Hora local en minutos desde medianoche (`0..1440`), o `None` si falla.
/// Equivale a `time()` + `localtime_r()` de `anim_is_sleep_time` (respeta `$TZ`
/// y `/etc/localtime`). La lógica de la franja vive en `bongocat_common::sleep`.
#[allow(unsafe_code)] // FFI puntual y documentado: time + localtime_r
fn local_now_minutes() -> Option<i32> {
    // SAFETY: `time(NULL)` devuelve el epoch actual; `localtime_r` escribe en un
    // `tm` de pila propio y devuelve ese mismo puntero (o NULL en error).
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return None;
        }
        Some(tm.tm_hour * 60 + tm.tm_min)
    }
}

/// Arranca el overlay: conecta a Wayland, crea la layer-surface y corre el bucle
/// `calloop`. Con `watch_config`, vigila `config_path` y recarga en caliente.
/// Devuelve cuando el compositor cierra la surface o llega SIGINT/SIGTERM.
pub fn run_overlay(
    config: &Config,
    config_path: Option<PathBuf>,
    watch_config: bool,
    no_toplevel: bool,
    input: input::Isolated,
    target_output_name: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh: QueueHandle<State> = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("falta wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|e| format!("el compositor no soporta wlr-layer-shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("falta wl_shm: {e}"))?;

    // HiDPI (opcional): `wp_viewporter` + `wp_fractional_scale_v1`. Con ambos, el
    // búfer se rasteriza a píxeles físicos y `wp_viewport` lo mapea al tamaño
    // lógico de la capa. Sin `viewporter` no se escala (búfer = tamaño lógico).
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fs_mgr = globals
        .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
        .ok();
    match (viewporter.is_some(), fs_mgr.is_some()) {
        (true, true) => eprintln!("bongocat: HiDPI: fractional-scale + viewporter"),
        (true, false) => eprintln!("bongocat: HiDPI: viewporter (escala entera de la salida)"),
        _ => eprintln!("bongocat: sin viewporter; sin escalado HiDPI"),
    }

    // Detección de pantalla completa (opcional). Se prueba primero el protocolo
    // wlr (sway, Hyprland, river, KWin); si no está, el camino COSMIC:
    // `ext-foreign-toplevel-list-v1` (la lista) + `zcosmic_toplevel_info_v1` v2
    // (el estado de cada uno). Si no hay ninguno, el gato no se auto-oculta.
    let ftl_mgr = if no_toplevel {
        None
    } else {
        globals
            .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ())
            .ok()
    };
    let (ext_list, cosmic_info) = if no_toplevel {
        eprintln!("bongocat: --no-toplevel: sin detección de pantalla completa");
        (None, None)
    } else if ftl_mgr.is_none() {
        let list = globals
            .bind::<ExtForeignToplevelListV1, _, _>(&qh, 1..=1, ())
            .ok();
        let info = globals
            .bind::<ZcosmicToplevelInfoV1, _, _>(&qh, 2..=3, ())
            .ok();
        // Solo sirve si están los dos: la lista da los handles y el "info" el estado.
        if list.is_some() && info.is_some() {
            (list, info)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };
    match (ftl_mgr.is_some(), cosmic_info.is_some()) {
        (true, _) => eprintln!("bongocat: auto-ocultar en pantalla completa: activo (wlr)"),
        (_, true) => eprintln!("bongocat: auto-ocultar en pantalla completa: activo (cosmic)"),
        _ if !no_toplevel => {
            eprintln!("bongocat: sin protocolo de toplevels; el gato no se auto-ocultará");
        }
        _ => {}
    }

    // Multi-monitor: si se pidió una salida por nombre, la resolvemos con una
    // cola de eventos aparte (SCTK reclama `wl_output` en la principal, no
    // podemos tener dos `Dispatch<WlOutput>` para el mismo estado).
    let target_output: Option<wl_output::WlOutput> = match &target_output_name {
        Some(name) => {
            let mut probe_q = conn.new_event_queue::<OutputProbe>();
            let probe_qh = probe_q.handle();
            let mut probe = OutputProbe::default();
            for g in globals.contents().clone_list() {
                if g.interface == "wl_output" {
                    let o = globals.registry().bind::<wl_output::WlOutput, _, _>(
                        g.name,
                        g.version.min(4),
                        &probe_qh,
                        (),
                    );
                    probe.outputs.push((o, None));
                }
            }
            // Dos roundtrips: el evento `name` puede llegar en el segundo.
            probe_q.roundtrip(&mut probe)?;
            probe_q.roundtrip(&mut probe)?;
            match probe
                .outputs
                .into_iter()
                .find(|(_, n)| n.as_deref() == Some(name.as_str()))
            {
                Some((o, _)) => {
                    eprintln!("bongocat: overlay en la salida '{name}'");
                    Some(o)
                }
                None => {
                    eprintln!("bongocat: salida '{name}' no encontrada; uso la de por defecto");
                    None
                }
            }
        }
        None => None,
    };

    // Alto lógico de la barra; el ancho lo decide el compositor (anclada a los
    // dos lados). Valor inicial de fallback hasta el primer `configure`.
    let height: u32 = config.overlay_height.max(1) as u32;
    let width: u32 = config.screen_width.max(1) as u32;

    let surface = compositor.create_surface(&qh);

    // Click-through: región de entrada VACÍA → los clics (y el ratón) pasan a
    // lo que haya debajo del overlay. Sin esto, la barra se traga todos los
    // clics de su rectángulo. Porta `wl_surface_set_input_region` de
    // `wayland_setup_surface`. Se aplica en el `layer.commit()` de abajo.
    let empty_region = compositor.wl_compositor().create_region(&qh, ());
    surface.set_input_region(Some(&empty_region));
    empty_region.destroy();

    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Top,
        Some("bongocat"),
        target_output.as_ref(), // None = la salida que elija el compositor
    );

    layer.set_anchor(anchor_for(config.overlay_position));
    layer.set_size(0, height);
    layer.set_exclusive_zone(-1); // no reservar espacio; el overlay flota
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    // Empareja la superficie con un viewport (para renderizar a resolución
    // física) y un receptor de fractional-scale (para saber la escala preferida).
    let viewport = viewporter
        .as_ref()
        .map(|vp| vp.get_viewport(layer.wl_surface(), &qh, ()));
    let fs_obj = fs_mgr
        .as_ref()
        .map(|m| m.get_fractional_scale(layer.wl_surface(), &qh, ()));

    let pool = SlotPool::new((width * height * 4) as usize, &shm)?;

    // Rasteriza los 5 fotogramas del gato a la altura configurada.
    let frames = anim::rasterize(
        config.cat_height.max(1) as u32,
        config.mirror_x,
        config.mirror_y,
    )?;
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
        config_path: config_path.clone(),
        frames,
        frame: config.idle_frame.clamp(0, 4) as u8,
        frame_dt: frame_dt_from_fps(config.fps),
        left_hold_until: now,
        right_hold_until: now,
        last_activity: now,
        last_reload: now,
        _ftl_mgr: ftl_mgr,
        _ext_list: ext_list,
        cosmic_info,
        toplevels: HashMap::new(),
        ext_to_cosmic: HashMap::new(),
        hidden: false,
        want_hidden: false,
        want_hidden_since: now,
        width,
        height,
        scale_120: 120,
        viewport,
        _fs_mgr: fs_mgr,
        _fs_obj: fs_obj,
        _target_output: target_output,
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

    // Lector de teclado: corre en el proceso hijo aislado. Un hilo puente lee su
    // tubería (bytes = bits de pata ya reducidos) y los mete en el canal calloop.
    let (tx, rx) = calloop::channel::channel::<u8>();
    {
        let pipe = std::fs::File::from(input.read);
        thread::Builder::new()
            .name("input:bridge".into())
            .spawn(move || input_child::parent_bridge(pipe, tx))
            .map_err(|e| format!("no se pudo crear el hilo puente de input: {e}"))?;
    }
    lh.insert_source(rx, |ev, _, state| {
        if let calloop::channel::Event::Msg(bit) = ev {
            state.on_paw(bit);
        }
    })?;

    // --watch-config: hilo con inotify → recarga en caliente (con debounce).
    if watch_config {
        if let Some(path) = config_path {
            let (wtx, wrx) = calloop::channel::channel::<()>();
            watch::spawn(path, wtx);
            lh.insert_source(wrx, |ev, _, state| {
                if let calloop::channel::Event::Msg(()) = ev {
                    state.reload();
                }
            })?;
        } else {
            eprintln!("bongocat: --watch-config sin fichero de configuración; se ignora");
        }
    }

    // Tick de animación a ritmo de FPS: recalcula el fotograma y redibuja si
    // cambió. Lee `state.frame_dt` para respetar cambios de `fps` en caliente.
    // TODO: bajar a un tick lento cuando no hay actividad (idle ~0% CPU).
    lh.insert_source(
        Timer::from_duration(frame_dt_from_fps(config.fps)),
        |_, _, state| {
            state.tick();
            TimeoutAction::ToDuration(state.frame_dt)
        },
    )?;

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
    config_path: Option<PathBuf>,
    frames: Frames,
    /// Fotograma actual (0–4), lo decide la máquina de estados.
    frame: u8,
    /// Duración entre ticks de animación (deriva de `fps`).
    frame_dt: Duration,
    /// Instantes hasta los que cada pata sigue "bajada".
    left_hold_until: Instant,
    right_hold_until: Instant,
    /// Última pulsación (para el reposo por inactividad).
    last_activity: Instant,
    /// Última recarga de config (para el debounce de 300 ms).
    last_reload: Instant,
    /// El manager de foreign-toplevel wlr, si el compositor lo soporta (se
    /// guarda solo para mantenerlo vivo).
    _ftl_mgr: Option<ZwlrForeignToplevelManagerV1>,
    /// La lista `ext-foreign-toplevel-list-v1` (camino COSMIC), viva mientras dure.
    _ext_list: Option<ExtForeignToplevelListV1>,
    /// El "info" de COSMIC: se usa para pedir un `zcosmic_toplevel_handle_v1` por
    /// cada toplevel de la lista (`get_cosmic_toplevel`).
    cosmic_info: Option<ZcosmicToplevelInfoV1>,
    /// Toplevels rastreados, por id de objeto. En el camino wlr la clave es el
    /// `zwlr_foreign_toplevel_handle_v1`; en el camino COSMIC, el
    /// `zcosmic_toplevel_handle_v1`.
    toplevels: HashMap<wayland_client::backend::ObjectId, TopInfo>,
    /// Camino COSMIC: id del `ext_foreign_toplevel_handle_v1` → id del
    /// `zcosmic_toplevel_handle_v1`, para poder limpiar al cerrarse la ventana
    /// (el handle COSMIC v2 no emite `closed` propio).
    ext_to_cosmic: HashMap<
        wayland_client::backend::ObjectId,
        (ExtForeignToplevelHandleV1, ZcosmicToplevelHandleV1),
    >,
    /// Estado aplicado: ¿el gato está oculto ahora (ventana a pantalla completa)?
    hidden: bool,
    /// Estado deseado según los toplevels; se aplica a `hidden` tras 350 ms
    /// estable (antirrebote frente a compositores que hacen oscilar el estado).
    want_hidden: bool,
    want_hidden_since: Instant,
    /// Tamaño **lógico** de la capa (el que da el `configure`).
    width: u32,
    height: u32,
    /// Escala HiDPI en base 120 (120 = 1.0×, 180 = 1.5×, 240 = 2.0×). Solo tiene
    /// efecto si hay `viewport` (si no, el búfer va a tamaño lógico).
    scale_120: u32,
    /// `wp_viewport` de la superficie: mapea el búfer físico al tamaño lógico.
    viewport: Option<WpViewport>,
    _fs_mgr: Option<WpFractionalScaleManagerV1>,
    _fs_obj: Option<WpFractionalScaleV1>,
    /// Salida fijada con `--monitor` (se guarda para mantener viva la proxy).
    _target_output: Option<wl_output::WlOutput>,
    exit: bool,
}

impl State {
    /// Escala efectiva en base 120. Sin `viewport` no se puede mapear un búfer
    /// físico distinto del lógico, así que se fuerza 120 (1.0×).
    fn eff_scale_120(&self) -> u32 {
        if self.viewport.is_some() {
            self.scale_120
        } else {
            120
        }
    }

    /// Altura del gato en píxeles **físicos** (la config está en lógicos).
    fn phys_cat_height(&self) -> u32 {
        scale_size_120(self.config.cat_height.max(1), self.eff_scale_120()).max(1) as u32
    }

    /// Posición del gato dentro del búfer **físico** de `phys_w`×`phys_h`.
    /// Porta el cálculo de `draw_bar`: los offsets de la config (lógicos) se
    /// pasan a físicos con `scale_offset_120`.
    fn cat_origin(&self, phys_w: i32, phys_h: i32) -> (i32, i32) {
        let s = self.eff_scale_120();
        let (cw, ch) = (self.frames.w as i32, self.frames.h as i32);
        let xoff = scale_offset_120(self.config.cat_x_offset, s);
        let yoff = scale_offset_120(self.config.cat_y_offset, s);
        let y = (phys_h - ch) / 2 + yoff;
        let x = match self.config.cat_align {
            Align::Center => (phys_w - cw) / 2 + xoff,
            Align::Left => xoff,
            Align::Right => phys_w - cw - xoff,
        };
        (x, y)
    }

    /// Re-rasteriza los fotogramas del gato a la altura física actual.
    fn rerasterize(&mut self) {
        match anim::rasterize(
            self.phys_cat_height(),
            self.config.mirror_x,
            self.config.mirror_y,
        ) {
            Ok(f) => self.frames = f,
            Err(e) => eprintln!("bongocat: re-rasterizado falló: {e}"),
        }
    }

    /// Cambia la escala HiDPI (evento `preferred_scale` o escala de la salida).
    fn set_scale(&mut self, new_120: u32) {
        if new_120 == 0 || new_120 == self.scale_120 {
            return;
        }
        self.scale_120 = new_120;
        if self.viewport.is_some() {
            self.rerasterize();
            eprintln!("bongocat: escala HiDPI {}/120", new_120);
            self.draw();
        }
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
        self.apply_pending_hidden();
        let now = Instant::now();
        let idle_sleep = self.config.idle_sleep_timeout_sec > 0
            && now.duration_since(self.last_activity).as_secs()
                >= self.config.idle_sleep_timeout_sec as u64;

        // Reposo por horario: si `enable_scheduled_sleep` y la hora local cae en
        // la franja `[sleep_begin, sleep_end)`. Porta `anim_is_sleep_time`.
        let scheduled_sleep = self.config.enable_scheduled_sleep
            && local_now_minutes().is_some_and(|m| {
                bongocat_common::sleep::is_scheduled_sleep(
                    m,
                    self.config.sleep_begin.minutes(),
                    self.config.sleep_end.minutes(),
                )
            });

        let next = if idle_sleep || scheduled_sleep {
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

    /// Recarga la configuración desde disco y aplica los cambios en vivo.
    /// Debounce de 300 ms (editores que escriben varias veces seguidas). Si la
    /// nueva config no carga, se mantiene la actual. Porta `config_reload_apply`
    /// y los caminos de `wayland_update_config`. Pendiente: cambio de teclado,
    /// de `layer` y de monitor.
    fn reload(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_reload) < Duration::from_millis(300) {
            return;
        }
        self.last_reload = now;

        let Some(path) = self.config_path.clone() else {
            return;
        };
        let loaded = match bongocat_common::io::load(Some(&path)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("bongocat: recarga falló ({e}); se mantiene la configuración");
                return;
            }
        };
        for w in &loaded.warnings {
            eprintln!("bongocat: aviso: {w}");
        }

        let old = std::mem::replace(&mut self.config, loaded.config);
        let c = self.config.clone();

        // Aspecto del gato → re-rasterizar (a la altura física actual).
        if c.cat_height != old.cat_height
            || c.mirror_x != old.mirror_x
            || c.mirror_y != old.mirror_y
        {
            self.rerasterize();
        }

        // FPS → el próximo tick lo recoge.
        self.frame_dt = frame_dt_from_fps(c.fps);

        // Barra: alto / posición.
        let mut surface_changed = false;
        if c.overlay_height != old.overlay_height {
            self.layer.set_size(0, c.overlay_height.max(1) as u32);
            surface_changed = true;
        }
        if c.overlay_position != old.overlay_position {
            self.layer.set_anchor(anchor_for(c.overlay_position));
            surface_changed = true;
        }
        if surface_changed {
            self.layer.commit(); // el `configure` que llega redibuja con el tamaño nuevo
        }

        self.draw();
        eprintln!("bongocat: configuración recargada");
    }

    /// Recalcula el estado *deseado* de ocultar: algún toplevel activado y a
    /// pantalla completa (y `disable_fullscreen_hide` desactivado). No dibuja: el
    /// cambio se aplica desde `apply_pending_hidden` con antirrebote. Para un
    /// solo monitor no hace falta mirar en qué salida está. Porta
    /// `fs_recompute_state` con el fallback global de `fullscreen.c`.
    fn recompute_hidden(&mut self) {
        let want = !self.config.disable_fullscreen_hide
            && self.toplevels.values().any(|t| t.fullscreen && t.activated);
        if want != self.want_hidden {
            self.want_hidden = want;
            self.want_hidden_since = Instant::now();
        }
    }

    /// Aplica ocultar/mostrar por pantalla completa con un antirrebote de
    /// 350 ms. Algunos compositores (visto en cosmic-comp 1.0) hacen oscilar el
    /// estado de pantalla completa de las ventanas; sin esto el gato
    /// parpadearía. Se llama desde `tick` (cada fotograma).
    fn apply_pending_hidden(&mut self) {
        if self.want_hidden == self.hidden {
            return;
        }
        if Instant::now().duration_since(self.want_hidden_since) < Duration::from_millis(350) {
            return;
        }
        self.hidden = self.want_hidden;
        eprintln!(
            "bongocat: pantalla completa {}",
            if self.hidden {
                "detectada — gato oculto"
            } else {
                "despejada"
            }
        );
        self.draw();
    }

    /// Limpia el buffer (transparente) y dibuja el fotograma actual del gato.
    ///
    /// El búfer se crea en píxeles **físicos** (`lógico × escala/120`); si hay
    /// `wp_viewport`, se le fija como destino el tamaño **lógico** para que el
    /// compositor lo reescale sin pérdida en pantallas HiDPI. A escala 1.0× (o
    /// sin viewport) físico == lógico y todo es idéntico al camino sin HiDPI.
    fn draw(&mut self) {
        let (lw, lh) = (self.width.max(1), self.height.max(1));
        let s = self.eff_scale_120();
        let pw = scale_size_120(lw as i32, s).max(1) as u32;
        let ph = scale_size_120(lh as i32, s).max(1) as u32;
        let stride = pw as i32 * 4;
        let needed = (pw * ph * 4) as usize;
        // Se calculan antes de tocar el pool (evita conflictos de préstamo).
        let (ox, oy) = self.cat_origin(pw as i32, ph as i32);
        let (fw, fh) = (self.frames.w, self.frames.h);
        let frame = self.frames.frame(self.frame as usize);

        if let Err(e) = self.pool.resize(needed.max(1)) {
            eprintln!("bongocat: no se pudo redimensionar el pool a {needed}: {e}");
            return;
        }
        let (buffer, canvas) =
            match self
                .pool
                .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
            {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("bongocat: create_buffer {pw}x{ph} falló: {e}");
                    return;
                }
            };

        if self.hidden {
            // Ventana a pantalla completa: barra totalmente transparente y sin
            // gato (equivale a opacidad efectiva 0 en `draw_bar`).
            canvas.fill(0);
        } else {
            // Fondo de la barra: negro con `overlay_opacity`. Premultiplicado
            // con RGB=0 → bytes [B,G,R,A] = [0,0,0,opacidad]. 0 = transparente.
            let op = self.config.overlay_opacity.clamp(0, 255) as u8;
            if op == 0 {
                canvas.fill(0);
            } else {
                for px in canvas.chunks_exact_mut(4) {
                    px.copy_from_slice(&[0, 0, 0, op]);
                }
            }
            anim::blit_over(canvas, (pw, ph), frame, (fw, fh), (ox, oy));
        }

        // El viewport traduce el búfer físico al tamaño lógico de la superficie.
        if let Some(vp) = &self.viewport {
            vp.set_destination(lw as i32, lh as i32);
        }

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
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
        o: &wl_output::WlOutput,
    ) {
        // Sin fractional-scale: usar la escala **entera** de la salida donde
        // entró la superficie (fallback de `scale_120_from_output` del C).
        if self._fs_obj.is_none() {
            if let Some(info) = self.output_state.info(o) {
                if info.scale_factor > 0 {
                    self.set_scale(info.scale_factor as u32 * 120);
                }
            }
        }
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
// wl_region no tiene eventos: solo la usamos para la región de entrada vacía.
delegate_noop!(State: ignore wl_region::WlRegion);
// HiDPI: el manager y el viewporter no emiten eventos; el viewport tampoco.
delegate_noop!(State: ignore WpViewporter);
delegate_noop!(State: ignore WpViewport);
delegate_noop!(State: ignore WpFractionalScaleManagerV1);

/// `wp_fractional_scale_v1::preferred_scale`: escala sugerida en /120.
impl Dispatch<WpFractionalScaleV1, ()> for State {
    fn event(
        state: &mut Self,
        _obj: &WpFractionalScaleV1,
        event: fs_v1::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let fs_v1::Event::PreferredScale { scale } = event {
            state.set_scale(scale);
        }
    }
}

// ── foreign-toplevel-management: detección de pantalla completa ──────────────
impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _mgr: &ZwlrForeignToplevelManagerV1,
        event: ftl_mgr::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ftl_mgr::Event::Toplevel { toplevel } => {
                state.toplevels.insert(toplevel.id(), TopInfo::default());
            }
            ftl_mgr::Event::Finished => {
                state.toplevels.clear();
                state.recompute_hidden();
            }
            _ => {}
        }
    }

    event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        ftl_mgr::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: ftl_handle::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let id = handle.id();
        match event {
            ftl_handle::Event::State { state: arr } => {
                let (fullscreen, activated) = parse_toplevel_state(&arr);
                if let Some(t) = state.toplevels.get_mut(&id) {
                    t.fullscreen = fullscreen;
                    t.activated = activated;
                }
                state.recompute_hidden();
            }
            ftl_handle::Event::Closed => {
                state.toplevels.remove(&id);
                state.recompute_hidden();
            }
            _ => {}
        }
    }
}

// ── Camino COSMIC / GNOME ───────────────────────────────────────────────────
// `ext-foreign-toplevel-list-v1` da la lista de ventanas; por cada una se pide
// a `zcosmic_toplevel_info_v1` (v2) un `zcosmic_toplevel_handle_v1` cuyo evento
// `state` trae los mismos flags (`activated`, `fullscreen`) que el protocolo wlr.

impl Dispatch<ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _list: &ExtForeignToplevelListV1,
        event: ext_list::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_list::Event::Toplevel { toplevel } => {
                // Puente v2: obtener el handle COSMIC para poder leer el estado.
                let Some(info) = state.cosmic_info.as_ref() else {
                    return;
                };
                let cosmic = info.get_cosmic_toplevel(&toplevel, qh, ());
                state.toplevels.insert(cosmic.id(), TopInfo::default());
                state
                    .ext_to_cosmic
                    .insert(toplevel.id(), (toplevel, cosmic));
            }
            ext_list::Event::Finished => {
                state.toplevels.clear();
                state.ext_to_cosmic.clear();
                state.recompute_hidden();
            }
            _ => {}
        }
    }

    event_created_child!(State, ExtForeignToplevelListV1, [
        ext_list::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_handle::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // El handle COSMIC v2 no emite `closed` propio: el de la lista es el
        // equivalente, así que aquí se limpian los dos.
        if let ext_handle::Event::Closed = event {
            if let Some((ext_h, cosmic_h)) = state.ext_to_cosmic.remove(&handle.id()) {
                state.toplevels.remove(&cosmic_h.id());
                cosmic_h.destroy();
                ext_h.destroy();
                state.recompute_hidden();
            }
        }
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        state: &mut Self,
        _info: &ZcosmicToplevelInfoV1,
        event: cosmic_info::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let cosmic_info::Event::Finished = event {
            state.toplevels.clear();
            state.ext_to_cosmic.clear();
            state.recompute_hidden();
        }
        // `done`: agrupa cambios de forma atómica; no lo necesitamos.
    }
}

impl Dispatch<ZcosmicToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZcosmicToplevelHandleV1,
        event: cosmic_handle::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let cosmic_handle::Event::State { state: arr } = event {
            let (fullscreen, activated) = parse_toplevel_state(&arr);
            if let Some(t) = state.toplevels.get_mut(&handle.id()) {
                t.fullscreen = fullscreen;
                t.activated = activated;
            }
            state.recompute_hidden();
        }
    }
}
