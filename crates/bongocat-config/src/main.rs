//! `bongocat-config` — ventana gráfica de configuración de bongocat (spec 0007,
//! Fase 4). La lanza el ítem "Configurar…" del menú de la bandeja.
//!
//! Toolkit: `egui`/`eframe` — se dibuja a sí misma, así que se ve igual en
//! GNOME, KDE, COSMIC, Sway/Hyprland… sin librerías de toolkit del sistema.
//!
//! **M2 (esta versión):** andamiaje — abre la ventana, navega las secciones de
//! `field_meta`, y carga el modelo desde la instancia viva (IPC `DUMP`) o del
//! fichero. La edición campo a campo llega en M3.

mod model;

use bongocat_common::field_meta::{Section, FIELDS};
use model::{Model, Source};

/// Secciones en el orden en que se muestran en la navegación lateral.
const SECTIONS: [Section; 6] = [
    Section::Position,
    Section::Appearance,
    Section::Input,
    Section::Sleep,
    Section::Theme,
    Section::Advanced,
];

fn main() -> eframe::Result {
    // `--instance <NOMBRE>` preselecciona la instancia de esa salida.
    let instance = std::env::args()
        .skip_while(|a| a != "--instance")
        .nth(1)
        .unwrap_or_default();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("bongocat · Configurar")
            .with_inner_size([760.0, 540.0])
            .with_min_inner_size([520.0, 380.0]),
        ..Default::default()
    };

    eframe::run_native(
        "bongocat-config",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(instance)))),
    )
}

struct App {
    model: Model,
    section: Section,
    /// Instancia elegida (vacío = la por defecto). Editable en la barra superior.
    instance: String,
}

impl App {
    fn new(instance: String) -> Self {
        let model = Model::load(App::instance_opt(&instance));
        Self {
            model,
            section: Section::Position,
            instance,
        }
    }

    fn instance_opt(instance: &str) -> Option<&str> {
        Some(instance.trim()).filter(|s| !s.is_empty())
    }

    fn reload(&mut self) {
        self.model = Model::load(App::instance_opt(&self.instance));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("cabecera").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Configurar bongocat");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (color, texto) = match self.model.source {
                        Source::Instance => (
                            egui::Color32::from_rgb(0x3f, 0xb9, 0x50),
                            "conectado a la instancia",
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
            ui.add_space(4.0);
        });

        egui::SidePanel::left("navegacion")
            .resizable(false)
            .exact_width(160.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                for s in SECTIONS {
                    ui.selectable_value(&mut self.section, s, s.label_es());
                }
                ui.separator();
                if ui.button("Recargar").clicked() {
                    self.reload();
                }
            });

        egui::TopBottomPanel::bottom("estado").show(ctx, |ui| {
            ui.add_space(2.0);
            let mut line = String::new();
            if self.model.is_dirty() {
                line.push_str("cambios sin guardar · ");
            }
            if self.model.status.is_empty() {
                line.push_str("M3 añadirá la edición campo a campo y el botón Guardar");
            } else {
                line.push_str(&format!("avisos: {}", self.model.status));
            }
            ui.small(line);
            ui.add_space(2.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(self.section.label_es());
            ui.add_space(8.0);
            let fuente = if self.model.source.connected() {
                "valores en vivo de la instancia"
            } else {
                "valores del fichero de configuración"
            };
            ui.label(format!(
                "Campos de esta sección · {fuente} · solo lectura (M2):"
            ));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("campos")
                    .num_columns(2)
                    .spacing([16.0, 10.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for f in FIELDS.iter().filter(|f| f.section == self.section) {
                            ui.vertical(|ui| {
                                ui.strong(f.label_es);
                                ui.weak(egui::RichText::new(f.key).monospace().small());
                            });
                            let val = self.model.value(f.key).unwrap_or_else(|| "—".into());
                            ui.label(egui::RichText::new(val).monospace())
                                .on_hover_text(f.help_es);
                            ui.end_row();
                        }
                    });
            });
        });
    }
}
