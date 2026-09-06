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

use wayvpet_common::config::split_line;
use wayvpet_common::sheet::InputModel;

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

/// Parsea un `--state` con forma `NOMBRE=row:R,frames:F[,fps:X][,col:C]`
/// (índices 1-based, spec 0014 §5.5). `Err` con motivo si no cuadra.
fn parse_state_spec(spec: &str) -> Result<VpetsState, String> {
    let (name, rest) = spec
        .split_once('=')
        .ok_or_else(|| format!("--state '{spec}': falta '=' (usa NOMBRE=row:R,frames:F)"))?;
    if name.is_empty() {
        return Err(format!("--state '{spec}': nombre vacío"));
    }
    let (mut row, mut frames, mut fps, mut col) = (None, None, None, None);
    for kv in rest.split(',').filter(|s| !s.is_empty()) {
        let (k, v) = kv
            .split_once(':')
            .ok_or_else(|| format!("--state '{spec}': '{kv}' no es clave:valor"))?;
        let n: u32 = v
            .parse()
            .map_err(|_| format!("--state '{spec}': '{v}' no es un número"))?;
        match k {
            "row" => row = Some(n),
            "frames" => frames = Some(n),
            "fps" => fps = Some(n),
            "col" => col = Some(n),
            other => return Err(format!("--state '{spec}': clave '{other}' desconocida")),
        }
    }
    let row = row.ok_or_else(|| format!("--state '{spec}': falta 'row'"))?;
    let frames = frames.ok_or_else(|| format!("--state '{spec}': falta 'frames'"))?;
    Ok(VpetsState {
        name: name.to_string(),
        row: row.max(1),
        frames: frames.max(1),
        col_start: col.unwrap_or(1).max(1),
        fps: fps.filter(|&n| n > 0),
    })
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
/// No emite la clave `name` (la antepone el llamante con el nombre de salida).
///
/// `sheet_basename` es el nombre con el que se copiará la hoja al directorio del
/// tema (sin ruta). `frame_w`/`frame_h` ya resueltos (del `.conf` o derivados).
#[must_use]
pub fn translate(
    conf: &VpetsConf,
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
        "# Importado de wayland-vpets por `wayvpet theme import-vpets`.\n\
         # No redistribuir arte de terceros; ver themes/COMUNIDAD.md.\n\
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

/// Opciones de `wayvpet theme import-vpets`.
pub struct ImportArgs<'a> {
    /// Ruta del origen: carpeta de mascota, un `.conf`, o una hoja `.png`.
    pub source: &'a str,
    /// Nombre del tema de salida (default: derivado del origen).
    pub name: Option<&'a str>,
    /// Directorio del tema de salida (default: `$XDG_DATA_HOME/wayvpet/themes/<name>`).
    pub out_dir: Option<PathBuf>,
    /// Solo imprimir el informe, no escribir nada.
    pub dry_run: bool,
    /// `frame_w` / `frame_h` forzados (si el `.conf` no los trae y no se pueden
    /// derivar de la hoja).
    pub frame_w: Option<u32>,
    pub frame_h: Option<u32>,
    /// `--state` repetible. Para un **APNG** suelto: un nombre a secas
    /// (`--state writing`, default `writing`). Para una **hoja PNG** suelta sin
    /// `.conf`: `NOMBRE=row:R,frames:F[,fps:X][,col:C]` (1-based) define un
    /// estado de rejilla (spec 0014 §5.5).
    pub states: &'a [String],
}

/// Lo que hay que escribir en el directorio del tema: el `theme.ini` ya
/// compuesto, los ficheros de imagen a copiar (nombre → bytes) y el informe.
struct Built {
    ini: String,
    files: Vec<(String, Vec<u8>)>,
    report: ImportReport,
    info: String,
}

/// Ejecuta el import. Imprime el informe por stdout. Devuelve la ruta del tema
/// creado, o `Err` con un motivo legible. **Solo falla** si no hay ningún estado
/// utilizable o la E/S de escritura falla (spec 0014 §5.6).
pub fn run(args: &ImportArgs) -> Result<PathBuf, String> {
    let src = Path::new(args.source);
    let meta = std::fs::metadata(src).map_err(|e| format!("{}: {e}", args.source))?;

    // Clasifica el origen y compón lo que se escribiría.
    let built = if meta.is_dir() {
        if find_conf(src).is_some() {
            build_from_mascot_dir(args, src)?
        } else if let Some(groups) = loose_png_groups(src) {
            build_from_loose_pngs(args, src, &groups)?
        } else {
            build_from_mascot_dir(args, src)? // carpeta con un solo PNG y sin .conf
        }
    } else if src
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("conf"))
    {
        build_from_conf_file(args, src)?
    } else if src
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"))
    {
        return Err("GIF todavía no soportado (falta decidir la crate `gif`); \
             convierte a APNG o a una hoja PNG"
            .to_string());
    } else {
        // `.png` / `.apng` suelto: hoja de rejilla o animación de un estado.
        build_from_single_file(args, src)?
    };

    let name = derive_out_name(args, src);
    if name.is_empty() || name.contains(['/', '\\', '\0']) || name.contains("..") {
        return Err(format!("nombre de tema inválido: '{name}'"));
    }
    let out = args
        .out_dir
        .clone()
        .unwrap_or_else(|| crate::theme::user_themes_dir().join(&name));
    // `translate` / `build_anim_state` no emiten `name`: se antepone aquí con el
    // nombre de salida ya validado.
    let ini = format!("name = {name}\n{}", built.ini);

    println!("origen:  {}", args.source);
    println!("{}", built.info);
    println!("destino: {}", out.display());
    print!("{}", built.report.render());

    if args.dry_run {
        println!("--dry-run: no se ha escrito nada.");
        return Ok(out);
    }
    if built.report.mapped.is_empty() {
        return Err("ningún estado utilizable en el origen; no creo el tema".to_string());
    }
    if out.exists() {
        return Err(format!("{} ya existe", out.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (fname, data) in &built.files {
        std::fs::write(out.join(fname), data)
            .map_err(|e| format!("{}: {e}", out.join(fname).display()))?;
    }
    std::fs::write(out.join("theme.ini"), &ini)
        .map_err(|e| format!("{}: {e}", out.join("theme.ini").display()))?;

    println!("tema escrito. validando…");
    if !crate::theme::check(&out.to_string_lossy()) {
        return Err("el tema recién escrito no pasa `theme check`".to_string());
    }
    Ok(out)
}

/// Nombre del tema de salida: `--name`, o el nombre del fichero/carpeta de
/// origen (sin extensión).
fn derive_out_name(args: &ImportArgs, src: &Path) -> String {
    args.name.map(str::to_string).unwrap_or_else(|| {
        src.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("vpets-import")
            .to_string()
    })
}

/// Lee un fichero de imagen con los topes de tamaño y lo decodifica. Devuelve
/// los bytes crudos (para copiarlos tal cual) y los fotogramas decodificados.
fn read_image(path: &Path) -> Result<(Vec<u8>, Vec<png_decode::DecodedPng>), String> {
    let smeta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !smeta.is_file() {
        return Err(format!("{}: no es un fichero regular", path.display()));
    }
    if smeta.len() > MAX_SHEET_BYTES {
        return Err(format!(
            "{}: {} bytes, máximo {MAX_SHEET_BYTES}",
            path.display(),
            smeta.len()
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // `decode_frames` valida las dimensiones **antes** de reservar (dims bomba).
    let frames =
        png_decode::decode_frames(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((bytes, frames))
}

/// Copia el `theme.ini` traducido de una mascota con `.conf` + hoja PNG.
fn build_from_mascot_dir(args: &ImportArgs, dir: &Path) -> Result<Built, String> {
    let conf_text = find_conf(dir)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let conf = parse_vpets_conf(&conf_text);
    let sheet = conf
        .sheet_path
        .as_ref()
        .map(|s| resolve_rel(dir, s))
        .filter(|p| p.is_file())
        .or_else(|| single_png(dir))
        .ok_or_else(|| {
            "no encuentro la hoja: la carpeta necesita un .conf con \
             custom_sprite_sheet_filename o un único PNG"
                .to_string()
        })?;
    grid_built(args, &conf, &sheet)
}

/// `theme.ini` de un `.conf` suelto (la hoja se resuelve relativa a su carpeta).
fn build_from_conf_file(args: &ImportArgs, conf_path: &Path) -> Result<Built, String> {
    let conf_text =
        std::fs::read_to_string(conf_path).map_err(|e| format!("{}: {e}", conf_path.display()))?;
    let conf = parse_vpets_conf(&conf_text);
    let base = conf_path.parent().unwrap_or(Path::new("."));
    let sheet = conf
        .sheet_path
        .as_ref()
        .map(|s| resolve_rel(base, s))
        .or_else(|| single_png(base))
        .ok_or_else(|| "el .conf no declara custom_sprite_sheet_filename".to_string())?;
    grid_built(args, &conf, &sheet)
}

/// `.png` / `.apng` suelto: 1 fotograma → hoja de rejilla (necesita
/// `--frame-w/--frame-h`); varios → animación de un estado (`--state`).
fn build_from_single_file(args: &ImportArgs, file: &Path) -> Result<Built, String> {
    let (bytes, frames) = read_image(file)?;
    if frames.len() > 1 {
        return build_anim_state(args, file, bytes, &frames);
    }
    // Estados definidos por `--state NOMBRE=row:R,frames:F[,...]` (spec §5.5).
    let mut conf = VpetsConf::default();
    for spec in args.states {
        conf.states.push(parse_state_spec(spec)?);
    }
    let sheet_dims = (frames[0].w, frames[0].h);
    let (fw, fh) = args
        .frame_w
        .zip(args.frame_h)
        .or_else(|| derive_frame_size(sheet_dims.0, sheet_dims.1, &conf.states))
        .ok_or_else(|| {
            format!(
                "una hoja suelta de {}×{} necesita --frame-w y --frame-h, o \
                 --state NOMBRE=row:R,frames:F para deducirlos",
                sheet_dims.0, sheet_dims.1
            )
        })?;
    let base = file
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("sheet.png")
        .to_string();
    let (ini, report) = translate(&conf, license_for(&conf), &base, fw, fh);
    Ok(Built {
        ini,
        files: vec![(base, bytes)],
        report,
        info: format!(
            "hoja:    {} ({}×{}), frame {fw}×{fh}",
            file.display(),
            sheet_dims.0,
            sheet_dims.1
        ),
    })
}

/// Núcleo común de los caminos "hoja de rejilla": decodifica, resuelve
/// `frame_w/h` (flags → `.conf` → derivado) y traduce.
fn grid_built(args: &ImportArgs, conf: &VpetsConf, sheet: &Path) -> Result<Built, String> {
    let (bytes, frames) = read_image(sheet)?;
    let (sw, sh) = (frames[0].w, frames[0].h);
    if frames.len() > 1 {
        eprintln!(
            "wayvpet: aviso: {} es un APNG; se usará solo el primer fotograma como hoja",
            sheet.display()
        );
    }
    let (fw, fh) = args
        .frame_w
        .zip(args.frame_h)
        .or_else(|| conf.frame_w.zip(conf.frame_h))
        .or_else(|| derive_frame_size(sw, sh, &conf.states))
        .ok_or_else(|| {
            format!("no puedo determinar el tamaño de frame de una hoja {sw}×{sh}; pasa --frame-w y --frame-h")
        })?;
    let base = sheet
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.contains(['/', '\\', '\0']))
        .unwrap_or("sheet.png")
        .to_string();
    let (ini, report) = translate(conf, license_for(conf), &base, fw, fh);
    Ok(Built {
        ini,
        files: vec![(base, bytes)],
        report,
        info: format!("hoja:    {} ({sw}×{sh}), frame {fw}×{fh}", sheet.display()),
    })
}

/// Ficheros `<estado>_<n>.png` / `<estado>.png` de un directorio, agrupados por
/// estado y ordenados por índice. `None` si ninguna clave nombra un estado
/// conducible (así una carpeta con `gato.png` a secas cae al camino "hoja
/// única", no a "PNGs sueltos").
fn loose_png_groups(dir: &Path) -> Option<BTreeMap<String, Vec<PathBuf>>> {
    let mut groups: BTreeMap<String, Vec<(u32, PathBuf)>> = BTreeMap::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if !p.is_file() || !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // `<estado>_<idx>` o `<estado>` a secas.
        let (state, idx) = match stem.rsplit_once('_') {
            Some((s, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => {
                (s.to_string(), n.parse().unwrap_or(0))
            }
            _ => (stem.to_string(), 0u32),
        };
        groups.entry(state).or_default().push((idx, p));
    }
    if !groups.keys().any(|k| StateId::from_theme_name(k).is_some()) {
        return None;
    }
    Some(
        groups
            .into_iter()
            .map(|(k, mut v)| {
                v.sort_by_key(|(i, _)| *i);
                (k, v.into_iter().map(|(_, p)| p).collect())
            })
            .collect(),
    )
}

/// Ensambla una carpeta de PNGs sueltos por estado en una hoja de rejilla única
/// (una fila por estado) y traduce.
fn build_from_loose_pngs(
    args: &ImportArgs,
    _dir: &Path,
    groups: &BTreeMap<String, Vec<PathBuf>>,
) -> Result<Built, String> {
    // Orden canónico de filas: los estados conocidos primero.
    let mut states: Vec<&String> = groups.keys().collect();
    states.sort_by_key(|s| canon_index(s));

    // Decodifica todo y fija `frame_w/h` con el primer PNG; exige uniformidad.
    let mut decoded: Vec<(&String, Vec<png_decode::DecodedPng>)> = Vec::new();
    let (mut fw, mut fh) = (0u32, 0u32);
    for st in &states {
        let mut fr = Vec::new();
        for path in &groups[*st] {
            let (_, mut d) = read_image(path)?;
            let img = d.remove(0);
            if fw == 0 {
                (fw, fh) = (img.w, img.h);
            } else if (img.w, img.h) != (fw, fh) {
                return Err(format!(
                    "{}: {}×{} no coincide con el primer PNG ({fw}×{fh}); \
                     los PNGs sueltos deben medir todos igual",
                    path.display(),
                    img.w,
                    img.h
                ));
            }
            fr.push(img);
        }
        decoded.push((st, fr));
    }
    if let (Some(w), Some(h)) = (args.frame_w, args.frame_h) {
        if (w, h) != (fw, fh) {
            eprintln!("wayvpet: aviso: --frame-w/-h {w}×{h} ignorados; los PNGs miden {fw}×{fh}");
        }
    }

    let max_cols = decoded.iter().map(|(_, f)| f.len()).max().unwrap_or(0) as u32;
    let rows = decoded.len() as u32;
    if max_cols == 0 || rows == 0 {
        return Err("no hay PNGs utilizables".to_string());
    }
    let (sheet_w, sheet_h) = (max_cols * fw, rows * fh);
    let mut canvas = vec![0u8; (sheet_w * sheet_h * 4) as usize];
    let mut conf = VpetsConf::default();
    for (r, (name, fr)) in decoded.iter().enumerate() {
        for (c, img) in fr.iter().enumerate() {
            blit(&mut canvas, sheet_w, (c as u32 * fw, r as u32 * fh), img);
        }
        conf.states.push(VpetsState {
            name: (*name).clone(),
            row: r as u32 + 1,
            frames: fr.len() as u32,
            col_start: 1,
            fps: None,
        });
    }
    let sheet_png = encode_rgba_png(sheet_w, sheet_h, &canvas)?;
    let (ini, report) = translate(&conf, license_for(&conf), "sheet.png", fw, fh);
    Ok(Built {
        ini,
        files: vec![("sheet.png".to_string(), sheet_png)],
        report,
        info: format!(
            "hoja ensamblada de {} PNGs sueltos → {sheet_w}×{sheet_h}, frame {fw}×{fh}",
            decoded.iter().map(|(_, f)| f.len()).sum::<usize>()
        ),
    })
}

/// Un fichero animado (APNG) → tema de un solo estado (`sheet_<estado> =`).
fn build_anim_state(
    args: &ImportArgs,
    file: &Path,
    bytes: Vec<u8>,
    frames: &[png_decode::DecodedPng],
) -> Result<Built, String> {
    // El 1er `--state` a secas (sin `=`) nombra el estado; default `writing`.
    let state = args
        .states
        .iter()
        .find(|s| !s.contains('='))
        .map_or("writing", String::as_str);
    if StateId::from_theme_name(state).is_none() {
        return Err(format!(
            "--state '{state}' no es un estado conducible (usa idle/writing/sleep/…)"
        ));
    }
    let (fw, fh) = args
        .frame_w
        .zip(args.frame_h)
        .unwrap_or((frames[0].w, frames[0].h));
    let fname = format!("{state}.apng");
    let default_fps = 12;

    let mut ini = String::new();
    let _ = write!(
        ini,
        "# Importado de wayland-vpets (animación de un estado) por \
         `wayvpet theme import-vpets`.\n\
         # No redistribuir arte de terceros; ver themes/COMUNIDAD.md.\n\
         license = {lic}\n\
         theme_format = 3\n\
         theme_version = 1\n\
         \n\
         sheet_{state} = {fname}\n\
         frame_w = {fw}\n\
         frame_h = {fh}\n\
         default_fps = {default_fps}\n\
         scale_filter = nearest\n\
         input_model = activity\n\
         row_base = 1\n",
        lic = license_for(&VpetsConf::default()),
    );

    let mut report = ImportReport {
        input_model: InputModel::Activity,
        ..Default::default()
    };
    report.mapped.push((state.to_string(), frames.len() as u32));
    for (canon, fb) in CANONICAL_FALLBACK {
        if *canon != state {
            report
                .missing
                .push(((*canon).to_string(), (*fb).to_string()));
        }
    }

    Ok(Built {
        ini,
        files: vec![(fname, bytes)],
        report,
        info: format!(
            "animación: {} ({} fotogramas {}×{}) → estado '{state}'",
            file.display(),
            frames.len(),
            frames[0].w,
            frames[0].h
        ),
    })
}

/// Índice del estado en el orden canónico de emisión (los desconocidos, al final).
fn canon_index(name: &str) -> usize {
    const ORDER: &[&str] = &[
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
    ORDER.iter().position(|c| *c == name).unwrap_or(ORDER.len())
}

/// Copia `img` (RGBA8, `img.w`×`img.h`) en `canvas` (`cw` de ancho) en `(x, y)`.
fn blit(canvas: &mut [u8], cw: u32, (x, y): (u32, u32), img: &png_decode::DecodedPng) {
    for row in 0..img.h {
        let si = ((row * img.w) * 4) as usize;
        let di = (((y + row) * cw + x) * 4) as usize;
        let n = (img.w * 4) as usize;
        canvas[di..di + n].copy_from_slice(&img.rgba[si..si + n]);
    }
}

/// Codifica un búfer RGBA8 recto a PNG con la crate `png`.
fn encode_rgba_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc
        .write_header()
        .map_err(|e| format!("no puedo codificar la hoja: {e}"))?;
    wr.write_image_data(rgba)
        .map_err(|e| format!("no puedo codificar la hoja: {e}"))?;
    wr.finish()
        .map_err(|e| format!("no puedo cerrar el PNG: {e}"))?;
    Ok(out)
}

/// Primer `wayvpet.conf` / `*.conf` dentro de `dir`.
fn find_conf(dir: &Path) -> Option<PathBuf> {
    let pref = dir.join("wayvpet.conf");
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
        let (ini, rep) = translate(&c, "IP de Nintendo", "charizard.png", 64, 48);
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
        let (ini, rep) = translate(&c, "MIT", "s.png", 8, 8);
        assert_eq!(rep.input_model, InputModel::Hands);
        assert!(ini.contains("input_model = hands"));
        assert!(ini.contains("state_left_down_row = 2") || ini.contains("state_left-down_row"));
    }

    #[test]
    fn conf_vacio_no_panica() {
        let c = parse_vpets_conf("");
        let (_, rep) = translate(&c, "y", "s.png", 8, 8);
        assert!(rep.mapped.is_empty());
        assert!(rep.render().contains("ningún estado utilizable"));
    }

    // --- E/S: extremo a extremo con una hoja PNG de juguete ------------------

    /// Directorio temporal único para un test (sin `tempfile`: nombre por
    /// nombre de test). Se borra al empezar por si quedó de una tanda previa.
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wayvpet-import-{tag}"));
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
            pet.join("wayvpet.conf"),
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
            states: &[],
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
            states: &[],
        };
        run(&args).expect("dry-run OK");
        assert!(!out.exists(), "--dry-run no crea el directorio");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// APNG RGBA de `n` marcos completos, todos del mismo color, con `png`.
    fn toy_apng(w: u32, h: u32, n: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_animated(n, 0).unwrap();
        let mut wr = enc.write_header().unwrap();
        for _ in 0..n {
            wr.write_image_data(&[10u8, 200, 30, 255].repeat((w * h) as usize))
                .unwrap();
        }
        wr.finish().unwrap();
        out
    }

    #[test]
    fn import_carpeta_de_pngs_sueltos() {
        let dir = scratch("loose");
        let pet = dir.join("pet");
        std::fs::create_dir_all(&pet).unwrap();
        for f in ["idle_0.png", "idle_1.png", "writing_0.png"] {
            std::fs::write(pet.join(f), toy_png(40, 40)).unwrap();
        }
        let out = dir.join("out");
        let args = ImportArgs {
            source: pet.to_str().unwrap(),
            name: Some("loosepet"),
            out_dir: Some(out.clone()),
            dry_run: false,
            frame_w: None,
            frame_h: None,
            states: &[],
        };
        run(&args).expect("import de PNGs sueltos OK");
        // Se ensambló una hoja 80×80 (2 columnas × 2 filas).
        let (bytes, frames) = read_image(&out.join("sheet.png")).unwrap();
        assert_eq!((frames[0].w, frames[0].h), (80, 80));
        let _ = bytes;
        let ini = std::fs::read_to_string(out.join("theme.ini")).unwrap();
        assert!(ini.contains("frame_w = 40"));
        assert!(ini.contains("state_idle_frames = 2"));
        assert!(ini.contains("state_writing_frames = 1"));
        assert!(crate::theme::check(&out.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_apng_como_un_estado() {
        let dir = scratch("apng");
        let file = dir.join("fly.apng");
        std::fs::write(&file, toy_apng(48, 48, 4)).unwrap();
        let out = dir.join("out");
        let st = [String::from("writing")];
        let args = ImportArgs {
            source: file.to_str().unwrap(),
            name: Some("flyer"),
            out_dir: Some(out.clone()),
            dry_run: false,
            frame_w: None,
            frame_h: None,
            states: &st,
        };
        run(&args).expect("import de APNG OK");
        assert!(out.join("writing.apng").is_file());
        let ini = std::fs::read_to_string(out.join("theme.ini")).unwrap();
        assert!(ini.contains("sheet_writing = writing.apng"));
        assert!(ini.contains("frame_w = 48"));
        assert!(crate::theme::check(&out.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corpus_de_conformidad() {
        // T-0014-conformidad (versión de juguete, spec §5.6): un abanico de
        // mascotas estilo wayland-vpets con todas las variantes que el
        // importador debe **tolerar sin fallar**; cada una se importa y pasa
        // `theme check`. Cero arte de terceros: PNGs de color plano.
        let root = scratch("corpus");
        let grid = |cols: u32, rows: u32| toy_png(cols * 16, rows * 16);

        /// Una mascota de juguete del corpus.
        struct Pet {
            name: &'static str,
            files: Vec<(&'static str, Vec<u8>)>,
            state: Option<&'static str>,
            /// El origen que se le pasa al importador es el 1er fichero, no la carpeta.
            source_is_file: bool,
        }
        let p = |name, files, state, source_is_file| Pet {
            name,
            files,
            state,
            source_is_file,
        };

        let pets: Vec<Pet> = vec![
            p(
                "hoja-unica",
                vec![
                    ("wayvpet.conf", b"animation_name=custom\ncustom_sprite_sheet_filename=s.png\ncustom_frame_width=16\ncustom_frame_height=16\nfps=12\ncustom_idle_row=1\ncustom_idle_frames=2\ncustom_writing_row=2\ncustom_writing_frames=3\ncustom_sleep_row=3\ncustom_sleep_frames=2\n".to_vec()),
                    ("s.png", grid(3, 3)),
                ],
                None,
                false,
            ),
            p(
                "row-base-0",
                vec![
                    ("wayvpet.conf", b"custom_sprite_sheet_filename=s.png\ncustom_frame_width=16\ncustom_frame_height=16\nrow_base=0\ncustom_idle_row=0\ncustom_idle_frames=1\ncustom_writing_row=1\ncustom_writing_frames=2\n".to_vec()),
                    ("s.png", grid(2, 2)),
                ],
                None,
                false,
            ),
            p(
                "animation-speed",
                vec![
                    ("pet.conf", b"custom_sprite_sheet_filename=s.png\ncustom_frame_width=16\ncustom_frame_height=16\nanimation_speed=8\ncustom_idle_row=1\ncustom_idle_frames=2\n".to_vec()),
                    ("s.png", grid(2, 1)),
                ],
                None,
                false,
            ),
            p(
                "solo-idle-faltan-reserva",
                vec![
                    ("wayvpet.conf", b"custom_sprite_sheet_filename=s.png\ncustom_frame_width=16\ncustom_frame_height=16\ncustom_idle_row=1\ncustom_idle_frames=1\n".to_vec()),
                    ("s.png", grid(1, 1)),
                ],
                None,
                false,
            ),
            p(
                "con-manos",
                vec![
                    ("wayvpet.conf", b"custom_sprite_sheet_filename=s.png\ncustom_frame_width=16\ncustom_frame_height=16\ncustom_idle_row=1\ncustom_idle_frames=1\ncustom_left_down_row=2\ncustom_left_down_frames=1\ncustom_right_down_row=3\ncustom_right_down_frames=1\n".to_vec()),
                    ("s.png", grid(1, 3)),
                ],
                None,
                false,
            ),
            p(
                "working-moving-ignorados",
                vec![
                    ("wayvpet.conf", b"custom_sprite_sheet_filename=s.png\ncustom_frame_width=16\ncustom_frame_height=16\ncustom_idle_row=1\ncustom_idle_frames=1\ncustom_writing_row=2\ncustom_writing_frames=1\ncustom_working_row=3\ncustom_working_frames=2\ncustom_moving_row=4\ncustom_moving_frames=2\n".to_vec()),
                    ("s.png", grid(2, 4)),
                ],
                None,
                false,
            ),
            p(
                "pngs-sueltos",
                vec![
                    ("idle_0.png", toy_png(16, 16)),
                    ("idle_1.png", toy_png(16, 16)),
                    ("writing_0.png", toy_png(16, 16)),
                ],
                None,
                false,
            ),
            p("apng-un-estado", vec![("anim.apng", toy_apng(16, 16, 3))], Some("writing"), true),
            p(
                "frame-derivado-del-tamano",
                vec![
                    ("wayvpet.conf", b"custom_sprite_sheet_filename=s.png\ncustom_idle_row=1\ncustom_idle_frames=2\ncustom_writing_row=2\ncustom_writing_frames=2\n".to_vec()),
                    ("s.png", grid(2, 2)),
                ],
                None,
                false,
            ),
        ];

        for Pet {
            name,
            files,
            state,
            source_is_file,
        } in pets
        {
            let pet = root.join(name);
            std::fs::create_dir_all(&pet).unwrap();
            for (f, bytes) in &files {
                std::fs::write(pet.join(f), bytes).unwrap();
            }
            let source = if source_is_file {
                pet.join(files[0].0)
            } else {
                pet.clone()
            };
            let out = root.join(format!("{name}-out"));
            let st: Vec<String> = state.iter().map(|s| (*s).to_string()).collect();
            let args = ImportArgs {
                source: source.to_str().unwrap(),
                name: Some(name),
                out_dir: Some(out.clone()),
                dry_run: false,
                frame_w: None,
                frame_h: None,
                states: &st,
            };
            run(&args).unwrap_or_else(|e| panic!("mascota '{name}': el import falló: {e}"));
            assert!(
                crate::theme::check(&out.to_string_lossy()),
                "mascota '{name}': no pasa `theme check`"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hoja_suelta_con_flags_de_estado() {
        // `--state idle=row:1,frames:2` sobre una hoja PNG sin `.conf` (spec §5.5).
        assert_eq!(
            parse_state_spec("writing=row:2,frames:3,fps:10").unwrap(),
            VpetsState {
                name: "writing".into(),
                row: 2,
                frames: 3,
                col_start: 1,
                fps: Some(10),
            }
        );
        assert!(parse_state_spec("mal").is_err(), "sin '='");
        assert!(parse_state_spec("x=frames:2").is_err(), "falta row");

        let dir = scratch("state-flags");
        let png = dir.join("s.png");
        std::fs::write(&png, toy_png(48, 32)).unwrap(); // 3 cols × 2 filas de 16
        let out = dir.join("out");
        let st = [
            String::from("idle=row:1,frames:2"),
            String::from("writing=row:2,frames:3"),
        ];
        let args = ImportArgs {
            source: png.to_str().unwrap(),
            name: Some("porflags"),
            out_dir: Some(out.clone()),
            dry_run: false,
            frame_w: Some(16),
            frame_h: Some(16),
            states: &st,
        };
        run(&args).expect("hoja + --state OK");
        let ini = std::fs::read_to_string(out.join("theme.ini")).unwrap();
        assert!(ini.contains("state_idle_frames = 2"));
        assert!(ini.contains("state_writing_row = 2"));
        assert!(crate::theme::check(&out.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gif_da_error_claro() {
        let dir = scratch("gif");
        let file = dir.join("x.gif");
        std::fs::write(&file, b"GIF89a...").unwrap();
        let args = ImportArgs {
            source: file.to_str().unwrap(),
            name: Some("g"),
            out_dir: Some(dir.join("out")),
            dry_run: true,
            frame_w: Some(8),
            frame_h: Some(8),
            states: &[],
        };
        let err = run(&args).unwrap_err();
        assert!(err.contains("GIF"), "el error menciona GIF: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
