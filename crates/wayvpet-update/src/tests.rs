//! Tests del hito M1 (spec 0015): comparación semver y parseo acotado del
//! `update-check.json`. Sin red y sin depender del entorno real salvo un
//! fichero temporal por test.

use super::*;

// ── Comparación semver ────────────────────────────────────────────────────────

#[test]
fn newer_por_numero() {
    assert!(is_newer("0.10.0", "0.9.9"), "0.10.0 > 0.9.9 (no es texto)");
    assert!(is_newer("1.2.4", "1.2.3"));
    assert!(is_newer("2.0.0", "1.99.99"));
    assert!(!is_newer("1.2.3", "1.2.4"));
    assert!(!is_newer("0.9.9", "0.10.0"));
}

#[test]
fn iguales_no_es_newer() {
    assert!(!is_newer("1.2.3", "1.2.3"));
    assert!(!is_newer("v1.2.3", "1.2.3"), "el prefijo v no cambia nada");
    assert!(!is_newer("  1.2.3  ", "1.2.3"), "espacios tolerados");
}

#[test]
fn latest_prerelease_no_dispara() {
    // Canales beta/nightly quedan fuera: una rc remota no cuenta como release.
    assert!(!is_newer("0.6.0-rc1", "0.5.0"));
    assert!(!is_newer("1.0.0-beta.2", "0.9.0"));
    // Pero la estable sí supera a quien corre una pre-release de esa misma.
    assert!(is_newer("1.0.0", "1.0.0-rc1"));
}

#[test]
fn no_semver_nunca_dispara() {
    assert!(!is_newer("ultima", "1.0.0"));
    assert!(!is_newer("1.0", "1.0.0"), "falta el patch");
    assert!(!is_newer("1.0.0", "vNIGHTLY"));
    assert!(!is_newer("", "1.0.0"));
}

// ── Modelo y parseo del estado ───────────────────────────────────────────────

const RELEASE_JSON: &str = r#"{
  "checked_at": "2026-09-06T12:00:00Z",
  "installed": "0.5.0",
  "latest": "0.6.0",
  "url": "https://github.com/athomo001/wayvpet/releases/tag/v0.6.0",
  "notes": "Arreglos varios.",
  "assets": [
    { "name": "wayvpet_0.6.0_amd64.deb", "url": "https://example/deb", "sha256": "aa" },
    { "name": "wayvpet-0.6.0.tar.gz", "url": "https://example/tgz" }
  ]
}"#;

#[test]
fn parsea_un_release_completo() {
    let st = parse_state(RELEASE_JSON.as_bytes()).expect("JSON válido");
    assert_eq!(st.latest.as_deref(), Some("0.6.0"));
    assert_eq!(st.installed.as_deref(), Some("0.5.0"));
    assert_eq!(st.assets.len(), 2);
    assert_eq!(st.assets[0].sha256.as_deref(), Some("aa"));
    assert_eq!(
        st.assets[1].sha256, None,
        "asset sin checksum → None, no error"
    );
    assert!(st.error.is_none());
    assert!(st.update_available("0.0.0"));
    assert_eq!(
        st.status_line("0.0.0"),
        "v0.6.0 disponible — https://github.com/athomo001/wayvpet/releases/tag/v0.6.0"
    );
}

#[test]
fn parsea_estado_de_error() {
    let st = parse_state(br#"{ "checked_at": "2026-09-06T12:00:00Z", "error": "sin red" }"#)
        .expect("un estado de error también es JSON válido");
    assert_eq!(st.error.as_deref(), Some("sin red"));
    assert!(st.latest.is_none());
    assert!(
        !st.update_available("0.0.0"),
        "con error nunca hay aviso, aunque falte 'latest'"
    );
    assert_eq!(
        st.status_line("0.0.0"),
        "sin datos (el último intento falló: sin red)"
    );
}

#[test]
fn error_gana_aunque_haya_latest() {
    let st =
        parse_state(br#"{ "checked_at": "x", "latest": "9.9.9", "error": "rate limit" }"#).unwrap();
    assert!(!st.update_available("1.0.0"));
}

#[test]
fn campos_desconocidos_se_ignoran() {
    let st = parse_state(
        br#"{ "checked_at": "x", "latest": "1.0.0", "futuro": 42, "otro": { "a": 1 } }"#,
    )
    .expect("una clave nueva del hijo no debe romper a un lector viejo");
    assert_eq!(st.latest.as_deref(), Some("1.0.0"));
}

#[test]
fn campos_ausentes_toman_su_default() {
    let st = parse_state(b"{}").expect("objeto vacío = estado por defecto");
    assert_eq!(st, UpdateState::default());
    assert!(st.checked_at.is_empty());
    assert!(st.assets.is_empty());
    assert_eq!(
        st.status_line("1.0.0"),
        "sin datos (todavía no se ha comprobado)"
    );
}

#[test]
fn rechaza_un_fichero_gigante_sin_parsear() {
    // JSON válido pero por encima del tope: se rechaza por tamaño.
    let mut big = String::from("{ \"notes\": \"");
    big.push_str(&"a".repeat(MAX_STATE_BYTES));
    big.push_str("\" }");
    match parse_state(big.as_bytes()) {
        Err(StateError::TooLarge { size }) => assert!(size > MAX_STATE_BYTES),
        other => panic!("esperaba TooLarge, salió {other:?}"),
    }
}

#[test]
fn justo_en_el_tope_se_parsea() {
    let mut buf = br#"{"checked_at":"x"}"#.to_vec();
    buf.resize(MAX_STATE_BYTES, b' '); // padding con espacios: sigue siendo JSON válido
    assert!(parse_state(&buf).is_ok());
    buf.push(b' ');
    assert!(matches!(
        parse_state(&buf),
        Err(StateError::TooLarge { .. })
    ));
}

#[test]
fn json_invalido_es_error_de_parseo() {
    assert!(matches!(
        parse_state(b"no soy json"),
        Err(StateError::Parse(_))
    ));
}

#[test]
fn to_json_da_una_vuelta_completa() {
    let st = parse_state(RELEASE_JSON.as_bytes()).unwrap();
    let rendered = to_json(&st);
    assert!(rendered.ends_with('\n'), "termina en salto de línea");
    let again = parse_state(rendered.as_bytes()).unwrap();
    assert_eq!(st, again);
}

#[test]
fn write_state_atomic_crea_directorio_y_da_la_vuelta() {
    let dir = std::env::temp_dir().join(format!("wayvpet-atomic-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    // El padre no existe todavía: write_state_atomic lo crea.
    let p = dir.join("sub/update-check.json");

    let st = parse_state(RELEASE_JSON.as_bytes()).unwrap();
    write_state_atomic(&p, &st).unwrap();

    assert_eq!(read_state(&p).unwrap().as_ref(), Some(&st));
    // No queda ningún temporal al lado.
    let sobras: Vec<_> = std::fs::read_dir(p.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(sobras.is_empty(), "temporales sin limpiar: {sobras:?}");

    // Sobrescribir en el sitio también funciona.
    let st2 = UpdateState {
        checked_at: "2026-09-07T00:00:00Z".into(),
        error: Some("sin red".into()),
        ..Default::default()
    };
    write_state_atomic(&p, &st2).unwrap();
    assert_eq!(read_state(&p).unwrap(), Some(st2));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn write_notice_escribe_si_hay_version_nueva_y_borra_si_no() {
    let dir = std::env::temp_dir().join(format!("wayvpet-notice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let state_p = dir.join("update-check.json");
    let notice_p = notice_path(&state_p);

    // El JSON dice latest=0.6.0, installed=0.5.0 → se escribe el aviso plano.
    // (El campo `installed` del propio estado manda sobre el fallback.)
    let st = parse_state(RELEASE_JSON.as_bytes()).unwrap();
    write_notice(&state_p, &st, "0.0.0").unwrap();
    let body = std::fs::read_to_string(&notice_p).unwrap();
    assert!(body.contains("latest=0.6.0"), "{body}");
    assert!(
        body.contains("url=https://github.com/athomo001/wayvpet/releases/tag/v0.6.0"),
        "{body}"
    );

    // Estado "al día" (installed == latest) → el aviso se borra, no queda colgado.
    let al_dia = UpdateState {
        checked_at: "x".into(),
        installed: Some("0.6.0".into()),
        latest: Some("0.6.0".into()),
        ..Default::default()
    };
    write_notice(&state_p, &al_dia, "0.0.0").unwrap();
    assert!(
        !notice_p.exists(),
        "el aviso debe desaparecer si estoy al día"
    );

    // Borrar cuando no existe no es error.
    write_notice(&state_p, &al_dia, "9.9.9").unwrap();

    // Un estado de error tampoco deja aviso.
    let err = UpdateState {
        checked_at: "x".into(),
        error: Some("sin red".into()),
        ..Default::default()
    };
    std::fs::write(&notice_p, "latest=1.2.3\n").unwrap();
    write_notice(&state_p, &err, "0.1.0").unwrap();
    assert!(!notice_p.exists());

    std::fs::remove_dir_all(&dir).ok();
}

// ── Ruta XDG y lectura de disco ─────────────────────────────────────────────

#[test]
fn state_path_prefiere_xdg_state_home() {
    let env = |k: &str| match k {
        "XDG_STATE_HOME" => Some("/xdg/state".to_string()),
        "HOME" => Some("/home/u".to_string()),
        _ => None,
    };
    assert_eq!(
        state_path(env),
        Some(PathBuf::from("/xdg/state/wayvpet/update-check.json"))
    );
}

#[test]
fn state_path_cae_a_home_local_state() {
    let env = |k: &str| (k == "HOME").then(|| "/home/u".to_string());
    assert_eq!(
        state_path(env),
        Some(PathBuf::from(
            "/home/u/.local/state/wayvpet/update-check.json"
        ))
    );
}

#[test]
fn state_path_sin_entorno_es_none() {
    assert_eq!(state_path(|_| None), None);
    assert_eq!(
        state_path(|k| (k == "XDG_STATE_HOME").then(String::new)),
        None
    );
}

#[test]
fn read_state_fichero_ausente_es_ok_none() {
    let p = Path::new("/no/existe/wayvpet/update-check.json");
    assert!(read_state(p).unwrap().is_none());
}

#[test]
fn read_state_lee_y_parsea_un_fichero_real() {
    let dir = std::env::temp_dir().join(format!("wayvpet-update-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let ok = dir.join("update-check.json");
    std::fs::write(&ok, RELEASE_JSON).unwrap();
    let st = read_state(&ok).unwrap().expect("Some");
    assert_eq!(st.latest.as_deref(), Some("0.6.0"));

    let bad = dir.join("bad.json");
    std::fs::write(&bad, "{ roto").unwrap();
    let err = read_state(&bad).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    let huge = dir.join("huge.json");
    std::fs::write(&huge, "\"".repeat(MAX_STATE_BYTES + 10)).unwrap();
    assert_eq!(
        read_state(&huge).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );

    std::fs::remove_dir_all(&dir).ok();
}
