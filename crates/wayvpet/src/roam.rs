//! Paseo del vpet **entre monitores** (multi-head, sobre spec 0008 §8.4).
//!
//! Un vpet con `can_roam` que se reparte sobre varias salidas corre como una
//! instancia (proceso) por monitor —el modelo de `spawn_per_monitor`—. La
//! posición del gato la coordinan esas instancias por un fichero plano en
//! `$XDG_RUNTIME_DIR/wayvpet/roam`: **solo la instancia `owner`** dibuja y anima
//! el gato; las demás lo observan (inotify) y **toman el relevo** cuando el gato
//! cruza la costura hacia su pantalla.
//!
//! Formato `clave=valor`, una por línea (igual que `update-notice` de spec
//! 0015): sin `serde`, sin dependencias nuevas en el núcleo. La geometría de las
//! salidas ([`RoamLayout`]) sale de `OutputInfo.logical_position/size` (SCTK).

use std::path::{Path, PathBuf};

/// Contenido del fichero de estado del paseo multi-monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct RoamState {
    /// X **lógica global** (espacio del compositor) del borde izquierdo del gato.
    pub world_x: f32,
    /// Sentido del paseo: `-1` izquierda, `0` quieto, `1` derecha.
    pub dir: i8,
    /// Nombre de la salida que renderiza el gato ahora mismo.
    pub owner: String,
    /// El gato lo está arrastrando el ratón (modo edición cruzando la costura).
    pub dragging: bool,
    /// Contador monotónico. Una instancia ignora un fichero cuyo `seq` no supera
    /// al último que ella misma escribió (no reacciona a su propio eco).
    pub seq: u64,
}

impl RoamState {
    /// Serializa al formato del fichero (con `\n` final).
    #[must_use]
    pub fn to_text(&self) -> String {
        format!(
            "world_x={}\ndir={}\nowner={}\ndragging={}\nseq={}\n",
            self.world_x,
            self.dir,
            self.owner,
            u8::from(self.dragging),
            self.seq,
        )
    }

    /// Parsea el fichero. `None` si falta `owner` (fichero a medias o corrupto):
    /// el llamante lo trata como "sin estado".
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut world_x = 0.0_f32;
        let mut dir = 0_i8;
        let mut owner: Option<String> = None;
        let mut dragging = false;
        let mut seq = 0_u64;
        for line in text.lines() {
            let Some((k, v)) = line.trim().split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "world_x" => world_x = v.parse().unwrap_or(0.0),
                "dir" => dir = v.parse::<i8>().unwrap_or(0).clamp(-1, 1),
                "owner" => owner = Some(v.to_string()).filter(|s| !s.is_empty()),
                "dragging" => dragging = matches!(v, "1" | "true"),
                "seq" => seq = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        Some(Self {
            world_x,
            dir,
            owner: owner?,
            dragging,
            seq,
        })
    }
}

/// Ruta del fichero: `$XDG_RUNTIME_DIR/wayvpet/roam`, o `None` sin `$XDG_RUNTIME_DIR`.
#[must_use]
pub fn path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|s| !s.is_empty())?;
    Some(Path::new(&dir).join("wayvpet/roam"))
}

/// Lee y parsea el fichero. `Ok(None)` si no existe todavía.
///
/// # Errores
/// E/S distinta de "no existe".
pub fn read(path: &Path) -> std::io::Result<Option<RoamState>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(RoamState::parse(&t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Escribe el estado de forma **atómica** (temporal + `rename`, vía
/// [`wayvpet_common::io::save_atomic`]). Crea el directorio padre si falta.
///
/// # Errores
/// E/S al crear el directorio o al escribir.
pub fn write_atomic(path: &Path, state: &RoamState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    wayvpet_common::io::save_atomic(path, &state.to_text())
}

/// Geometría de las salidas para el paseo global. `(nombre, x0, ancho)` en
/// píxeles lógicos del espacio del compositor, ordenadas de izquierda a derecha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoamLayout {
    outputs: Vec<(String, i32, i32)>,
}

impl RoamLayout {
    /// Ordena por `x0`. Salidas con ancho ≤ 0 se descartan (aún sin geometría).
    #[must_use]
    pub fn new(outputs: impl IntoIterator<Item = (String, i32, i32)>) -> Self {
        let mut outputs: Vec<_> = outputs.into_iter().filter(|(_, _, w)| *w > 0).collect();
        outputs.sort_by_key(|(_, x, _)| *x);
        Self { outputs }
    }

    /// Nº de salidas con geometría conocida (multi-head necesita ≥ 2).
    #[must_use]
    pub fn count(&self) -> usize {
        self.outputs.len()
    }

    /// `(x0, ancho)` de la salida `name`.
    #[must_use]
    pub fn rect_of(&self, name: &str) -> Option<(i32, i32)> {
        self.outputs
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, x, w)| (*x, *w))
    }

    /// Salida que contiene la X global `cx` (se usa con el **centro** del gato).
    /// Si `cx` cae en un hueco entre salidas, la de borde más cercano.
    #[must_use]
    pub fn owner_at(&self, cx: f32) -> Option<&str> {
        let cx = cx.round() as i32;
        for (name, x0, w) in &self.outputs {
            if cx >= *x0 && cx < *x0 + *w {
                return Some(name);
            }
        }
        self.outputs
            .iter()
            .min_by_key(|(_, x0, w)| (cx - x0).abs().min((cx - (x0 + w)).abs()))
            .map(|(n, _, _)| n.as_str())
    }

    /// Rango global `[min_x, max_x)` que cubren todas las salidas (para rascar la
    /// pared solo en el extremo de verdad).
    #[must_use]
    pub fn global_bounds(&self) -> Option<(i32, i32)> {
        let first = self.outputs.first()?;
        let last = self.outputs.last()?;
        Some((first.1, last.1 + last.2))
    }

    /// La salida inmediatamente a la izquierda (`dir < 0`) o derecha (`dir > 0`)
    /// de `name`; `None` si `name` es el extremo en esa dirección.
    #[must_use]
    pub fn neighbor(&self, name: &str, dir: i8) -> Option<&str> {
        let i = self.outputs.iter().position(|(n, _, _)| n == name)?;
        let j = match dir {
            d if d < 0 => i.checked_sub(1)?,
            d if d > 0 => i + 1,
            _ => return None,
        };
        self.outputs.get(j).map(|(n, _, _)| n.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_ida_y_vuelta() {
        let s = RoamState {
            world_x: 1920.5,
            dir: -1,
            owner: "HDMI-A-1".into(),
            dragging: true,
            seq: 42,
        };
        assert_eq!(RoamState::parse(&s.to_text()), Some(s));
    }

    #[test]
    fn state_sin_owner_es_none() {
        assert_eq!(RoamState::parse("world_x=10\ndir=1\nseq=3\n"), None);
        assert_eq!(RoamState::parse("owner=\n"), None, "owner vacío no cuenta");
        assert_eq!(RoamState::parse(""), None);
    }

    #[test]
    fn state_tolera_lineas_raras_y_acota_dir() {
        let s = RoamState::parse("\n# nada\nbasura\nowner = eDP-1 \ndir=9\nfuturo=x\n").unwrap();
        assert_eq!(s.owner, "eDP-1");
        assert_eq!(s.dir, 1, "dir se acota a [-1,1]");
        assert_eq!(s.world_x, 0.0);
        assert!(!s.dragging);
    }

    #[test]
    fn read_fichero_ausente_es_ok_none() {
        assert!(read(Path::new("/no/existe/wayvpet/roam"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn write_atomic_y_relee() {
        let dir = std::env::temp_dir().join(format!("wayvpet-roam-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let p = dir.join("sub/roam");
        let s = RoamState {
            world_x: -30.0,
            dir: 1,
            owner: "DP-2".into(),
            dragging: false,
            seq: 7,
        };
        write_atomic(&p, &s).unwrap();
        assert_eq!(read(&p).unwrap(), Some(s));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn layout() -> RoamLayout {
        // eDP-1 a la izquierda (0..1920), HDMI a la derecha (1920..3360), y una
        // salida sin geometría (ancho 0) que debe ignorarse.
        RoamLayout::new([
            ("HDMI-A-1".to_string(), 1920, 1440),
            ("eDP-1".to_string(), 0, 1920),
            ("VGA-1".to_string(), 5000, 0),
        ])
    }

    #[test]
    fn layout_ordena_y_descarta_sin_geometria() {
        let l = layout();
        assert_eq!(l.count(), 2);
        assert_eq!(l.rect_of("eDP-1"), Some((0, 1920)));
        assert_eq!(l.rect_of("HDMI-A-1"), Some((1920, 1440)));
        assert_eq!(l.rect_of("VGA-1"), None);
        assert_eq!(l.global_bounds(), Some((0, 3360)));
    }

    #[test]
    fn owner_at_por_el_centro_del_gato() {
        let l = layout();
        assert_eq!(l.owner_at(10.0), Some("eDP-1"));
        assert_eq!(l.owner_at(1919.0), Some("eDP-1"));
        assert_eq!(
            l.owner_at(1920.0),
            Some("HDMI-A-1"),
            "la costura pertenece a la derecha"
        );
        assert_eq!(l.owner_at(3000.0), Some("HDMI-A-1"));
        // fuera de todo → la de borde más cercano
        assert_eq!(l.owner_at(-500.0), Some("eDP-1"));
        assert_eq!(l.owner_at(9999.0), Some("HDMI-A-1"));
    }

    #[test]
    fn neighbor_izquierda_derecha_y_extremos() {
        let l = layout();
        assert_eq!(l.neighbor("eDP-1", 1), Some("HDMI-A-1"));
        assert_eq!(
            l.neighbor("eDP-1", -1),
            None,
            "eDP-1 es el extremo izquierdo"
        );
        assert_eq!(l.neighbor("HDMI-A-1", -1), Some("eDP-1"));
        assert_eq!(
            l.neighbor("HDMI-A-1", 1),
            None,
            "HDMI es el extremo derecho"
        );
        assert_eq!(l.neighbor("eDP-1", 0), None);
    }
}
