//! Configuración y perfil de comportamiento de mascotas virtuales (vPets).
//!
//! Cada carpeta de tema/vPet puede incluir un archivo `vpet.ini` con sus
//! características y dinámicas particulares (caminar por la pantalla, acciones
//! de ocio en reposo como comer RAM, velocidades, márgenes y seguimiento de ratón).
//! Si no existe `vpet.ini`, se leen las claves compatibles presentes en `theme.ini`.

use crate::config::{split_line, Align};

/// Perfil de configuración y comportamiento de un vPet.
#[derive(Debug, Clone, PartialEq)]
pub struct VpetConfig {
    /// Altura del personaje en píxeles.
    pub cat_height: Option<u32>,
    /// Alineación base en el overlay (Left, Center, Right).
    pub cat_align: Option<Align>,
    /// Desplazamiento horizontal relativo.
    pub cat_x_offset: Option<i32>,
    /// Desplazamiento vertical relativo.
    pub cat_y_offset: Option<i32>,
    /// Indica si el vPet se desplaza/patrulla por la pantalla.
    pub can_roam: bool,
    /// Velocidad de desplazamiento en píxeles por segundo.
    pub roam_speed: u32,
    /// Margen mínimo respecto al borde de la pantalla antes de girar (px).
    pub roam_margin: i32,
    /// Si voltea horizontalmente el sprite según la dirección en que camina.
    pub flip_on_walk: bool,
    /// Lista de acciones que realiza durante el reposo (ej. ["walk", "eat_ram"]).
    pub idle_actions: Vec<String>,
    /// Intervalo de tiempo (en segundos de inactividad) para cambiar de acción de ocio.
    pub idle_action_interval: u64,
    /// Duración de cada acción de ocio en segundos.
    pub idle_action_duration: u64,
    /// Si sigue la posición del cursor con la mirada.
    pub track_mouse: bool,
    /// Tiempo propio de inactividad en segundos antes de dormir (None = hereda global).
    pub sleep_timeout: Option<u64>,
}

impl Default for VpetConfig {
    fn default() -> Self {
        Self {
            cat_height: None,
            cat_align: None,
            cat_x_offset: None,
            cat_y_offset: None,
            can_roam: false,
            roam_speed: 45,
            roam_margin: 32,
            flip_on_walk: true,
            idle_actions: Vec::new(),
            idle_action_interval: 12,
            idle_action_duration: 5,
            track_mouse: true,
            sleep_timeout: None,
        }
    }
}

impl VpetConfig {
    /// Combina con otra configuración de respaldo (usada para absorber valores
    /// definidos en `theme.ini` cuando no se especificaron en `vpet.ini`).
    pub fn merge_fallback(&mut self, fallback: &VpetConfig) {
        if self.cat_height.is_none() {
            self.cat_height = fallback.cat_height;
        }
        if self.cat_align.is_none() {
            self.cat_align = fallback.cat_align;
        }
        if self.cat_x_offset.is_none() {
            self.cat_x_offset = fallback.cat_x_offset;
        }
        if self.cat_y_offset.is_none() {
            self.cat_y_offset = fallback.cat_y_offset;
        }
        if !self.can_roam && fallback.can_roam {
            self.can_roam = true;
        }
        if self.roam_speed == 45 && fallback.roam_speed != 45 {
            self.roam_speed = fallback.roam_speed;
        }
        if self.idle_actions.is_empty() && !fallback.idle_actions.is_empty() {
            self.idle_actions = fallback.idle_actions.clone();
        }
        if self.sleep_timeout.is_none() {
            self.sleep_timeout = fallback.sleep_timeout;
        }
    }
}

/// Parsea un contenido en formato INI con las claves de comportamiento del vPet.
#[must_use]
pub fn parse_vpet_ini(content: &str) -> VpetConfig {
    let mut cfg = VpetConfig::default();

    for raw in content.lines() {
        let t = raw.trim_start_matches([' ', '\t']);
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        let Some(l) = split_line(raw) else { continue };
        let v = l.value;
        let v_lower = v.to_ascii_lowercase();

        match l.key.as_str() {
            "cat_height" | "height" => {
                if let Ok(n) = v.parse::<u32>() {
                    cfg.cat_height = Some(n);
                }
            }
            "cat_align" | "align" => {
                cfg.cat_align = match v_lower.as_str() {
                    "left" => Some(Align::Left),
                    "right" => Some(Align::Right),
                    "center" => Some(Align::Center),
                    _ => None,
                };
            }
            "cat_x_offset" | "x_offset" => {
                if let Ok(n) = v.parse::<i32>() {
                    cfg.cat_x_offset = Some(n);
                }
            }
            "cat_y_offset" | "y_offset" => {
                if let Ok(n) = v.parse::<i32>() {
                    cfg.cat_y_offset = Some(n);
                }
            }
            "can_roam" | "enable_roam" | "roam" | "walk_screen" => {
                cfg.can_roam = matches!(v_lower.as_str(), "1" | "true" | "yes" | "on");
            }
            "roam_speed" | "walk_speed" | "speed" => {
                if let Ok(n) = v.parse::<u32>() {
                    cfg.roam_speed = n.clamp(1, 500);
                }
            }
            "roam_margin" | "margin" => {
                if let Ok(n) = v.parse::<i32>() {
                    cfg.roam_margin = n.clamp(0, 500);
                }
            }
            "flip_on_walk" | "flip_walk" | "flip_horizontal" => {
                cfg.flip_on_walk = matches!(v_lower.as_str(), "1" | "true" | "yes" | "on");
            }
            "idle_actions" | "actions" => {
                cfg.idle_actions = v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "idle_action_interval" | "action_interval" => {
                if let Ok(n) = v.parse::<u64>() {
                    cfg.idle_action_interval = n.max(1);
                }
            }
            "idle_action_duration" | "action_duration" => {
                if let Ok(n) = v.parse::<u64>() {
                    cfg.idle_action_duration = n.max(1);
                }
            }
            "track_mouse" | "mouse_tracking" | "enable_gaze" => {
                cfg.track_mouse = matches!(v_lower.as_str(), "1" | "true" | "yes" | "on");
            }
            "sleep_timeout" | "idle_sleep_timeout" => {
                if let Ok(n) = v.parse::<u64>() {
                    cfg.sleep_timeout = Some(n);
                }
            }
            _ => {}
        }
    }

    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vpet_completo() {
        let ini = r#"
            # Perfil vPet
            cat_height = 124
            cat_align = center
            cat_x_offset = 5
            cat_y_offset = -3
            can_roam = 1
            roam_speed = 50
            roam_margin = 40
            flip_on_walk = 1
            idle_actions = walk, eat_ram
            idle_action_interval = 14
            idle_action_duration = 6
            track_mouse = 1
            sleep_timeout = 45
        "#;
        let c = parse_vpet_ini(ini);
        assert_eq!(c.cat_height, Some(124));
        assert_eq!(c.cat_align, Some(Align::Center));
        assert_eq!(c.cat_x_offset, Some(5));
        assert_eq!(c.cat_y_offset, Some(-3));
        assert!(c.can_roam);
        assert_eq!(c.roam_speed, 50);
        assert_eq!(c.roam_margin, 40);
        assert!(c.flip_on_walk);
        assert_eq!(c.idle_actions, vec!["walk", "eat_ram"]);
        assert_eq!(c.idle_action_interval, 14);
        assert_eq!(c.idle_action_duration, 6);
        assert!(c.track_mouse);
        assert_eq!(c.sleep_timeout, Some(45));
    }

    #[test]
    fn parse_vpet_vacio_y_merge() {
        let mut a = parse_vpet_ini("");
        assert!(!a.can_roam);
        assert_eq!(a.roam_speed, 45);

        let b = parse_vpet_ini(
            r#"
            cat_height = 100
            can_roam = 1
            roam_speed = 60
            idle_actions = walk
        "#,
        );
        a.merge_fallback(&b);
        assert_eq!(a.cat_height, Some(100));
        assert!(a.can_roam);
        assert_eq!(a.roam_speed, 60);
        assert_eq!(a.idle_actions, vec!["walk"]);
    }
}
