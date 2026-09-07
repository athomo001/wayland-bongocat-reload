//! Diálogo del aviso de nueva versión (spec 0015 M4/M5). Ventana pequeña e
//! independiente que abre el ítem "🔔 Versión nueva" del tray
//! (`wayvpet-config --update-dialog`).
//!
//! No enlaza nada de red: para los datos y la descarga llama al helper
//! `wayvpet-update-check` (`--print-notice` y `--download`) y lee su stdout. Si
//! el helper no está instalado, muestra el aviso y ofrece abrir el navegador.
//!
//! **Nunca instala** la actualización: "Descargar" deja el paquete en
//! `~/Descargas` verificado por SHA-256 y abre la carpeta; instalar lo hace el
//! usuario con su gestor.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use wayvpet_common::io;

const HELPER: &str = "wayvpet-update-check";

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("wayvpet · Versión nueva")
            .with_inner_size([460.0, 430.0])
            .with_min_inner_size([360.0, 260.0]),
        ..Default::default()
    };
    eframe::run_native(
        "wayvpet-update-dialog",
        options,
        Box::new(|_cc| Ok(Box::new(UpdateDialog::new()))),
    )
}

struct Notice {
    version: String,
    date: String,
    url: String,
    notes: String,
}

enum Load {
    UpToDate,
    Failed(String),
    Ready(Notice),
}

enum DlMsg {
    Progress { done: u64, total: u64 },
    Done(String),
    Error(String),
}

enum Dl {
    Idle,
    Running { done: u64, total: u64 },
    Done(String),
    Error(String),
}

struct UpdateDialog {
    load: Load,
    dl: Dl,
    dl_rx: Option<Receiver<DlMsg>>,
    channel: Option<String>,
}

impl UpdateDialog {
    fn new() -> Self {
        Self {
            load: load_notice(),
            dl: Dl::Idle,
            dl_rx: None,
            channel: io::install_channel_real(),
        }
    }

    fn start_download(&mut self) {
        let (tx, rx) = mpsc::channel();
        let channel = self.channel.clone();
        thread::spawn(move || {
            let mut cmd = Command::new(HELPER);
            if let Some(c) = &channel {
                cmd.arg("--channel").arg(c);
            }
            cmd.arg("--download")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(DlMsg::Error(format!("no pude lanzar {HELPER}: {e}")));
                    return;
                }
            };
            let Some(out) = child.stdout.take() else {
                let _ = tx.send(DlMsg::Error("sin salida del helper".into()));
                return;
            };
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(rest) = line.strip_prefix("PROGRESS ") {
                    let mut it = rest.split_whitespace();
                    let done = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let total = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let _ = tx.send(DlMsg::Progress { done, total });
                } else if let Some(p) = line.strip_prefix("OK ") {
                    let _ = tx.send(DlMsg::Done(p.to_string()));
                } else if let Some(e) = line.strip_prefix("ERROR ") {
                    let _ = tx.send(DlMsg::Error(e.to_string()));
                }
            }
            let _ = child.wait();
        });
        self.dl = Dl::Running { done: 0, total: 0 };
        self.dl_rx = Some(rx);
    }

    fn pump(&mut self) {
        // Vaciar a un buffer local primero: no se puede tocar `self.dl_rx`
        // mientras está prestado por `try_iter`.
        let msgs: Vec<DlMsg> = match &self.dl_rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for msg in msgs {
            match msg {
                DlMsg::Progress { done, total } => self.dl = Dl::Running { done, total },
                DlMsg::Done(p) => {
                    self.dl = Dl::Done(p);
                    self.dl_rx = None;
                }
                DlMsg::Error(e) => {
                    self.dl = Dl::Error(e);
                    self.dl_rx = None;
                }
            }
        }
    }
}

impl eframe::App for UpdateDialog {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        if matches!(self.dl, Dl::Running { .. }) {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            match &self.load {
                Load::UpToDate => {
                    ui.heading("Estás al día");
                    ui.label("No hay ninguna versión más nueva.");
                    ui.add_space(8.0);
                    if ui.button("Cerrar").clicked() {
                        close(ctx);
                    }
                }
                Load::Failed(e) => {
                    ui.heading("No se pudo comprobar");
                    ui.label(e.as_str());
                    ui.add_space(8.0);
                    if ui.button("Cerrar").clicked() {
                        close(ctx);
                    }
                }
                Load::Ready(n) => {
                    ui.heading(format!("🔔 wayvpet v{}", n.version));
                    let date = n.date.split('T').next().unwrap_or("");
                    if !date.is_empty() {
                        ui.label(format!("Publicada: {date}"));
                    }
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let notes = if n.notes.trim().is_empty() {
                                "(sin notas)"
                            } else {
                                n.notes.trim()
                            };
                            ui.label(notes);
                        });
                    ui.separator();
                    dl_section(ui, ctx, self, &n.url.clone());
                }
            }
        });
    }
}

/// Sección inferior: botones / progreso / resultado de la descarga.
fn dl_section(ui: &mut egui::Ui, ctx: &egui::Context, app: &mut UpdateDialog, url: &str) {
    match &app.dl {
        Dl::Idle => {
            ui.horizontal(|ui| {
                if ui.button("Descargar").clicked() {
                    app.start_download();
                }
                if ui.button("Ver en el navegador").clicked() {
                    xdg_open(url);
                    close(ctx);
                }
                if ui.button("Ahora no").clicked() {
                    close(ctx);
                }
            });
            ui.add_space(2.0);
            ui.small("«Descargar» deja el paquete verificado en ~/Descargas. No instala nada.");
        }
        Dl::Running { done, total } => {
            let frac = if *total > 0 {
                *done as f32 / *total as f32
            } else {
                0.0
            };
            let bar = egui::ProgressBar::new(frac.clamp(0.0, 1.0)).show_percentage();
            ui.add(bar);
            ui.small(format!("Descargando… {} / {}", human(*done), human(*total)));
        }
        Dl::Done(path) => {
            ui.label(format!("✓ Descargado en {path}"));
            ui.small("Ábrelo con tu gestor para instalar; wayvpet no lo hace por ti.");
            ui.horizontal(|ui| {
                if ui.button("Abrir carpeta").clicked() {
                    let dir = std::path::Path::new(path)
                        .parent()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| ".".into());
                    xdg_open(&dir);
                }
                if ui.button("Cerrar").clicked() {
                    close(ctx);
                }
            });
        }
        Dl::Error(e) => {
            ui.colored_label(egui::Color32::from_rgb(0xd0, 0x50, 0x50), format!("✗ {e}"));
            ui.horizontal(|ui| {
                if ui.button("Reintentar").clicked() {
                    app.start_download();
                }
                if ui.button("Ver en el navegador").clicked() {
                    xdg_open(url);
                    close(ctx);
                }
                if ui.button("Cerrar").clicked() {
                    close(ctx);
                }
            });
        }
    }
}

fn close(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}

fn xdg_open(target: &str) {
    if target.is_empty() {
        return;
    }
    let _ = Command::new("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Bytes → texto corto (`1.2 MB`).
fn human(n: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// Llama a `wayvpet-update-check --print-notice` y parsea su salida.
fn load_notice() -> Load {
    let out = match Command::new(HELPER).arg("--print-notice").output() {
        Ok(o) => o,
        Err(e) => {
            return Load::Failed(format!(
                "no encuentro «{HELPER}» (paquete wayvpet-update): {e}"
            ))
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let text = text.trim();
    if text == "al día" {
        return Load::UpToDate;
    }
    if !out.status.success() && text.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Load::Failed(err.trim().to_string());
    }
    parse_print_notice(text)
}

fn parse_print_notice(text: &str) -> Load {
    let mut version = String::new();
    let mut date = String::new();
    let mut url = String::new();
    let mut notes = String::new();
    let mut in_notes = false;
    for line in text.lines() {
        if in_notes {
            notes.push_str(line);
            notes.push('\n');
        } else if line == "---" {
            in_notes = true;
        } else if let Some(v) = line.strip_prefix("version=") {
            version = v.trim().to_string();
        } else if let Some(d) = line.strip_prefix("date=") {
            date = d.trim().to_string();
        } else if let Some(u) = line.strip_prefix("url=") {
            url = u.trim().to_string();
        }
    }
    if version.is_empty() {
        return Load::Failed("el helper no devolvió una versión".into());
    }
    Load::Ready(Notice {
        version,
        date,
        url,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_la_salida_de_print_notice() {
        let s =
            "version=0.6.0\ndate=2026-09-06T12:00:00Z\nurl=https://x/rel\n---\n## Cambios\n- uno\n";
        let Load::Ready(n) = parse_print_notice(s) else {
            panic!("esperaba Ready");
        };
        assert_eq!(n.version, "0.6.0");
        assert_eq!(n.date.split('T').next().unwrap(), "2026-09-06");
        assert_eq!(n.url, "https://x/rel");
        assert!(n.notes.contains("## Cambios") && n.notes.contains("- uno"));
    }

    #[test]
    fn sin_version_es_fallo() {
        assert!(matches!(
            parse_print_notice("date=x\n---\nhola"),
            Load::Failed(_)
        ));
    }

    #[test]
    fn human_legible() {
        assert_eq!(human(0), "0 B");
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(human(5 * 1024 * 1024), "5.0 MB");
    }
}
