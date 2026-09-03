//! Importador de mascotas de **wayland-vpets** (spec 0014 §5.5, hito M4).
//!
//! wayland-vpets (furudbat, MIT) describe sus sprite sheets en un `.conf` de
//! estilo INI con claves `custom_*`. Aquí vive la parte **pura**: leer ese
//! `.conf`, traducir sus claves a nuestro `theme_format = 3` y componer un
//! informe de lo que se mapeó, se ignoró o falta. La E/S (resolver el origen,
//! copiar los PNG, escribir el `theme.ini`, `theme check`) vive en el binario y
//! se apoya en esto.
//!
//! Principio de la spec: **nunca falla por un estado ausente**. Un estado que la
//! v1 no conduce (`working`/`moving`) se ignora con aviso; un estado canónico
//! que el pet no define se sustituye por su reserva (§5.2).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bongocat_common::config::split_line;
use bongocat_common::sheet::InputModel;

use crate::png_decode;
use crate::sheet_anim::StateId;

/// Tope al tamaño en disco de una hoja que se importa (igual que `theme.rs`).
const MAX_SHEET_BYTES: u64 = 16 * 1024 * 1024;

/// Un estado tal y como venía en el `.conf` de wayland-vpets, ya con la fila
/// normalizada a **1-based** (sea cual sea el `row_base` del origen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpetsState {
    pub name: String,
    /// Fila de la rejilla, 1-based.
    pub row: u32,
    /// Nº de frames (columnas consecutivas). 0 = no declarado.
    pub frames: u32,
    /// Columna inicial, 1-based (default 1).
    pub col_start: u32,
    /// `fps` propio del estado, si lo traía.
    pub fps: Option<u32>,
}

/// Un `.conf` de wayland-vpets ya parseado a estructura. Nunca falla: las claves
/// desconocidas se ignoran y lo ausente queda a `None` / vacío.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VpetsConf {
    /// `animation_name` (informativo; `custom` es lo normal para packs sueltos).
    pub animation_name: Option<String>,
    /// Ruta de la hoja tal cual venía (`custom_sprite_sheet_filename`), para
    /// copiarla; puede ser absoluta o relativa al `.conf`.
    pub sheet_path: Option<String>,
    /// `custom_frame_width` / `sprite_width` … si el pack lo declara.
    pub frame_w: Option<u32>,
    pub frame_h: Option<u32>,
    /// `fps` / `animation_speed`.
    pub default_fps: Option<u32>,
    /// Estados con al menos una clave `custom_<n>_*`, en orden de aparición.
    pub states: Vec<VpetsState>,
}

/// Acumula las claves `custom_<n>_*` de un estado mientras se parsea.
#[derive(Default)]
struct Accum {
    row: Option<u32>,
    frames: Option<u32>,
    col_start: Option<u32>,
    fps: Option<u32>,
}

/// Parsea el texto de un `.conf` de wayland-vpets. Acepta también nuestras
/// propias claves (`sheet`, `state_<n>_*`) para que reimportar un tema ya
/// convertido funcione.
#[must_use]
pub fn parse_vpets_conf(text: &str) -> VpetsConf {
    let mut conf = VpetsConf::default();
    let mut per_state: BTreeMap<String, Accum> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut row_base: u32 = 1;

    for raw in text.lines() {
        let s = raw.trim_start_matches([' ', '\t']);
        if s.is_empty() || s.starts_with('#') || s.starts_with(';') {
            continue;
        }
        let Some(l) = split_line(raw) else { continue };
        let (k, v) = (l.key.as_str(), l.value);
        let key = k.to_ascii_lowercase();
        match key.as_str() {
            "animation_name" => conf.animation_name = Some(v),
            "custom_sprite_sheet_filename" | "sprite_sheet" | "sheet" => {
                if !v.is_empty() {
                    conf.sheet_path = Some(v);
                }
            }
            "custom_frame_width" | "frame_width" | "sprite_width" | "frame_w" => {
                conf.frame_w = v.parse().ok().filter(|&n| n > 0);
            }
            "custom_frame_height" | "frame_height" | "sprite_height" | "frame_h" => {
                conf.frame_h = v.parse().ok().filter(|&n| n > 0);
            }
            "fps" | "animation_speed" | "default_fps" => {
                conf.default_fps = v.parse().ok().filter(|&n| n > 0);
            }
            "row_base" => row_base = v.parse().unwrap_or(1),
            _ => {
                // custom_<estado>_<campo>  o  state_<estado>_<campo>
                let rest = key
                    .strip_prefix("custom_")
                    .or_else(|| key.strip_prefix("state_"));
                let Some(rest) = rest else { continue };
                let Some((name, field)) = rest.rsplit_once('_') else {
                    continue;
                };
                // `custom_sprite_sheet_filename` ya se trató arriba; su rsplit
                // daría name="custom_sprite_sheet", field="filename": lo saltamos.
                if !is_state_field(field) {
                    continue;
                }
                if !per_state.contains_key(name) {
                    order.push(name.to_string());
                }
                let acc = per_state.entry(name.to_string()).or_default();
                let n: Option<u32> = v.parse().ok();
                match field {
                    "row" => acc.row = n,
                    "frames" => acc.frames = n,
                    "col" => acc.col_start = n,
                    "fps" => acc.fps = n,
                    _ => {}
                }
            }
        }
    }

    for name in order {
        let acc = &per_state[&name];
        // Fila a 1-based con independencia del `row_base` del origen.
        let row = acc
            .row
            .map_or(1, |r| r.saturating_sub(row_base).saturating_add(1));
        let col_start = acc
            .col_start
            .map_or(1, |c| c.saturating_sub(row_base).saturating_add(1));
        conf.states.push(VpetsState {
            name,
            row,
            frames: acc.frames.unwrap_or(0),
            col_start,
            fps: acc.fps.filter(|&n| n > 0),
        });
    }
    conf
}

/// ¿Es `field` el último token de una clave de estado que nos interesa? Se
/// compara contra el sufijo tras el **último** `_` (como hace `parse_sheet_ini`),
/// así que aquí solo caben tokens simples.
fn is_state_field(field: &str) -> bool {
    matches!(field, "row" | "frames" | "col" | "fps")
}

/// Deriva `(frame_w, frame_h)` del tamaño de la hoja cuando el `.conf` no los
/// declara (spec 0014 §5.5): columnas = máximo `col_start-1 + frames` sobre los
/// estados; filas = fila máxima. `None` si no hay datos suficientes o la hoja no
/// divide en partes enteras.
#[must_use]
pub fn derive_frame_size(sheet_w: u32, sheet_h: u32, states: &[VpetsState]) -> Option<(u32, u32)> {
    let cols = states
        .iter()
        .map(|s| (s.col_start.saturating_sub(1)).saturating_add(s.frames.max(1)))
        .max()
        .filter(|&c| c > 0)?;
    let rows = states.iter().map(|s| s.row).max().filter(|&r| r > 0)?;
    if sheet_w == 0 || sheet_h == 0 || sheet_w % cols != 0 || sheet_h % rows != 0 {
        return None;
    }
    Some((sheet_w / cols, sheet_h / rows))
}

/// Resultado de traducir un `.conf`: qué estados se mapearon, cuáles se
/// ignoraron (no conducibles en v1) y cuáles canónicos faltan y con qué reserva.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Estados traducidos (nombre canónico → nº de frames).
    pub mapped: Vec<(String, u32)>,
    /// Estados del pet que la v1 no conduce (`working`/`moving`…): se ignoran.
    pub ignored: Vec<String>,
    /// Estados canónicos ausentes → nombre de la reserva que se usará.
    pub missing: Vec<(String, String)>,
    /// Modelo de entrada detectado.
    pub input_model: InputModel,
}

/// Estados canónicos cuya ausencia merece una línea en el informe, con la
/// reserva que anunciará (§5.2). No es la cadena completa de `SheetAnim`, solo
/// lo que el usuario necesita saber al importar.
const CANONICAL_FALLBACK: &[(&str, &str)] = &[
    ("idle", "writing"),
    ("writing", "idle"),
    ("sleep", "boring → idle"),
];

impl ImportReport {
    /// Texto multilínea para imprimir al terminar el import (o en `--dry-run`).
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "modelo de entrada: {:?}", self.input_model);
        if self.mapped.is_empty() {
            let _ = writeln!(out, "  ¡ningún estado utilizable!");
        } else {
            let _ = write!(out, "  mapeados: ");
            let list: Vec<String> = self
                .mapped
                .iter()
                .map(|(n, f)| format!("{n}×{f}"))
                .collect();
            let _ = writeln!(out, "{}", list.join(", "));
        }
        if !self.ignored.is_empty() {
            let _ = writeln!(
                out,
                "  ignorados (sin disparador en v1): {}",
                self.ignored.join(", ")
            );
        }
        for (name, fb) in &self.missing {
            let _ = writeln!(out, "  falta '{name}' → se usará '{fb}'");
        }
        out
    }
}

/// Traduce un `VpetsConf` a un `theme.ini` de `theme_format = 3` y su informe.
///
/// `sheet_basename` es el nombre con el que se copiará la hoja al directorio del
/// tema (sin ruta). `frame_w`/`frame_h` ya resueltos (del `.conf` o derivados).
#[must_use]
pub fn translate(
    conf: &VpetsConf,
    name: &str,
    license: &str,
    sheet_basename: &str,
    frame_w: u32,
    frame_h: u32,
) -> (String, ImportReport) {
    let mut report = ImportReport::default();
    let mut drivable: Vec<(&VpetsState, StateId)> = Vec::new();
    for st in &conf.states {
        match StateId::from_theme_name(&st.name) {
            Some(id) => drivable.push((st, id)),
            None => report.ignored.push(st.name.clone()),
        }
    }

    let has_hands = drivable.iter().any(|(_, id)| {
        matches!(
            id,
            StateId::ActiveLeft | StateId::ActiveRight | StateId::ActiveBoth
        )
    });
    report.input_model = if has_hands {
        InputModel::Hands
    } else {
        InputModel::Activity
    };

    // Orden canónico de emisión; los que no estén aquí van después, alfabéticos.
    const CANON_ORDER: &[&str] = &[
        "idle",
        "boring",
        "start_writing",
        "writing",
        "end_writing",
        "happy",
        "sleep",
        "wake_up",
        "active_left",
        "active_right",
        "active_both",
    ];
    drivable.sort_by_key(|(st, _)| {
        CANON_ORDER
            .iter()
            .position(|c| *c == st.name)
            .unwrap_or(CANON_ORDER.len())
    });

    let default_fps = conf.default_fps.unwrap_or(12);
    let mut ini = String::new();
    let _ = writeln!(
        ini,
        "# Importado de wayland-vpets por `bongocat theme import-vpets`.\n\
         # No redistribuir arte de terceros; ver themes/COMUNIDAD.md.\n\
         name = {name}\n\
         license = {license}\n\
         theme_format = 3\n\
         theme_version = 1\n\
         \n\
         sheet = {sheet_basename}\n\
         frame_w = {frame_w}\n\
         frame_h = {frame_h}\n\
         default_fps = {default_fps}\n\
         scale_filter = nearest\n\
         input_model = {model}\n\
         row_base = 1",
        model = match report.input_model {
            InputModel::Hands => "hands",
            InputModel::Activity => "activity",
        }
    );

    for (st, id) in &drivable {
        let frames = st.frames.max(1);
        let _ = writeln!(ini);
        let _ = writeln!(ini, "state_{}_row = {}", st.name, st.row.max(1));
        let _ = writeln!(ini, "state_{}_frames = {frames}", st.name);
        if st.col_start > 1 {
            let _ = writeln!(ini, "state_{}_col = {}", st.name, st.col_start);
        }
        if let Some(f) = st.fps {
            let _ = writeln!(ini, "state_{}_fps = {f}", st.name);
        }
        report.mapped.push((st.name.clone(), frames));
        let _ = id;
    }

    for (canon, fb) in CANONICAL_FALLBACK {
        if !drivable.iter().any(|(st, _)| st.name == *canon) {
            report
                .missing
                .push(((*canon).to_string(), (*fb).to_string()));
        }
    }

    (ini, report)
}

// --- E/S: resolver el origen, copiar la hoja, escribir el tema ---------------

/// Opciones de `bongocat theme import-vpets`.
pub struct ImportArgs<'a> {
    /// Ruta del origen: carpeta de mascota, un `.conf`, o una hoja `.png`.
    pub source: &'a str,
    /// Nombre del tema de salida (default: derivado del origen).
    pub name: Option<&'a str>,
    /// Directorio del tema de salida (default: `$XDG_DATA_HOME/bongocat/themes/<name>`).
    pub out_dir: Option<PathBuf>,
    /// Solo imprimir el informe, no escribir nada.
    pub dry_run: bool,
    /// `frame_w` / `frame_h` forzados (si el `.conf` no los trae y no se pueden
    /// derivar de la hoja).
    pub frame_w: Option<u32>,
    pub frame_h: Option<u32>,
}

/// Ejecuta el import. Imprime el informe por stdout. Devuelve la ruta del tema
/// creado, o `Err` con un motivo legible. **Solo falla** si no hay ninguna hoja
/// utilizable o la E/S de escritura falla (spec 0014 §5.6).
pub fn run(args: &ImportArgs) -> Result<PathBuf, String> {
    let src = Path::new(args.source);
    let meta = std::fs::metadata(src).map_err(|e| format!("{}: {e}", args.source))?;

    // 1. Localizar el `.conf` (si lo hay) y la hoja.
    let (conf_text, sheet_path, base_dir) = if meta.is_dir() {
        let conf = find_conf(src);
        let conf_text = conf
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        (conf_text, None, src.to_path_buf())
    } else if src
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("conf"))
    {
        let conf_text =
            std::fs::read_to_string(src).map_err(|e| format!("{}: {e}", args.source))?;
        let base = src.parent().unwrap_or(Path::new(".")).to_path_buf();
        (conf_text, None, base)
    } else {
        // Una hoja suelta: sin `.conf`. Necesita `--frame-w/--frame-h`.
        let base = src.parent().unwrap_or(Path::new(".")).to_path_buf();
        (String::new(), Some(src.to_path_buf()), base)
    };

    let conf = parse_vpets_conf(&conf_text);

    // Hoja: la pasada directa, o la del `.conf` (relativa a su carpeta), o el
    // único PNG de la carpeta de la mascota.
    let sheet = sheet_path
        .or_else(|| conf.sheet_path.as_ref().map(|s| resolve_rel(&base_dir, s)))
        .or_else(|| single_png(&base_dir))
        .ok_or_else(|| {
            "no encuentro la hoja: pasa un .png, un .conf con custom_sprite_sheet_filename, \
             o una carpeta con un solo PNG"
                .to_string()
        })?;

    // 2. Validar y decodificar la hoja (rechaza dimensiones bomba antes de
    //    reservar; T-0014-seguridad).
    let smeta =
        std::fs::symlink_metadata(&sheet).map_err(|e| format!("{}: {e}", sheet.display()))?;
    if !smeta.is_file() {
        return Err(format!("{}: no es un fichero regular", sheet.display()));
    }
    if smeta.len() > MAX_SHEET_BYTES {
        return Err(format!(
            "{}: {} bytes, máximo {MAX_SHEET_BYTES}",
            sheet.display(),
            smeta.len()
        ));
    }
    let bytes = std::fs::read(&sheet).map_err(|e| format!("{}: {e}", sheet.display()))?;
    let frames =
        png_decode::decode_frames(&bytes).map_err(|e| format!("{}: {e}", sheet.display()))?;
    let (sheet_w, sheet_h) = (frames[0].w, frames[0].h);

    // 3. Resolver frame_w/frame_h: flags → .conf → derivado de la hoja.
    let (fw, fh) = args
        .frame_w
        .zip(args.frame_h)
        .or_else(|| conf.frame_w.zip(conf.frame_h))
        .or_else(|| derive_frame_size(sheet_w, sheet_h, &conf.states))
        .ok_or_else(|| {
            format!(
                "no puedo determinar el tamaño de frame de una hoja {sheet_w}×{sheet_h}; \
                 pasa --frame-w y --frame-h"
            )
        })?;

    // 4. Nombre y directorio de salida.
    let name = derive_name(args, &conf, src);
    if name.is_empty() || name.contains(['/', '\\', '\0']) || name.contains("..") {
        return Err(format!("nombre de tema inválido: '{name}'"));
    }
    let out = args
        .out_dir
        .clone()
        .unwrap_or_else(|| crate::theme::user_themes_dir().join(&name));

    let sheet_basename = sheet
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.contains(['/', '\\', '\0']))
        .unwrap_or("sheet.png");

    let (ini, report) = translate(&conf, &name, license_for(&conf), sheet_basename, fw, fh);

    println!("origen:  {}", args.source);
    println!(
        "hoja:    {} ({sheet_w}×{sheet_h}), frame {fw}×{fh}",
        sheet.display()
    );
    println!("destino: {}", out.display());
    print!("{}", report.render());

    if args.dry_run {
        println!("--dry-run: no se ha escrito nada.");
        return Ok(out);
    }
    if report.mapped.is_empty() {
        return Err("ningún estado utilizable en el origen; no creo el tema".to_string());
    }
    if out.exists() {
        return Err(format!("{} ya existe", out.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join(sheet_basename), &bytes)
        .map_err(|e| format!("{}: {e}", out.join(sheet_basename).display()))?;
    std::fs::write(out.join("theme.ini"), &ini)
        .map_err(|e| format!("{}: {e}", out.join("theme.ini").display()))?;

    println!("tema escrito. validando…");
    if !crate::theme::check(&out.to_string_lossy()) {
        return Err("el tema recién escrito no pasa `theme check`".to_string());
    }
    Ok(out)
}

/// Primer `bongocat.conf` / `*.conf` dentro de `dir`.
fn find_conf(dir: &Path) -> Option<PathBuf> {
    let pref = dir.join("bongocat.conf");
    if pref.is_file() {
        return Some(pref);
    }
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        (p.extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("conf"))
            && p.is_file())
        .then_some(p)
    })
}

/// Único fichero `.png` de `dir` (para "carpeta de mascota sin `.conf`").
fn single_png(dir: &Path) -> Option<PathBuf> {
    let mut hit = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) && p.is_file() {
            if hit.is_some() {
                return None; // ambiguo
            }
            hit = Some(p);
        }
    }
    hit
}

/// Resuelve `s` (la ruta de la hoja del `.conf`) relativa a `base` si no es
/// absoluta.
fn resolve_rel(base: &Path, s: &str) -> PathBuf {
    let p = Path::new(s);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// Nombre del tema: `--name`, o `animation_name` si no es `custom`, o el nombre
/// del fichero/carpeta de origen.
fn derive_name(args: &ImportArgs, conf: &VpetsConf, src: &Path) -> String {
    if let Some(n) = args.name {
        return n.to_string();
    }
    if let Some(a) = &conf.animation_name {
        if !a.is_empty() && a != "custom" {
            return a.clone();
        }
    }
    src.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("vpets-import")
        .to_string()
}

/// Licencia para el `theme.ini`: aviso genérico de que el arte importado puede
/// ser IP ajena (el usuario lo edita si sabe la licencia real).
fn license_for(_conf: &VpetsConf) -> &'static str {
    "\"arte importado de wayland-vpets — revisa la licencia antes de redistribuir\""
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = "\
animation_name = custom
custom_sprite_sheet_filename = /home/u/.local/share/wpets/charizard.png
fps = 15
custom_idle_row = 1
custom_idle_frames = 2
custom_writing_row = 2
custom_writing_frames = 4
custom_writing_fps = 20
custom_working_row = 3
custom_working_frames = 3
";

    #[test]
    fn parsea_claves_custom() {
        let c = parse_vpets_conf(CONF);
        assert_eq!(c.animation_name.as_deref(), Some("custom"));
        assert_eq!(
            c.sheet_path.as_deref(),
            Some("/home/u/.local/share/wpets/charizard.png")
        );
        assert_eq!(c.default_fps, Some(15));
        assert_eq!(c.states.len(), 3);
        let w = c.states.iter().find(|s| s.name == "writing").unwrap();
        assert_eq!((w.row, w.frames, w.fps), (2, 4, Some(20)));
    }

    #[test]
    fn row_base_0_se_normaliza_a_1() {
        let c = parse_vpets_conf(
            "row_base = 0\ncustom_idle_row = 0\ncustom_idle_frames = 1\n\
             custom_writing_row = 1\ncustom_writing_frames = 2\n",
        );
        assert_eq!(c.states[0].row, 1, "fila 0 (0-based) → 1 (1-based)");
        assert_eq!(c.states[1].row, 2);
    }

    #[test]
    fn deriva_tamano_de_frame_de_la_hoja() {
        let c = parse_vpets_conf(CONF);
        // 3 filas (working en la 3), máx 4 frames por fila → hoja 256×144
        // ⇒ frame 64×48.
        assert_eq!(derive_frame_size(256, 144, &c.states), Some((64, 48)));
        // Hoja que no divide en enteros → None (el llamante avisa).
        assert_eq!(derive_frame_size(250, 144, &c.states), None);
    }

    #[test]
    fn traduce_a_theme_ini_e_informa() {
        let c = parse_vpets_conf(CONF);
        let (ini, rep) = translate(&c, "Charizard", "IP de Nintendo", "charizard.png", 64, 48);
        assert!(ini.contains("theme_format = 3"));
        assert!(ini.contains("sheet = charizard.png"));
        assert!(ini.contains("frame_w = 64"));
        assert!(ini.contains("input_model = activity"));
        assert!(ini.contains("state_writing_row = 2"));
        assert!(ini.contains("state_writing_frames = 4"));
        assert!(ini.contains("state_writing_fps = 20"));
        // `working` no es conducible en v1.
        assert!(!ini.contains("state_working"));
        assert_eq!(rep.ignored, ["working"]);
        assert_eq!(
            rep.mapped,
            [("idle".to_string(), 2), ("writing".to_string(), 4)]
        );
        // Falta `sleep` → aparece en el informe con su reserva.
        assert!(rep.missing.iter().any(|(n, _)| n == "sleep"));
        assert!(!rep.missing.iter().any(|(n, _)| n == "idle"));
    }

    #[test]
    fn detecta_modelo_con_manos() {
        let c = parse_vpets_conf(
            "custom_idle_row = 1\ncustom_idle_frames = 1\n\
             custom_left_down_row = 2\ncustom_left_down_frames = 1\n\
             custom_right_down_row = 3\ncustom_right_down_frames = 1\n",
        );
        let (ini, rep) = translate(&c, "Bongo", "MIT", "s.png", 8, 8);
        assert_eq!(rep.input_model, InputModel::Hands);
        assert!(ini.contains("input_model = hands"));
        assert!(ini.contains("state_left_down_row = 2") || ini.contains("state_left-down_row"));
    }

    #[test]
    fn conf_vacio_no_panica() {
        let c = parse_vpets_conf("");
        let (_, rep) = translate(&c, "x", "y", "s.png", 8, 8);
        assert!(rep.mapped.is_empty());
        assert!(rep.render().contains("ningún estado utilizable"));
    }

    // --- E/S: extremo a extremo con una hoja PNG de juguete ------------------

    /// Directorio temporal único para un test (sin `tempfile`: nombre por
    /// nombre de test). Se borra al empezar por si quedó de una tanda previa.
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bongocat-import-{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Un PNG RGBA opaco de `w`×`h`, todo del mismo color, con la crate `png`.
    fn toy_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&[120u8, 90, 200, 255].repeat((w * h) as usize))
            .unwrap();
        drop(wr);
        out
    }

    #[test]
    fn import_extremo_a_extremo_desde_carpeta() {
        let dir = scratch("e2e");
        let pet = dir.join("pet");
        std::fs::create_dir_all(&pet).unwrap();
        std::fs::write(pet.join("sheet.png"), toy_png(180, 120)).unwrap();
        std::fs::write(
            pet.join("bongocat.conf"),
            "animation_name = custom\n\
             custom_sprite_sheet_filename = sheet.png\n\
             custom_frame_width = 60\ncustom_frame_height = 60\n\
             fps = 9\n\
             custom_idle_row = 1\ncustom_idle_frames = 2\n\
             custom_writing_row = 2\ncustom_writing_frames = 3\n\
             custom_moving_row = 3\ncustom_moving_frames = 1\n",
        )
        .unwrap();

        let out = dir.join("out");
        let args = ImportArgs {
            source: pet.to_str().unwrap(),
            name: Some("toy"),
            out_dir: Some(out.clone()),
            dry_run: false,
            frame_w: None,
            frame_h: None,
        };
        let created = run(&args).expect("import OK");
        assert_eq!(created, out);
        // La hoja se copió y el theme.ini es de formato 3 y válido.
        assert!(out.join("sheet.png").is_file());
        let ini = std::fs::read_to_string(out.join("theme.ini")).unwrap();
        assert!(ini.contains("theme_format = 3"));
        assert!(ini.contains("frame_w = 60"));
        assert!(ini.contains("state_writing_frames = 3"));
        assert!(!ini.contains("state_moving")); // no conducible
                                                // `theme check` sobre la carpeta creada pasa.
        assert!(crate::theme::check(&out.to_string_lossy()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_no_escribe_nada() {
        let dir = scratch("dry");
        let png_path = dir.join("s.png");
        std::fs::write(&png_path, toy_png(64, 32)).unwrap();
        let out = dir.join("out");
        let args = ImportArgs {
            source: png_path.to_str().unwrap(),
            name: Some("d"),
            out_dir: Some(out.clone()),
            dry_run: true,
            frame_w: Some(32),
            frame_h: Some(32),
        };
        run(&args).expect("dry-run OK");
        assert!(!out.exists(), "--dry-run no crea el directorio");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
