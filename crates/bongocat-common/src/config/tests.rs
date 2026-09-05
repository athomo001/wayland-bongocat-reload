//! Casos portados de `tests/test_config.c` (fuente independiente). El C prueba
//! `load_config` sobre ficheros temporales; aquí se prueba `parse_ini` sobre el
//! mismo contenido — la lógica de parseo/validación es la misma.

use super::*;

#[test]
fn defaults_con_configuracion_vacia() {
    let (c, w) = parse_ini("\n");
    assert!(w.is_empty(), "sin avisos: {w:?}");
    assert_eq!(c.fps, 60);
    assert_eq!(c.cat_height, 40);
    assert_eq!(c.overlay_height, 50);
    assert_eq!(c.overlay_opacity, 150);
    assert_eq!(c.overlay_position, Position::Top);
    assert_eq!(c.layer, Layer::Top);
    assert!(c.enable_antialiasing);
    assert!(c.enable_hand_mapping);
    assert_eq!(c.cat_x_offset, 100);
    assert_eq!(c.cat_y_offset, 10);
    assert_eq!(c.keypress_duration, 100);
    assert_eq!(c, Config::default());
}

#[test]
fn clamping_de_enteros() {
    let (c, w) = parse_ini(
        "fps=999\ncat_height=0\noverlay_opacity=-50\noverlay_height=1\ncat_opacity=250\n",
    );
    assert_eq!(c.fps, 120, "fps al máximo");
    assert_eq!(c.cat_height, 10, "cat_height al mínimo");
    assert_eq!(c.overlay_opacity, 0, "opacidad al mínimo");
    assert_eq!(c.overlay_height, 20, "overlay_height al mínimo");
    assert_eq!(c.cat_opacity, 100, "cat_opacity al máximo (%)");
    assert_eq!(w.len(), 5, "un aviso por cada clamp: {w:?}");
}

#[test]
fn enable_tray_por_defecto_y_parseo() {
    assert!(parse_ini("").0.enable_tray, "activo por defecto");
    let (c, w) = parse_ini("enable_tray=0\n");
    assert!(w.is_empty());
    assert!(!c.enable_tray);
    assert!(parse_ini("enable_tray=2\n").1.len() == 1, "solo 0/1");
}

#[test]
fn happy_kpm_parsea_y_se_recorta() {
    let (c, w) = parse_ini("");
    assert_eq!(c.happy_kpm, 0, "desactivado por defecto");
    assert!(w.is_empty());

    let (c, w) = parse_ini("happy_kpm=180\n");
    assert!(w.is_empty());
    assert_eq!(c.happy_kpm, 180);

    let (c, w) = parse_ini("happy_kpm=99999\n");
    assert_eq!(c.happy_kpm, 10_000, "tope 10000");
    assert_eq!(w.len(), 1);

    // ida y vuelta por to_ini
    let (round, _) = parse_ini(&c.to_ini());
    assert_eq!(round.happy_kpm, c.happy_kpm);
}

#[test]
fn cat_opacity_por_defecto_y_ronda() {
    let (c, _) = parse_ini("");
    assert_eq!(c.cat_opacity, 100, "por defecto opaco");
    let (c2, w) = parse_ini("cat_opacity=60\n");
    assert!(w.is_empty());
    assert_eq!(c2.cat_opacity, 60);
    // ida y vuelta por to_ini
    let (c3, _) = parse_ini(&c2.to_ini());
    assert_eq!(c3.cat_opacity, 60);
}

#[test]
fn parseo_de_horas() {
    let (c, w) = parse_ini("enable_scheduled_sleep=1\nsleep_begin=22:30\nsleep_end=06:15\n");
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(c.sleep_begin, Time { hour: 22, min: 30 });
    assert_eq!(c.sleep_end, Time { hour: 6, min: 15 });
    assert!(c.enable_scheduled_sleep, "begin != end, sigue activo");
}

#[test]
fn enteros_malformados_conservan_el_valor_por_defecto() {
    let (c, _) = parse_ini("fps=abc\n");
    assert_eq!(c.fps, 60, "'abc' rechazado, fps queda por defecto");

    let (c, w) = parse_ini("fps=60junk\nsleep_begin=22:30junk\nenable_debug=2\n");
    assert_eq!(c.fps, 60, "basura final rechazada");
    assert_eq!(c.sleep_begin, Time { hour: 0, min: 0 }, "hora inválida");
    assert!(!c.enable_debug, "booleano inválido (2) rechazado");
    assert_eq!(w.len(), 3, "tres avisos: {w:?}");
}

#[test]
fn lista_de_monitores() {
    let (c, w) = parse_ini("monitor=eDP-1, HDMI-A-1 , DP-2\n");
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(c.output_names, ["eDP-1", "HDMI-A-1", "DP-2"]);
    assert_eq!(c.output_name.as_deref(), Some("eDP-1"));
}

#[test]
fn validacion_de_ruta_de_dispositivo() {
    let (c, _) = parse_ini("keyboard_device=/dev/input/event0\n");
    assert_eq!(c.keyboard_devices, ["/dev/input/event0"]);

    // Path traversal → rechazado (en `parse_ini` no se añade el dispositivo por
    // defecto; eso es cosa del cargador de fichero).
    let (c, w) = parse_ini("keyboard_device=/dev/input/../shadow\n");
    assert!(c.keyboard_devices.is_empty());
    assert_eq!(w.len(), 1);

    let (c, w) = parse_ini("keyboard_device=/etc/passwd\n");
    assert!(c.keyboard_devices.is_empty());
    assert_eq!(w.len(), 1);
}

#[test]
fn parseo_de_enums() {
    let (c, w) = parse_ini("overlay_position=bottom\nlayer=background\ncat_align=right\n");
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(c.overlay_position, Position::Bottom);
    assert_eq!(c.layer, Layer::Background);
    assert_eq!(c.cat_align, Align::Right);

    for (name, expected) in [
        ("background", Layer::Background),
        ("bottom", Layer::Bottom),
        ("top", Layer::Top),
        ("overlay", Layer::Overlay),
    ] {
        let (c, w) = parse_ini(&format!("layer={name}\n"));
        assert!(w.is_empty());
        assert_eq!(c.layer, expected, "layer={name}");
    }
}

#[test]
fn to_ini_hace_ida_y_vuelta() {
    // La configuración por defecto emitida como INI vuelve a parsearse idéntica.
    let original = Config::default();
    let (reparsed, w) = parse_ini(&original.to_ini());
    assert!(w.is_empty(), "el INI emitido no debe generar avisos: {w:?}");
    assert_eq!(reparsed, original);

    // Y con valores no triviales.
    let (mut custom, _) = parse_ini(
        "fps=45\nlayer=overlay\ncat_align=right\nsleep_begin=07:05\nmonitor=eDP-1,HDMI-A-1\nkeyboard_device=/dev/input/event9\n",
    );
    custom.enable_scheduled_sleep = true;
    custom.sleep_end = Time { hour: 8, min: 0 };
    let (round, _) = parse_ini(&custom.to_ini());
    assert_eq!(round, custom);
}

#[test]
fn set_live_valida_recorta_y_falla() {
    let mut c = Config::default();

    // Válido y en rango: sin avisos.
    assert_eq!(set_live(&mut c, "fps", "72"), Ok(Vec::new()));
    assert_eq!(c.fps, 72);

    // Válido pero fuera de rango: se recorta y avisa.
    let w = set_live(&mut c, "cat_opacity", "500").unwrap();
    assert_eq!(c.cat_opacity, 100);
    assert_eq!(w.len(), 1);

    // Tipo inválido: error y sin cambios.
    assert!(set_live(&mut c, "fps", "abc").is_err());
    assert_eq!(c.fps, 72, "un SET inválido no toca la config");

    // Clave desconocida: error.
    assert!(set_live(&mut c, "no_existe", "1").is_err());
}

#[test]
fn factory_config_es_el_example_y_no_avisa() {
    let (_, w) = parse_ini(EXAMPLE_INI);
    assert!(
        w.is_empty(),
        "el bongocat.conf.example no debe generar avisos: {w:?}"
    );
    let f = factory_config();
    // Valores que el usuario ve como "normales" (≠ Config::default()).
    assert_eq!(f.cat_height, 110);
    assert_eq!(f.cat_opacity, 100);
    assert_ne!(f, Config::default(), "fábrica ≠ default del parser");
}

#[test]
fn comentarios_y_espacios_en_blanco() {
    let src = "# esto es un comentario\n  fps = 30  # comentario en línea\n\n   \t  \n; comentario con punto y coma\ncat_height = 100\n";
    let (c, w) = parse_ini(src);
    assert_eq!(c.fps, 30);
    assert_eq!(c.cat_height, 100);
    // La línea con ';' no es comentario válido en este formato → un aviso.
    assert_eq!(w.len(), 1, "solo la línea del ';' avisa: {w:?}");
    assert!(w[0].contains("punto y coma"));
}
