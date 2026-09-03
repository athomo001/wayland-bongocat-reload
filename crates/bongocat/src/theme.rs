//! Resolución y carga de temas desde disco (spec 0006). La parte pura
//! (parseo del `theme.ini`, aspecto) vive en `bongocat_common::theme`; aquí
//! están las rutas de búsqueda y la lectura/validación de los ficheros.
//!
//! Cualquier fallo → `None` y el llamante usa el gato **embebido** (`classic`):
//! bongocat nunca se queda sin gato.

use std::io;
use std::path::{Path, PathBuf};

use bongocat_common::sheet::{parse_sheet_ini, SheetTheme};
use bongocat_common::theme::{
    format_supported, parse_theme_ini, ThemeMeta, DEFAULT_FRAME_FILES, THEME_FORMAT_SUPPORTED,
};

use crate::anim::SheetImages;
use crate::png_decode;

const MAX_FRAME_BYTES: u64 = 2 * 1024 * 1024;
/// Tope al tamaño en disco de una hoja de sprites (`theme_format = 3`).
const MAX_SHEET_BYTES: u64 = 16 * 1024 * 1024;

/// El arte de un tema, según su `theme_format`.
pub enum ThemeArt {
    /// `theme_format` 1/2: cinco SVG con nombres fijos (el `classic`).
    Svg(Box<[Vec<u8>; 5]>),
    /// `theme_format = 3`: sprite sheet PNG en rejilla, estilo wayland-vpets
    /// (spec 0014).
    Sheet(SheetArt),
}

/// Arte de un tema de sprite sheet: la rejilla parseada + las hojas
/// decodificadas (la global y/o las de `sheet_<estado> =`), por nombre de
/// fichero.
pub struct SheetArt {
    pub sheet: SheetTheme,
    pub images: SheetImages,
}

/// Un tema ya cargado en memoria: metadatos + el arte (SVG o sprite sheet).
pub struct LoadedTheme {
    pub meta: ThemeMeta,
    /// Directorio resuelto del tema.
    pub dir: PathBuf,
    pub art: ThemeArt,
}

impl LoadedTheme {
    /// Resumen de una línea para `theme check` / `--dry-run`.
    #[must_use]
    pub fn describe(&self) -> String {
        let label = if self.meta.name.is_empty() {
            self.dir.file_name().and_then(|s| s.to_str()).unwrap_or("?")
        } else {
            &self.meta.name
        };
        match &self.art {
            ThemeArt::Svg(_) => format!(
                "'{label}' — SVG, aspecto {}:{}, theme_format {}",
                self.meta.aspect.0, self.meta.aspect.1, self.meta.theme_format
            ),
            ThemeArt::Sheet(s) => {
                let estados: Vec<String> = s
                    .sheet
                    .states
                    .iter()
                    .map(|st| format!("{}×{}", st.name, st.frames))
                    .collect();
                let hojas = s.images.len();
                format!(
                    "'{label}' — sprite sheet, {hoja_txt}, frame {}×{}, {:?} model, estados: {}",
                    s.sheet.frame_w,
                    s.sheet.frame_h,
                    s.sheet.input_model,
                    estados.join(", "),
                    hoja_txt = if hojas == 1 {
                        let (n, p) = s.images.iter().next().unwrap();
                        format!("hoja {n} {}×{}", p.w, p.h)
                    } else {
                        format!("{hojas} hojas")
                    },
                )
            }
        }
    }
}

/// Directorios donde se buscan temas por nombre, en orden de prioridad.
#[must_use]
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home_share = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(d) = home_share {
        dirs.push(d.join("bongocat/themes"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for base in data_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(base).join("bongocat/themes"));
    }
    // Árbol del repo (desarrollo): `./themes/`.
    dirs.push(PathBuf::from("themes"));
    dirs
}

/// Nombres de tema disponibles (únicos, ordenados) en todas las rutas.
#[must_use]
pub fn list() -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for d in search_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            if e.path().is_dir() {
                if let Some(n) = e.file_name().to_str() {
                    names.insert(n.to_string());
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Resuelve `spec` (nombre o ruta) a un tema cargado. `None` = usar el embebido.
#[must_use]
pub fn resolve(spec: &str) -> Option<LoadedTheme> {
    if spec.is_empty() {
        return None;
    }
    let dir = if spec.contains('/') {
        let p = PathBuf::from(spec);
        if !p.is_dir() {
            eprintln!("bongocat: tema: '{spec}' no es un directorio; uso el clásico");
            return None;
        }
        p
    } else {
        if spec.contains("..") || spec.contains('\0') {
            eprintln!("bongocat: tema inválido: '{spec}'");
            return None;
        }
        match search_dirs()
            .into_iter()
            .map(|d| d.join(spec))
            .find(|d| d.is_dir())
        {
            Some(d) => d,
            None => {
                eprintln!("bongocat: tema '{spec}' no encontrado; uso el clásico");
                return None;
            }
        }
    };
    load_dir(&dir)
}

fn load_dir(dir: &Path) -> Option<LoadedTheme> {
    let ini = std::fs::read_to_string(dir.join("theme.ini")).unwrap_or_default();
    let meta = parse_theme_ini(&ini);
    if !format_supported(&meta) {
        eprintln!(
            "bongocat: tema {}: theme_format {} > {} soportado; uso el clásico",
            dir.display(),
            meta.theme_format,
            THEME_FORMAT_SUPPORTED
        );
        return None;
    }

    // `theme_format = 3`: sprite sheet PNG en rejilla (spec 0014).
    if meta.theme_format == 3 {
        return load_sheet(dir, &ini, meta);
    }

    let mut frames: [Vec<u8>; 5] = Default::default();
    for (i, name) in meta.frame_files.iter().enumerate() {
        let path = dir.join(name);
        let md = match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() && m.len() <= MAX_FRAME_BYTES => m,
            Ok(_) => {
                eprintln!(
                    "bongocat: tema: {} no es un fichero regular ≤ 2 MiB",
                    path.display()
                );
                return None;
            }
            Err(e) => {
                eprintln!("bongocat: tema: falta {} ({e})", path.display());
                return None;
            }
        };
        let _ = md;
        let bytes = std::fs::read(&path).ok()?;
        if resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).is_err() {
            eprintln!(
                "bongocat: tema: {} no parsea como SVG; uso el clásico",
                path.display()
            );
            return None;
        }
        frames[i] = bytes;
    }

    let label = if meta.name.is_empty() {
        dir.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    } else {
        meta.name.clone()
    };
    eprintln!("bongocat: tema '{label}' cargado de {}", dir.display());
    Some(LoadedTheme {
        meta,
        dir: dir.to_path_buf(),
        art: ThemeArt::Svg(Box::new(frames)),
    })
}

/// Carga un tema `theme_format = 3` (sprite sheet). Cualquier fallo → `None` y
/// el llamante se queda con el gato embebido — como con los temas SVG.
///
/// Acepta una **hoja global** (`sheet =`) y/o **hojas por estado**
/// (`sheet_<estado> =`), spec 0014 §5.1. Cada fichero se decodifica una sola vez.
fn load_sheet(dir: &Path, ini: &str, meta: ThemeMeta) -> Option<LoadedTheme> {
    let sheet = parse_sheet_ini(ini);
    if sheet.frame_w == 0 || sheet.frame_h == 0 {
        eprintln!(
            "bongocat: tema {}: falta frame_w/frame_h; uso el clásico",
            dir.display()
        );
        return None;
    }
    if sheet.states.is_empty() {
        eprintln!(
            "bongocat: tema {}: ningún state_<n>_row/_frames utilizable; uso el clásico",
            dir.display()
        );
        return None;
    }
    let files = sheet.sheet_files();
    if files.is_empty() {
        eprintln!(
            "bongocat: tema {}: ni 'sheet =' ni 'sheet_<estado> ='; uso el clásico",
            dir.display()
        );
        return None;
    }

    // Cota superior de la rejilla, para el aviso "hoja más pequeña de la cuenta".
    let need_w = sheet
        .states
        .iter()
        .map(|s| (s.col_start + s.frames) * sheet.frame_w)
        .max()
        .unwrap_or(0);
    let need_h = sheet
        .states
        .iter()
        .map(|s| (s.row + 1) * sheet.frame_h)
        .max()
        .unwrap_or(0);

    let mut images = SheetImages::new();
    for name in files {
        // Cada hoja: nombre de fichero simple, junto al theme.ini.
        if name.contains("..") || name.contains(['/', '\\', '\0']) {
            eprintln!(
                "bongocat: tema {}: 'sheet = {name}' debe ser un nombre de fichero simple",
                dir.display()
            );
            return None;
        }
        let path = dir.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() && m.len() <= MAX_SHEET_BYTES => {}
            Ok(_) => {
                eprintln!(
                    "bongocat: tema: {} no es un fichero regular ≤ 16 MiB",
                    path.display()
                );
                return None;
            }
            Err(e) => {
                eprintln!("bongocat: tema: falta {} ({e})", path.display());
                return None;
            }
        }
        let bytes = std::fs::read(&path).ok()?;
        let png = match png_decode::decode_rgba8(&bytes) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("bongocat: tema: {}: {e}; uso el clásico", path.display());
                return None;
            }
        };
        if png.w < need_w || png.h < need_h {
            eprintln!(
                "bongocat: tema {}: la hoja {name} ({}×{}) es menor que la rejilla \
                 ({need_w}×{need_h}); los frames que falten saldrán transparentes",
                dir.display(),
                png.w,
                png.h
            );
        }
        images.insert(name.to_string(), png);
    }

    let loaded = LoadedTheme {
        meta,
        dir: dir.to_path_buf(),
        art: ThemeArt::Sheet(SheetArt { sheet, images }),
    };
    eprintln!(
        "bongocat: tema {} cargado de {}",
        loaded.describe(),
        dir.display()
    );
    Some(loaded)
}

/// Primer directorio de temas **escribible** (`$XDG_DATA_HOME/bongocat/themes`).
fn user_themes_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("bongocat/themes")
}

/// `bongocat theme new NOMBRE`: crea la carpeta del tema con los 5 SVG del
/// `classic` como plantilla y un `theme.ini` rellenado. Devuelve la ruta.
///
/// # Errores
/// Si `NOMBRE` no es válido, si el tema ya existe, o por errores de E/S.
pub fn scaffold(name: &str) -> io::Result<PathBuf> {
    if name.is_empty()
        || name.contains(['/', '\\', '\0'])
        || name.contains("..")
        || name.starts_with('.')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nombre de tema inválido",
        ));
    }
    let dir = user_themes_dir().join(name);
    if dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} ya existe", dir.display()),
        ));
    }
    std::fs::create_dir_all(&dir)?;

    let svgs = crate::anim::classic_frame_svgs();
    for (file, svg) in DEFAULT_FRAME_FILES.iter().zip(svgs.iter()) {
        std::fs::write(dir.join(file), svg)?;
    }
    let ini = format!(
        "name = {name}\n\
         author = \n\
         license = CC-BY-4.0\n\
         theme_format = 1\n\
         theme_version = 1\n\
         aspect_ratio = 500:277\n\
         default_cat_height = 110\n"
    );
    std::fs::write(dir.join("theme.ini"), ini)?;
    Ok(dir)
}

/// `bongocat theme check NOMBRE|RUTA`: intenta cargarlo y reporta. `true` = OK.
#[must_use]
pub fn check(spec: &str) -> bool {
    match resolve(spec) {
        Some(t) => {
            println!("OK: {}", t.describe());
            true
        }
        None => {
            // `resolve` ya avisó del motivo por stderr.
            false
        }
    }
}
