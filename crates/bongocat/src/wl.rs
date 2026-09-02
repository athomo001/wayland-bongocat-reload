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
//! lógica). `--monitor` fija la salida. Socket de control IPC (spec 0003 M1:
//! PING/STATE/QUIT). Porta `src/platform/wayland.c`.

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
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
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
    protocol::{wl_compositor, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self as ftl_handle, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self as ftl_mgr, ZwlrForeignToplevelManagerV1},
};

use std::collections::HashMap;
use std::io::Write;
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
use crate::{input, input_child, ipc, theme, watch};

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

/// Botón izquierdo del ratón en el protocolo Linux `input-event-codes.h`.
const BTN_LEFT: u32 = 0x110;

/// Estado del modo edición con ratón (spec 0005). La geometría vive en
/// `bongocat_common::edit`; aquí solo el estado del arrastre.
#[derive(Default)]
struct EditState {
    active: bool,
    dragging: bool,
    /// Desplazamiento puntero → origen del gato al agarrar (lógicas).
    grab_dx: f64,
    grab_dy: f64,
    /// Última posición conocida del puntero (para re-anclar al usar la rueda).
    ptr: (f64, f64),
    /// `(cat_x_offset, cat_y_offset, cat_height)` al entrar, para "¿cambió algo?".
    snapshot: (i32, i32, i32),
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

/// Rasteriza los 5 fotogramas a la altura `h` (px) desde el tema cargado, o del
/// embebido si no hay. Un tema SVG (`theme_format` 1/2) va por `rasterize_from`;
/// uno de sprite sheet (formato 3, spec 0014) por `rasterize_sheet`.
fn rasterize_loaded(
    theme: Option<&theme::LoadedTheme>,
    h: u32,
    mx: bool,
    my: bool,
) -> Result<Frames, Box<dyn Error>> {
    let Some(t) = theme else {
        return anim::rasterize(h, mx, my);
    };
    match &t.art {
        theme::ThemeArt::Svg(svgs) => {
            anim::rasterize_from(svgs.as_ref(), t.meta.aspect, false, h, mx, my)
        }
        theme::ThemeArt::Sheet(s) => {
            anim::rasterize_sheet(&s.sheet, &s.png.rgba, s.png.w, s.png.h, h, mx, my)
        }
    }
}

/// Rasteriza los 5 fotogramas: del tema si hay, si no del embebido.
fn rasterize_theme(
    theme: Option<&theme::LoadedTheme>,
    cfg: &Config,
) -> Result<Frames, Box<dyn Error>> {
    rasterize_loaded(
        theme,
        cfg.cat_height.max(1) as u32,
        cfg.mirror_x,
        cfg.mirror_y,
    )
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

    // Tema (spec 0006): si `theme=` está, se cargan sus 5 SVG; si falla, el
    // gato embebido (`classic`) sigue disponible siempre.
    let theme = theme::resolve(&config.theme);
    let frames = rasterize_theme(theme.as_ref(), config)?;
    eprintln!(
        "bongocat: {} fotogramas rasterizados a {}x{} (aspecto {}:{})",
        5, frames.w, frames.h, frames.aspect.0, frames.aspect.1
    );

    let now = Instant::now();
    let mut state = State {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        wl_compositor: compositor.wl_compositor().clone(),
        _pointer: None,
        edit: EditState::default(),
        qh: qh.clone(),
        shm,
        pool,
        layer,
        config: config.clone(),
        config_path: config_path.clone(),
        frames,
        theme,
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
        ipc_dirty: std::collections::HashSet::new(),
        manual_hidden: None,
        exit: false,
        configured: false,
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

    // Socket de control IPC (spec 0003 M1): PING / STATE / QUIT.
    let _ipc_guard = if config.enable_ipc {
        match ipc::bind(target_output_name.as_deref()) {
            Ok((listener, guard)) => {
                let src = calloop::generic::Generic::new(
                    listener,
                    calloop::Interest::READ,
                    calloop::Mode::Level,
                );
                lh.insert_source(src, |_readiness, listener, state: &mut State| {
                    loop {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                if !ipc::same_uid(&stream) {
                                    continue;
                                }
                                if let Ok(req) = ipc::read_request(&stream) {
                                    let reply = state.ipc_reply(&req);
                                    let _ = writeln!(&stream, "{reply}");
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(_) => break,
                        }
                    }
                    Ok(calloop::PostAction::Continue)
                })?;
                Some(guard)
            }
            Err(e) => {
                eprintln!("bongocat: no se pudo abrir el socket IPC: {e}");
                None
            }
        }
    } else {
        eprintln!("bongocat: enable_ipc=0; sin socket de control");
        None
    };

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
    seat_state: SeatState,
    /// Para recrear la región de entrada (vacía normal; rect del gato en edición).
    wl_compositor: wl_compositor::WlCompositor,
    /// Puntero del seat, si lo hay (se guarda para mantenerlo vivo).
    _pointer: Option<wl_pointer::WlPointer>,
    edit: EditState,
    qh: QueueHandle<State>,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    config: Config,
    config_path: Option<PathBuf>,
    frames: Frames,
    /// Tema activo cargado de disco, o `None` para el gato embebido.
    theme: Option<theme::LoadedTheme>,
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
    /// Claves cambiadas por `SET` de IPC y aún sin `SAVE` al fichero.
    ipc_dirty: std::collections::HashSet<String>,
    /// Anulación manual del ocultado (IPC `SHOW`/`HIDE`/`TOGGLE`): `Some` fuerza
    /// el estado; `None` = seguir la lógica de pantalla completa.
    manual_hidden: Option<bool>,
    exit: bool,
    /// ¿Llegó ya el primer `configure`? Antes no se puede adjuntar búfer
    /// (`zwlr_layer_surface_v1`: error si se ataca antes del ack inicial).
    configured: bool,
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

    /// Relación de aspecto del tema activo, en `i32` (para `edit::cat_rect`).
    fn cat_aspect(&self) -> (i32, i32) {
        (self.frames.aspect.0 as i32, self.frames.aspect.1 as i32)
    }

    /// Cambia el tema en caliente (IPC `THEME`). `spec` vacío / `embedded` /
    /// `none` → el gato embebido. Marca `theme` como sucia para `SAVE`.
    fn set_theme(&mut self, spec: &str) -> String {
        let spec = match spec {
            "embedded" | "none" | "classic-embedded" => "",
            s => s,
        };
        let loaded = theme::resolve(spec);
        if !spec.is_empty() && loaded.is_none() {
            return format!("ERR el tema '{spec}' no cargó; sigo con el actual");
        }
        self.theme = loaded;
        self.config.theme = spec.to_string();
        self.ipc_dirty.insert("theme".to_string());
        self.rerasterize();
        self.draw();
        format!(
            "OK theme={}",
            if spec.is_empty() { "embedded" } else { spec }
        )
    }

    /// Re-rasteriza los fotogramas del gato a la altura física actual, desde el
    /// tema activo o el embebido.
    fn rerasterize(&mut self) {
        let h = self.phys_cat_height();
        let (mx, my) = (self.config.mirror_x, self.config.mirror_y);
        match rasterize_loaded(self.theme.as_ref(), h, mx, my) {
            Ok(f) => self.frames = f,
            Err(e) => eprintln!("bongocat: re-rasterizado falló: {e}"),
        }
    }

    /// Reacciona a un cambio de configuración (recarga de fichero o `SET` por
    /// IPC): re-rasteriza si cambió el aspecto del gato, ajusta `frame_dt`,
    /// redimensiona/reancla la barra si hace falta, y redibuja. Común a
    /// `reload` y al `SET` en vivo.
    fn apply_config_diff(&mut self, old: &Config) {
        let c = self.config.clone();
        if c.theme != old.theme {
            self.theme = theme::resolve(&c.theme);
        }
        if c.theme != old.theme
            || c.cat_height != old.cat_height
            || c.mirror_x != old.mirror_x
            || c.mirror_y != old.mirror_y
        {
            self.rerasterize();
        }
        self.frame_dt = frame_dt_from_fps(c.fps);

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
        self.apply_config_diff(&old);
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

    /// ¿Está el gato oculto ahora mismo? Anulación manual sobre la lógica de
    /// pantalla completa.
    fn effective_hidden(&self) -> bool {
        self.manual_hidden.unwrap_or(self.hidden)
    }

    /// Entra o sale del modo edición (spec 0005). Al salir con `persist`, si los
    /// `cat_*_offset` / `cat_height` cambiaron, se guardan al `.conf` (con
    /// `ConfDoc`, sin tocar el resto).
    fn edit_set(&mut self, active: bool, persist: bool) -> String {
        if active != self.edit.active {
            self.edit.active = active;
            self.edit.dragging = false;
            if active {
                self.edit.snapshot = (
                    self.config.cat_x_offset,
                    self.config.cat_y_offset,
                    self.config.cat_height,
                );
                self.layer
                    .set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
            } else {
                self.layer
                    .set_keyboard_interactivity(KeyboardInteractivity::None);
                let now = (
                    self.config.cat_x_offset,
                    self.config.cat_y_offset,
                    self.config.cat_height,
                );
                if persist && now != self.edit.snapshot {
                    for k in ["cat_x_offset", "cat_y_offset", "cat_height"] {
                        self.ipc_dirty.insert(k.to_string());
                    }
                    eprintln!("bongocat: modo edición — {}", self.ipc_save());
                }
            }
            self.layer.commit();
            self.draw(); // recoloca la región de entrada
            let r = bongocat_common::edit::cat_rect(
                &self.config,
                self.width as i32,
                self.height as i32,
                self.cat_aspect(),
            );
            eprintln!(
                "bongocat: modo edición {} — región del gato = {:?}",
                if active { "ON" } else { "OFF" },
                r
            );
        }
        format!("OK edit={}", if active { "on" } else { "off" })
    }

    /// Botón izquierdo dentro del gato: empezar a arrastrar.
    fn edit_press(&mut self, px: f64, py: f64) {
        let (bw, bh) = (self.width as i32, self.height as i32);
        let rect = bongocat_common::edit::cat_rect(&self.config, bw, bh, self.cat_aspect());
        let inside = bongocat_common::edit::hit(rect, px as i32, py as i32);
        if inside {
            self.edit.dragging = true;
            self.edit.grab_dx = px - f64::from(rect.0);
            self.edit.grab_dy = py - f64::from(rect.1);
        }
        self.edit.ptr = (px, py);
    }

    /// Movimiento con el gato agarrado: recalcula `cat_x_offset` / `cat_y_offset`.
    fn edit_drag(&mut self, px: f64, py: f64) {
        use bongocat_common::edit;
        self.edit.ptr = (px, py);
        let (bw, bh) = (self.width as i32, self.height as i32);
        let (_, _, cw, ch) = edit::cat_rect(&self.config, bw, bh, self.cat_aspect());
        let ox = (px - self.edit.grab_dx).round() as i32;
        let oy = (py - self.edit.grab_dy).round() as i32;
        let (ox, oy) = edit::clamp_origin(ox, oy, bw, bh, cw, ch);
        self.config.cat_x_offset = edit::origin_to_x_offset(self.config.cat_align, ox, bw, cw);
        self.config.cat_y_offset = edit::origin_to_y_offset(oy, bh, ch);
        self.draw();
    }

    /// Rueda en modo edición: cambia `cat_height` (con recache). Si se está
    /// arrastrando, re-ancla el agarre para que el gato "crezca bajo el cursor".
    fn edit_wheel(&mut self, step: i32) {
        let new_h = bongocat_common::edit::resize_cat_height(self.config.cat_height, step);
        if new_h == self.config.cat_height {
            return;
        }
        self.config.cat_height = new_h;
        self.rerasterize();
        if self.edit.dragging {
            let (bw, bh) = (self.width as i32, self.height as i32);
            let (rx, ry, ..) =
                bongocat_common::edit::cat_rect(&self.config, bw, bh, self.cat_aspect());
            self.edit.grab_dx = self.edit.ptr.0 - f64::from(rx);
            self.edit.grab_dy = self.edit.ptr.1 - f64::from(ry);
        }
        self.draw();
    }

    /// Responde a una petición del socket de control (spec 0003 M1–M3, 0011).
    /// Verbos: `PING`, `STATE`, `GET clave`, `SET clave valor` (en vivo),
    /// `SAVE`, `RELOAD`, `SHOW`/`HIDE`/`TOGGLE`/`AUTO`, `QUIT`.
    fn ipc_reply(&mut self, req: &str) -> String {
        let mut parts = req.splitn(3, char::is_whitespace);
        let verb = parts.next().unwrap_or("").to_ascii_uppercase();
        let arg1 = parts.next().unwrap_or("").trim();
        let arg2 = parts.next().unwrap_or("").trim();

        match verb.as_str() {
            "PING" => "PONG".to_string(),
            "STATE" => format!(
                "pid={} frame={} hidden={} manual_hidden={} edit={} theme={} scale_120={} \
                 fps={} width={} height={} cat_height={} cat_opacity={}",
                std::process::id(),
                self.frame,
                self.effective_hidden(),
                match self.manual_hidden {
                    Some(true) => "hide",
                    Some(false) => "show",
                    None => "auto",
                },
                self.edit.active,
                if self.config.theme.is_empty() {
                    "embedded"
                } else {
                    &self.config.theme
                },
                self.scale_120,
                self.config.fps,
                self.width,
                self.height,
                self.config.cat_height,
                self.config.cat_opacity,
            ),
            "GET" if !arg1.is_empty() => {
                match bongocat_common::config::ConfDoc::parse(&self.config.to_ini()).get(arg1) {
                    Some(v) => v.to_string(),
                    None => format!("ERR clave desconocida: {arg1}"),
                }
            }
            "SET" if !arg1.is_empty() && !arg2.is_empty() => {
                let old = self.config.clone();
                match bongocat_common::config::set_live(&mut self.config, arg1, arg2) {
                    Ok(warnings) => {
                        self.apply_config_diff(&old);
                        self.ipc_dirty.insert(arg1.to_string());
                        if warnings.is_empty() {
                            format!("OK {arg1}={arg2}")
                        } else {
                            format!("OK {} (ajustado: {})", arg1, warnings.join("; "))
                        }
                    }
                    Err(e) => format!("ERR {e}"),
                }
            }
            "GET" | "SET" => "ERR uso: GET clave | SET clave valor".to_string(),
            "THEME" => match arg1 {
                "" => "ERR uso: THEME list | next | <nombre>".to_string(),
                "list" => {
                    let mut l = theme::list();
                    l.insert(0, "embedded".to_string());
                    l.join(" ")
                }
                "next" => {
                    let all = theme::list();
                    if all.is_empty() {
                        return "ERR no hay temas instalados".to_string();
                    }
                    let cur = self.config.theme.clone();
                    let next = match all.iter().position(|n| *n == cur) {
                        Some(i) => all[(i + 1) % all.len()].clone(),
                        None => all[0].clone(),
                    };
                    self.set_theme(&next)
                }
                name => self.set_theme(name),
            },
            "EDIT" => match match arg1 {
                "on" => Some(true),
                "off" => Some(false),
                "toggle" | "" => Some(!self.edit.active),
                _ => None,
            } {
                Some(on) => self.edit_set(on, !on), // al apagar, persiste
                None => "ERR uso: EDIT on|off|toggle".to_string(),
            },
            "SHOW" | "HIDE" | "TOGGLE" | "AUTO" => {
                self.manual_hidden = match verb.as_str() {
                    "SHOW" => Some(false),
                    "HIDE" => Some(true),
                    "TOGGLE" => Some(!self.effective_hidden()),
                    _ => None, // AUTO: vuelve a seguir la pantalla completa
                };
                self.draw();
                format!(
                    "OK {}",
                    if self.effective_hidden() {
                        "hidden"
                    } else {
                        "shown"
                    }
                )
            }
            "SAVE" => self.ipc_save(),
            "RELOAD" => {
                if self.config_path.is_some() {
                    self.reload();
                    "OK".to_string()
                } else {
                    "ERR sin fichero de configuración".to_string()
                }
            }
            "QUIT" => {
                self.exit = true;
                "OK".to_string()
            }
            "" => "ERR petición vacía".to_string(),
            other => format!("ERR comando desconocido: {other}"),
        }
    }

    /// `SAVE`: vuelca al `.conf` **solo las claves cambiadas por `SET`** desde el
    /// último guardado, con `ConfDoc` para no tocar comentarios ni el resto de
    /// líneas. Escritura atómica.
    fn ipc_save(&mut self) -> String {
        let Some(path) = self.config_path.clone() else {
            return "ERR sin fichero de configuración".to_string();
        };
        if self.ipc_dirty.is_empty() {
            return "OK (nada que guardar)".to_string();
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut doc = bongocat_common::config::ConfDoc::parse(&text);
        let ini = bongocat_common::config::ConfDoc::parse(&self.config.to_ini());
        for key in &self.ipc_dirty {
            if let Some(v) = ini.get(key) {
                doc.set(key, v);
            }
        }
        match bongocat_common::io::save_atomic(&path, &doc.render()) {
            Ok(()) => {
                let n = self.ipc_dirty.len();
                self.ipc_dirty.clear();
                self.last_reload = Instant::now(); // no re-disparar la recarga por el watcher
                format!("OK ({n} clave(s) guardada(s))")
            }
            Err(e) => format!("ERR no se pudo escribir {}: {e}", path.display()),
        }
    }

    /// Limpia el buffer (transparente) y dibuja el fotograma actual del gato.
    ///
    /// El búfer se crea en píxeles **físicos** (`lógico × escala/120`); si hay
    /// `wp_viewport`, se le fija como destino el tamaño **lógico** para que el
    /// compositor lo reescale sin pérdida en pantallas HiDPI. A escala 1.0× (o
    /// sin viewport) físico == lógico y todo es idéntico al camino sin HiDPI.
    fn draw(&mut self) {
        // No adjuntar búfer antes del primer `configure` (protocolo layer-shell).
        // El estado interno sí avanza; se pintará al llegar el `configure`.
        if !self.configured {
            return;
        }
        let (lw, lh) = (self.width.max(1), self.height.max(1));
        let s = self.eff_scale_120();
        let pw = scale_size_120(lw as i32, s).max(1) as u32;
        let ph = scale_size_120(lh as i32, s).max(1) as u32;
        let stride = pw as i32 * 4;
        let needed = (pw * ph * 4) as usize;
        // Se calculan antes de tocar el pool (evita conflictos de préstamo).
        let (ox, oy) = self.cat_origin(pw as i32, ph as i32);
        let (fw, fh) = (self.frames.w, self.frames.h);
        let hidden = self.effective_hidden();
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

        if hidden {
            // Oculto (pantalla completa o `HIDE` manual): barra transparente y
            // sin gato (opacidad efectiva 0 en `draw_bar`).
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
            // Opacidad del gato: % (0–100) → factor 0–255 para el blit.
            let cat_op = (self.config.cat_opacity.clamp(0, 100) * 255 / 100) as u8;
            anim::blit_over(canvas, (pw, ph), frame, (fw, fh), (ox, oy), cat_op);
        }

        // El viewport traduce el búfer físico al tamaño lógico de la superficie.
        if let Some(vp) = &self.viewport {
            vp.set_destination(lw as i32, lh as i32);
        }

        // Región de entrada: vacía (click-through) normalmente; la **barra
        // entera** mientras dure el modo edición, para que el arrastre no se
        // corte cuando el gato (y su rect) se mueven bajo el cursor. El
        // hit-test del gato lo hace `edit_press`.
        let region = self.wl_compositor.create_region(&self.qh, ());
        if self.edit.active {
            region.add(0, 0, lw as i32, lh as i32);
        }
        self.layer.wl_surface().set_input_region(Some(&region));
        region.destroy();

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
        self.configured = true; // desde aquí ya se puede adjuntar búfer
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
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_layer!(State);
delegate_registry!(State);
delegate_seat!(State);
delegate_pointer!(State);
// wl_region no tiene eventos: solo la usamos para la región de entrada.
delegate_noop!(State: ignore wl_region::WlRegion);

// ── Modo edición con ratón (spec 0005 M2–M4, M6) ────────────────────────────
impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer && self._pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(p) => {
                    self._pointer = Some(p);
                    eprintln!("bongocat: puntero del seat listo (modo edición disponible)");
                }
                Err(e) => eprintln!("bongocat: sin puntero para el modo edición: {e}"),
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer {
            self._pointer = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        if !self.edit.active {
            return;
        }
        let surface = self.layer.wl_surface().clone();
        for ev in events {
            if ev.surface != surface {
                continue;
            }
            let (px, py) = ev.position;
            match ev.kind {
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.edit_press(px, py);
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.edit.dragging = false;
                }
                PointerEventKind::Leave { .. } => {
                    self.edit.dragging = false; // por si el cursor se sale
                }
                PointerEventKind::Motion { .. } if self.edit.dragging => {
                    self.edit_drag(px, py);
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let dir = if vertical.absolute < 0.0 { 1 } else { -1 };
                    self.edit_wheel(dir * bongocat_common::edit::wheel_step(false));
                }
                _ => {}
            }
        }
    }
}
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
