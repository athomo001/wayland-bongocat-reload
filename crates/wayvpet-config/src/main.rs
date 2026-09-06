//! `wayvpet-config` — ventana gráfica de configuración de wayvpet (spec 0007,
//! Fase 4). La lanza el ítem "Configurar…" del menú de la bandeja.
//!
//! Toolkit: `egui`/`eframe` — se dibuja a sí misma, así que se ve igual en
//! GNOME, KDE, COSMIC, Sway/Hyprland… sin librerías de toolkit del sistema.
//!
//! **M3 (esta versión):** cada campo de `field_meta` se renderiza según su
//! `FieldKind` y se aplica **en vivo** por IPC `SET`; botones Guardar /
//! Restablecer; se cierra sola si la instancia que la abrió desaparece. El mapa
//! de pantalla y la galería de temas llegan en M4.

mod expert;
mod model;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use model::{Model, Source};
use wayvpet_common::field_meta::{FieldKind, FieldMeta, Section, FIELDS};
use wayvpet_common::ipc;

/// Secciones en el orden en que se muestran en la navegación lateral.
const SECTIONS: [Section; 7] = [
    Section::Position,
    Section::Appearance,
    Section::Input,
    Section::Sleep,
    Section::Theme,
    Section::Advanced,
    Section::Expert,
];

/// Cada cuánto se comprueba que la instancia sigue viva. Si desaparece, el
/// socket se borra y `connect` falla al instante, así que el cierre es rápido.
const PING_EVERY: Duration = Duration::from_millis(1000);

fn main() -> eframe::Result {
    // `--instance <NOMBRE>` preselecciona la instancia de esa salida.
    let instance = std::env::args()
        .skip_while(|a| a != "--instance")
        .nth(1)
        .unwrap_or_default();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("wayvpet · Configurar")
            .with_inner_size([880.0, 620.0])
            .with_min_inner_size([640.0, 440.0]),
        ..Default::default()
    };

    eframe::run_native(
        "wayvpet-config",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(instance)))),
    )
}

struct App {
    model: Model,
    section: Section,
    /// Instancia elegida (vacío = la por defecto). Editable en la barra superior.
    instance: String,
    /// Búferes de los campos de texto/hora/lista: se aplican al perder el foco,
    /// no en cada tecla. Se vacían al recargar.
    edits: HashMap<&'static str, String>,
    /// La ventana la abrió una instancia viva: si esa instancia desaparece
    /// (p. ej. "Cerrar" del tray), la ventana se cierra también.
    tied_to_instance: bool,
    last_ping: Instant,
    ping_fails: u8,
    /// Ficheros `.ini` abiertos en el modo experto (vacío hasta entrar ahí).
    raw: Vec<expert::RawFile>,
}

impl App {
    fn new(instance: String) -> Self {
        let model = Model::load(opt(&instance));
        let tied_to_instance = model.source == Source::Instance;
        Self {
            model,
            section: Section::Position,
            instance,
            edits: HashMap::new(),
            tied_to_instance,
            last_ping: Instant::now(),
            ping_fails: 0,
            raw: Vec::new(),
        }
    }

    fn reload(&mut self) {
        self.model = Model::load(opt(&self.instance));
        self.tied_to_instance = self.model.source == Source::Instance;
        self.ping_fails = 0;
        self.edits.clear();
        self.raw.clear();
    }

    /// Si la ventana está atada a una instancia, comprueba que sigue respondiendo;
    /// tras 2 fallos seguidos, cierra la ventana.
    fn watch_instance(&mut self, ctx: &egui::Context) {
        if !self.tied_to_instance {
            return;
        }
        ctx.request_repaint_after(PING_EVERY);
        if self.last_ping.elapsed() < PING_EVERY {
            return;
        }
        self.last_ping = Instant::now();
        match ipc::send_request(opt(&self.instance), "PING") {
            Ok(r) if r.trim() == "PONG" => self.ping_fails = 0,
            _ => {
                self.ping_fails = self.ping_fails.saturating_add(1);
                if self.ping_fails >= 2 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
}

/// `""` → `None`; texto → `Some(recortado)`.
fn opt(s: &str) -> Option<&str> {
    Some(s.trim()).filter(|s| !s.is_empty())
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.watch_instance(ctx);
        self.top_bar(ctx);
        self.side_nav(ctx);
        self.bottom_bar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            ui.heading(self.section.label_es());
            ui.add_space(10.0);
            if self.section == Section::Expert {
                self.expert_panel(ui);
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Un vpet que se pasea solo no se "coloca" con estos campos:
                    // se recoloca arrastrándolo, y sigue con su conducta.
                    if self.section == Section::Position && self.model.roaming {
                        ui.label(
                            "Este vpet se mueve solo por la pantalla. Para recolocarlo, \
                             abre la bandeja del sistema → «Arrastre libre» y muévelo con \
                             el ratón; al soltarlo sigue caminando. El tamaño está en \
                             «Apariencia» (o con la rueda durante el arrastre).",
                        );
                        return;
                    }
                    for f in FIELDS.iter().filter(|f| f.section == self.section) {
                        field_row(ui, &mut self.model, &mut self.edits, f);
                    }
                });
        });
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("cabecera").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Configurar wayvpet");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (color, texto) = match self.model.source {
                        Source::Instance => (
                            egui::Color32::from_rgb(0x3f, 0xb9, 0x50),
                            "conectado — se aplica en vivo",
                        ),
                        Source::File => (
                            egui::Color32::from_rgb(0xd6, 0x9e, 0x2e),
                            "sin instancia — se guardará al fichero",
                        ),
                        Source::Defaults => (
                            egui::Color32::from_rgb(0xc8, 0x4b, 0x4b),
                            "sin config — valores por defecto",
                        ),
                    };
                    ui.colored_label(color, "\u{25CF}");
                    ui.label(texto);
                });
            });
            ui.horizontal(|ui| {
                ui.label("Instancia:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.instance)
                        .hint_text("(por defecto)")
                        .desired_width(160.0),
                );
                if ui.button("Conectar").clicked() || (resp.lost_focus() && enter(ui)) {
                    self.reload();
                }
            });
            ui.add_space(6.0);
        });
    }

    fn side_nav(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navegacion")
            .resizable(false)
            .exact_width(190.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                for s in SECTIONS {
                    ui.selectable_value(&mut self.section, s, s.label_es());
                    ui.add_space(2.0);
                }
            });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("acciones").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if !self.model.status.is_empty() {
                    ui.label(&self.model.status);
                } else if self.model.is_dirty() {
                    ui.label("cambios sin guardar");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dirty = self.model.is_dirty();
                    if ui
                        .add_enabled(dirty, egui::Button::new("Guardar"))
                        .clicked()
                    {
                        if let Err(e) = self.model.save() {
                            self.model.status = e;
                        }
                    }
                    if ui
                        .add_enabled(dirty, egui::Button::new("Deshacer"))
                        .on_hover_text("Descarta los cambios sin guardar (relee el wayvpet.conf)")
                        .clicked()
                    {
                        self.model.reset();
                        self.edits.clear();
                    }
                    if ui
                        .button("Valores de fábrica")
                        .on_hover_text(
                            "Pone todos los ajustes en su valor por defecto (no toca \
                             el tema ni los dispositivos). Hay que Guardar para que quede.",
                        )
                        .clicked()
                    {
                        self.model.factory_reset();
                        self.edits.clear();
                    }
                });
            });
            ui.add_space(5.0);
        });
    }

    /// Modo experto: editores de texto crudo de los `.ini`.
    fn expert_panel(&mut self, ui: &mut egui::Ui) {
        if self.raw.is_empty() {
            self.raw = expert::gather(&self.model);
        }
        ui.colored_label(
            egui::Color32::from_rgb(0xd6, 0x9e, 0x2e),
            "⚠ Editas los ficheros directamente. Un error de sintaxis puede impedir \
             que wayvpet arranque; el parser descarta la línea mala pero conviene \
             revisar.",
        );
        ui.add_space(8.0);

        if self.raw.is_empty() {
            ui.label("No encuentro ningún fichero editable (¿sin wayvpet.conf?).");
            return;
        }

        let instance = opt(&self.instance).map(str::to_owned);
        let theme = self.model.cfg.theme.trim().to_owned();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for rf in &mut self.raw {
                    egui::CollapsingHeader::new(format!(
                        "{}{}",
                        rf.label,
                        if rf.dirty() { "  •" } else { "" }
                    ))
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.weak(
                            egui::RichText::new(rf.path.display().to_string())
                                .small()
                                .monospace(),
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut rf.text)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(14),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(rf.dirty(), egui::Button::new("Guardar"))
                                .clicked()
                            {
                                rf.save(instance.as_deref(), &theme);
                            }
                            if ui
                                .add_enabled(rf.dirty(), egui::Button::new("Descartar"))
                                .clicked()
                            {
                                rf.reload();
                            }
                            if !rf.status.is_empty() {
                                ui.label(egui::RichText::new(&rf.status).small());
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
            });
    }
}

/// ¿Se acaba de pulsar Enter en este `ui`?
fn enter(ui: &egui::Ui) -> bool {
    ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// Reescala `v` de `[a0,a1]` a `[b0,b1]` (redondeo al entero más cercano).
fn rescale(v: i32, a0: i32, a1: i32, b0: i32, b1: i32) -> i32 {
    if a1 == a0 {
        return b0;
    }
    let t = f64::from(v - a0) / f64::from(a1 - a0);
    (f64::from(b0) + t * f64::from(b1 - b0)).round() as i32
}

/// Una fila de campo: etiqueta (negrita) + clave (gris), y debajo el widget.
fn field_row(
    ui: &mut egui::Ui,
    model: &mut Model,
    edits: &mut HashMap<&'static str, String>,
    f: &FieldMeta,
) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong(f.label_es);
        ui.weak(egui::RichText::new(f.key).monospace().small());
    });
    ui.label(egui::RichText::new(f.help_es).small().weak());
    ui.add_space(2.0);
    field_widget(ui, model, edits, f);
    ui.add_space(6.0);
    ui.separator();
}

/// Renderiza el widget de un campo según su `FieldKind` y aplica el cambio en
/// caliente (`Model::set`).
fn field_widget(
    ui: &mut egui::Ui,
    model: &mut Model,
    edits: &mut HashMap<&'static str, String>,
    f: &FieldMeta,
) {
    match f.kind {
        FieldKind::Int {
            min,
            max,
            step,
            unit,
        } => {
            let mut v: i32 = model
                .value(f.key)
                .and_then(|s| s.parse().ok())
                .unwrap_or(min)
                .clamp(min, max);
            let mut slider = egui::Slider::new(&mut v, min..=max);
            if step > 1 {
                slider = slider.step_by(f64::from(step));
            }
            if !unit.is_empty() {
                slider = slider.suffix(unit);
            }
            if ui.add(slider).changed() {
                if let Err(e) = model.set(f.key, &v.to_string()) {
                    model.status = e;
                }
            }
        }
        FieldKind::IntScaled {
            store_min,
            store_max,
            ui_min,
            ui_max,
            step,
            unit,
        } => {
            let stored: i32 = model
                .value(f.key)
                .and_then(|s| s.parse().ok())
                .unwrap_or(store_min)
                .clamp(store_min, store_max);
            let mut shown = rescale(stored, store_min, store_max, ui_min, ui_max);
            let mut slider = egui::Slider::new(&mut shown, ui_min..=ui_max).suffix(unit);
            if step > 1 {
                slider = slider.step_by(f64::from(step));
            }
            if ui.add(slider).changed() {
                let store = rescale(shown, ui_min, ui_max, store_min, store_max);
                if let Err(e) = model.set(f.key, &store.to_string()) {
                    model.status = e;
                }
            }
        }
        FieldKind::Bool => {
            let mut b = model.value(f.key).is_some_and(|s| s == "1");
            if ui.checkbox(&mut b, "activado").changed() {
                if let Err(e) = model.set(f.key, if b { "1" } else { "0" }) {
                    model.status = e;
                }
            }
        }
        FieldKind::Enum(opts) => {
            let current = model.value(f.key).unwrap_or_default();
            let mut sel = current.clone();
            egui::ComboBox::from_id_salt(f.key)
                .selected_text(&sel)
                .show_ui(ui, |ui| {
                    for o in opts {
                        ui.selectable_value(&mut sel, (*o).to_owned(), *o);
                    }
                });
            if sel != current {
                if let Err(e) = model.set(f.key, &sel) {
                    model.status = e;
                }
            }
        }
        FieldKind::Time | FieldKind::Text | FieldKind::List => {
            // `monitor` es una lista editable (coma); `*_device` / `*_name` son
            // repetibles y de solo lectura aquí (se editan en el .conf).
            let editable =
                matches!(f.kind, FieldKind::Text | FieldKind::Time) || f.key == "monitor";
            let buf = edits
                .entry(f.key)
                .or_insert_with(|| model.value(f.key).unwrap_or_default());

            if editable {
                let hint = match f.kind {
                    FieldKind::Time => "HH:MM",
                    FieldKind::List => "eDP-1, HDMI-A-1",
                    _ => "",
                };
                let resp = ui.add(
                    egui::TextEdit::singleline(buf)
                        .hint_text(hint)
                        .desired_width(240.0),
                );
                if resp.lost_focus() {
                    let v = buf.clone();
                    if let Err(e) = model.set(f.key, &v) {
                        model.status = e;
                        *buf = model.value(f.key).unwrap_or_default();
                    }
                }
            } else {
                ui.add_enabled(false, egui::TextEdit::singleline(buf).desired_width(240.0));
                ui.label(
                    egui::RichText::new("se edita en el wayvpet.conf")
                        .small()
                        .weak(),
                );
            }
        }
    }
}
