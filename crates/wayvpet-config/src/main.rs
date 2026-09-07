//! `wayvpet-config` — ventana gráfica de configuración de wayvpet (spec 0007,
//! Fase 4). La lanza el ítem "Configurar…" del menú de la bandeja.
//!
//! Toolkit: `egui`/`eframe` — se dibuja a sí misma, así que se ve igual en
//! GNOME, KDE, COSMIC, Sway/Hyprland… sin librerías de toolkit del sistema.
//!
//! Cada campo de `field_meta` se renderiza según su `FieldKind` y se aplica **en
//! vivo** por IPC `SET`; Guardar / Deshacer / Valores de fábrica; se cierra sola
//! si la instancia que la abrió desaparece. Secciones especiales: galería de
//! temas con miniaturas (`THUMB`) + "mapa de pantalla" + selector de monitor
//! (Posición), modo experto (editar los `.ini`), Presets y perfiles, asistente
//! de primer uso. Falta de la fase: solo la matriz manual GNOME/KDE/Sway.

mod expert;
mod model;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use model::{Model, Source};
use wayvpet_common::field_meta::{FieldKind, FieldMeta, Section, FIELDS};
use wayvpet_common::ipc;

/// Secciones en el orden en que se muestran en la navegación lateral.
const SECTIONS: [Section; 8] = [
    Section::Position,
    Section::Appearance,
    Section::Input,
    Section::Sleep,
    Section::Theme,
    Section::Advanced,
    Section::Expert,
    Section::Presets,
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
    /// Asistente de primer uso: `Some` mientras no haya `wayvpet.conf`.
    wizard: Option<Wizard>,
    /// Búfer del nombre para "Guardar como perfil".
    profile_name: String,
    /// Miniaturas de temas ya cargadas (`None` = se intentó y no hubo).
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
}

/// Estado del asistente de primer uso (spec 0007 M5).
struct Wizard {
    step: u8,
    themes: Vec<String>,
}

impl App {
    fn new(instance: String) -> Self {
        let model = Model::load(opt(&instance));
        let tied_to_instance = model.source == Source::Instance;
        // Primer uso: sin instancia y sin fichero → guía en 3 pasos.
        let wizard = (model.source == Source::Defaults).then(|| Wizard {
            step: 0,
            themes: expert::installed_themes(),
        });
        Self {
            model,
            section: Section::Position,
            instance,
            edits: HashMap::new(),
            tied_to_instance,
            last_ping: Instant::now(),
            ping_fails: 0,
            raw: Vec::new(),
            wizard,
            profile_name: String::new(),
            thumbs: HashMap::new(),
        }
    }

    fn reload(&mut self) {
        self.model = Model::load(opt(&self.instance));
        self.tied_to_instance = self.model.source == Source::Instance;
        self.ping_fails = 0;
        self.edits.clear();
        self.raw.clear();
        self.thumbs.clear();
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
        if self.wizard.is_some() {
            self.wizard_ui(ctx);
            return;
        }
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
            if self.section == Section::Theme {
                self.theme_panel(ui);
                return;
            }
            if self.section == Section::Presets {
                self.presets_panel(ui);
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
                    if self.section == Section::Position {
                        screen_map(ui, &mut self.model);
                        ui.add_space(10.0);
                        monitor_picker(ui, &mut self.model);
                        ui.add_space(10.0);
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

    /// Sección "Tema": galería de temas instalados (clic para cambiar en vivo) +
    /// campo de texto para una ruta a una carpeta de tema propia.
    /// Sección "Presets": botones que aplican un `.conf` parcial de un tirón.
    fn presets_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "Un preset cambia varias opciones a la vez (sobre la configuración              actual). Se aplica al instante; pulsa Guardar si quieres que quede.",
        );
        ui.add_space(8.0);
        if self.model.presets.is_empty() {
            ui.label("No hay presets (necesita una instancia en marcha, o instala presets en ~/.local/share/wayvpet/presets/).");
            return;
        }
        let mut pick: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            for name in &self.model.presets {
                if ui.button(name).clicked() {
                    pick = Some(name.clone());
                }
            }
        });
        if let Some(n) = pick {
            self.model.apply_preset(&n);
            self.edits.clear();
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);
        ui.heading("Perfiles");
        ui.label(
            "Un perfil es una configuración **completa** con nombre              (trabajo / juego / streaming…). Cambiar de perfil reemplaza tu              wayvpet.conf por el suyo.",
        );
        ui.add_space(8.0);

        if !self.model.profiles.is_empty() {
            let active = self.model.active_profile.clone();
            let mut switch: Option<String> = None;
            ui.horizontal_wrapped(|ui| {
                for name in &self.model.profiles {
                    let is_active = active.as_deref() == Some(name.as_str());
                    if ui.selectable_label(is_active, name).clicked() && !is_active {
                        switch = Some(name.clone());
                    }
                }
            });
            if let Some(n) = switch {
                self.model.switch_profile(&n);
                self.edits.clear();
            }
            ui.add_space(6.0);
        } else {
            ui.small("No hay perfiles guardados todavía.");
            ui.add_space(6.0);
        }

        ui.horizontal(|ui| {
            ui.label("Guardar la config actual como:");
            ui.add(
                egui::TextEdit::singleline(&mut self.profile_name)
                    .hint_text("nombre")
                    .desired_width(140.0),
            );
            let ok = self
                .profile_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && !self.profile_name.is_empty();
            if ui
                .add_enabled(ok, egui::Button::new("Guardar perfil"))
                .clicked()
            {
                let n = self.profile_name.clone();
                self.model.save_profile(&n);
                self.profile_name.clear();
            }
        });
    }

    /// Miniatura de un tema (lazy + cacheada). Pide `THUMB <name>` a la
    /// instancia, que rasteriza un fotograma a PNG; se decodifica y se sube como
    /// textura. `None` si no hay instancia o algo falla.
    fn thumb(
        &mut self,
        ctx: &egui::Context,
        instance: Option<&str>,
        name: &str,
    ) -> Option<egui::TextureHandle> {
        if let Some(cached) = self.thumbs.get(name) {
            return cached.clone();
        }
        let tex = load_thumb(ctx, instance, name);
        self.thumbs.insert(name.to_owned(), tex.clone());
        tex
    }

    fn theme_panel(&mut self, ui: &mut egui::Ui) {
        let active = self.model.active_theme().to_owned();

        if self.model.themes.is_empty() {
            ui.label(
                "La galería de temas necesita una instancia de wayvpet en marcha. \
                 Mientras tanto, escribe el nombre o la ruta del tema abajo.",
            );
        } else {
            ui.label("Clic en un tema para activarlo (se aplica al instante):");
            ui.add_space(6.0);
            let mut pick: Option<String> = None;
            let names = self.model.themes.clone();
            let instance = opt(&self.instance).map(str::to_owned);
            let ctx = ui.ctx().clone();
            ui.horizontal_wrapped(|ui| {
                for name in &names {
                    let is_active = *name == active;
                    let label = if name == "embedded" {
                        "vpet embebido"
                    } else {
                        name.as_str()
                    };
                    ui.allocate_ui(egui::vec2(96.0, 116.0), |ui| {
                        ui.vertical_centered(|ui| {
                            let tex = self.thumb(&ctx, instance.as_deref(), name);
                            if let Some(t) = tex {
                                let img =
                                    egui::Image::from_texture((t.id(), egui::vec2(72.0, 72.0)))
                                        .maintain_aspect_ratio(true)
                                        .max_size(egui::vec2(72.0, 72.0));
                                if ui
                                    .add(egui::ImageButton::new(img).selected(is_active))
                                    .clicked()
                                    && !is_active
                                {
                                    pick = Some(name.clone());
                                }
                            } else if ui.selectable_label(is_active, "▢").clicked() && !is_active
                            {
                                pick = Some(name.clone());
                            }
                            ui.small(label);
                        });
                    });
                }
            });
            if let Some(name) = pick {
                self.model.set_theme(&name);
                self.edits.remove("theme");
            }
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Tema propio (ruta a una carpeta)").strong());
        if let Some(meta) = FIELDS.iter().find(|f| f.key == "theme") {
            field_widget(ui, &mut self.model, &mut self.edits, meta);
            ui.small(meta.help_es);
        }
    }
}

/// "Mapa de pantalla": un rectángulo a escala de la salida donde se arrastra un
/// punto para colocar el vpet. Trata los offsets como desde el **centro** de la
/// pantalla (exacto para `classic`; aproximado para temas con anclaje distinto,
/// pero los deslizadores y la vista previa en vivo afinan).
fn screen_map(ui: &mut egui::Ui, model: &mut Model) {
    let (sw, sh) = model.screen;
    let (sw, sh) = (sw.max(1) as f32, sh.max(1) as f32);
    let mw = 360.0_f32;
    let mh = (mw * sh / sw).clamp(120.0, 260.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(mw, mh), egui::Sense::hover());
    let p = ui.painter_at(rect);

    p.rect_filled(rect, 4.0, egui::Color32::from_gray(38));
    // borde con 4 segmentos (API estable en cualquier versión de egui)
    let g = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(90));
    for (a, b) in [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ] {
        p.line_segment([a, b], g);
    }
    let c = rect.center();
    let cross = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(80));
    p.line_segment(
        [egui::pos2(c.x - 7.0, c.y), egui::pos2(c.x + 7.0, c.y)],
        cross,
    );
    p.line_segment(
        [egui::pos2(c.x, c.y - 7.0), egui::pos2(c.x, c.y + 7.0)],
        cross,
    );

    let (kx, ky) = (mw / sw, mh / sh);
    let cur_x = model
        .value("cat_x_offset")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    let cur_y = model
        .value("cat_y_offset")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    let pad = egui::vec2(6.0, 6.0);
    let inner = egui::Rect::from_min_max(rect.min + pad, rect.max - pad);
    let dot = inner.clamp(egui::pos2(c.x + cur_x * kx, c.y + cur_y * ky));

    let resp = ui.interact(
        egui::Rect::from_center_size(dot, egui::vec2(18.0, 18.0)),
        ui.id().with("screenmap_dot"),
        egui::Sense::drag(),
    );
    p.circle_filled(dot, 7.0, egui::Color32::from_rgb(0xf4, 0xc8, 0x28));
    p.circle_stroke(dot, 7.0, egui::Stroke::new(1.5_f32, egui::Color32::BLACK));

    if resp.dragged() {
        let np = inner.clamp(dot + resp.drag_delta());
        let nx = (((np.x - c.x) / kx).round() as i32).clamp(-2560, 2560);
        let ny = (((np.y - c.y) / ky).round() as i32).clamp(-1600, 1600);
        let _ = model.set("cat_x_offset", &nx.to_string());
        let _ = model.set("cat_y_offset", &ny.to_string());
    }
    ui.small("Arrastra el punto para colocar el vpet (aproximado; afina con los deslizadores).");
}

impl App {
    /// Asistente de primer uso: 3 pasos → crea `wayvpet.conf`.
    fn wizard_ui(&mut self, ctx: &egui::Context) {
        let Some(w) = self.wizard.as_ref() else {
            return;
        };
        let step = w.step;
        let mut goto: Option<u8> = None;
        let mut finish = false;
        let mut skip = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.heading("Bienvenido a wayvpet");
            ui.label("Vamos a crear tu configuración. Puedes cambiarlo todo luego.");
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("Paso {} de 3", step + 1))
                    .small()
                    .weak(),
            );
            ui.separator();
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match step {
                    0 => {
                        ui.strong("Posición y tamaño");
                        ui.small("Dónde aparece el vpet y cómo de grande.");
                        ui.add_space(8.0);
                        for key in ["cat_align", "cat_x_offset", "cat_y_offset", "cat_height"] {
                            if let Some(m) = FIELDS.iter().find(|f| f.key == key) {
                                field_row(ui, &mut self.model, &mut self.edits, m);
                            }
                        }
                    }
                    1 => {
                        ui.strong("Entrada");
                        ui.small(
                            "El teclado se detecta solo. Aquí solo el ratón (opcional): \
                         el vpet también anima una pata al moverlo o clicar.",
                        );
                        ui.add_space(8.0);
                        for key in ["enable_mouse", "mouse_paw"] {
                            if let Some(m) = FIELDS.iter().find(|f| f.key == key) {
                                field_row(ui, &mut self.model, &mut self.edits, m);
                            }
                        }
                    }
                    _ => {
                        ui.strong("Tema");
                        ui.small("El aspecto del vpet. Puedes cambiarlo cuando quieras.");
                        ui.add_space(8.0);
                        let active = self.model.active_theme().to_owned();
                        ui.horizontal_wrapped(|ui| {
                            for name in &self.wizard.as_ref().unwrap().themes {
                                let is_active = *name == active;
                                let label = if name == "embedded" {
                                    "vpet embebido".to_owned()
                                } else {
                                    name.clone()
                                };
                                if ui.selectable_label(is_active, label).clicked() {
                                    self.model.cfg.theme = if name == "embedded" {
                                        String::new()
                                    } else {
                                        name.clone()
                                    };
                                }
                            }
                        });
                    }
                });
        });

        egui::TopBottomPanel::bottom("wiz_nav").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Omitir (valores por defecto)").clicked() {
                    skip = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if step == 2 {
                        if ui.button("Crear configuración").clicked() {
                            finish = true;
                        }
                    } else if ui.button("Siguiente").clicked() {
                        goto = Some(step + 1);
                    }
                    if step > 0 && ui.button("Atrás").clicked() {
                        goto = Some(step - 1);
                    }
                });
            });
            ui.add_space(6.0);
        });

        if let Some(s) = goto {
            if let Some(w) = self.wizard.as_mut() {
                w.step = s;
            }
        }
        if skip {
            self.model.cfg = wayvpet_common::config::factory_config();
            finish = true;
        }
        if finish {
            self.finish_wizard();
        }
    }

    /// Escribe la config del asistente al `wayvpet.conf` y sale del modo guía.
    fn finish_wizard(&mut self) {
        let path = self
            .model
            .path
            .clone()
            .or_else(wayvpet_common::io::resolve_config_path_real);
        match path {
            Some(p) => match wayvpet_common::io::save_atomic(&p, &self.model.cfg.to_ini()) {
                Ok(()) => {
                    self.wizard = None;
                    self.reload();
                    self.model.status = format!("configuración creada en {}", p.display());
                }
                Err(e) => self.model.status = format!("no se pudo escribir {}: {e}", p.display()),
            },
            None => self.model.status = "no sé dónde crear el wayvpet.conf (¿HOME?)".to_owned(),
        }
    }
}

/// Selector de monitor: una casilla por salida conectada (`OUTPUTS`). El
/// conjunto marcado forma la clave `monitor` (lista por comas; vacío = todas /
/// la que elija el compositor). El cambio se aplica **al reiniciar**.
fn monitor_picker(ui: &mut egui::Ui, model: &mut Model) {
    if model.outputs.is_empty() {
        return; // sin instancia no sabemos qué salidas hay; queda el campo de texto
    }
    ui.strong("Monitores");
    let current: std::collections::BTreeSet<String> = model
        .value("monitor")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    let mut next = current.clone();
    ui.horizontal_wrapped(|ui| {
        for name in &model.outputs {
            let mut on = next.contains(name);
            if ui.checkbox(&mut on, name).changed() {
                if on {
                    next.insert(name.clone());
                } else {
                    next.remove(name);
                }
            }
        }
    });
    if next != current {
        let joined = next.iter().cloned().collect::<Vec<_>>().join(",");
        let _ = model.set("monitor", &joined);
        model.status = "monitores: se aplica al reiniciar wayvpet".to_owned();
    }
    ui.small("Vacío = donde elija el compositor. El cambio surte efecto al reiniciar.");
}

/// Pide `THUMB <name>`, lee el PNG que deja la instancia y lo sube como textura.
fn load_thumb(
    ctx: &egui::Context,
    instance: Option<&str>,
    name: &str,
) -> Option<egui::TextureHandle> {
    let reply = ipc::send_request(instance, &format!("THUMB {name}")).ok()?;
    let path = reply.strip_prefix("OK ")?.trim();
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let img = egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &buf[..info.buffer_size()],
    );
    Some(ctx.load_texture(format!("thumb-{name}"), img, egui::TextureOptions::LINEAR))
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
