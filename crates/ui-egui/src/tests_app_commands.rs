//! UI commands of `commands/app.rs` driven as an agent does (by id, with params): the refusals they give, preset,
//! point-curve and effect files exported and imported again, the tracklog, and System Info.
use serde_json::{Value, json};

use crate::{LightcraftApp, Services};

fn app() -> LightcraftApp {
    LightcraftApp::new(lightcraft_engine::Session::with_demo(), Services { png: None, ..Default::default() })
}

fn temp(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lc-ui-appcmd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn toast(app: &LightcraftApp) -> String {
    app.ui.toast.as_ref().map(|t| t.0.to_string()).unwrap_or_default()
}

#[test]
fn commands_refuse_params_that_belong_to_another_command() {
    let mut app = app();
    let e = app.run("app.settings", json!({"tab": "colours"})).unwrap_err();
    assert!(e.message.starts_with("unknown settings tab `colours` (general|import|"), "{e}");
    assert_eq!(
        app.run("file.addPhotos", json!({"path": "/x"})).unwrap_err().message,
        "file.addPhotos: takes `paths` (file.addFolder takes a `path`)"
    );
    assert_eq!(
        app.run("file.addFolder", json!({"paths": ["/x"]})).unwrap_err().message,
        "file.addFolder: takes a `path` (file.addPhotos takes `paths`)"
    );
    assert!(app.run("file.addFromDevice", json!({"paths": ["/x"]})).unwrap_err().message.ends_with("takes a `path` (a card's DCIM folder)"));
    assert!(app.ui.dialog.is_none(), "a refusal opens nothing");
}

#[test]
fn without_a_file_dialog_a_command_that_needs_a_file_says_so_or_does_nothing() {
    let mut app = app();
    for id in ["file.exportCurvePresets", "file.importCurvePresets", "file.exportMaskEffects", "file.exportKeywords", "file.importKeywords"] {
        assert_eq!(app.run(id, json!({})).unwrap_err().message, "no file dialog on this platform", "{id}");
    }
    // these have nothing to do without a file: no error, no change
    assert_eq!(app.run("file.importLightroom", json!({})).unwrap(), Value::Null);
    assert_eq!(app.run("photo.locate", json!({})).unwrap(), Value::Null);
    assert_eq!(app.run("photo.tagFromTracklog", json!({})).unwrap(), Value::Null);
    assert_eq!(app.run("file.importCurvePresets", json!({"paths": []})).unwrap(), Value::Null);
    // a picker that is cancelled is not an error either
    app.services.save_curve_preset_file = Some(Box::new(|_: &str| None));
    assert_eq!(app.run("file.exportCurvePresets", json!({})).unwrap(), Value::Null);
}

#[test]
fn point_curve_presets_export_and_import_again() {
    let mut app = app();
    let dir = temp("curves");
    let mut d = (*app.session.develop_of(app.session.active().unwrap()).unwrap()).clone();
    d.curve.master = serde_json::from_value(json!([{"x": 0.0, "y": 0.0}, {"x": 0.5, "y": 0.62}, {"x": 1.0, "y": 1.0}])).unwrap();
    app.run("develop.merge", json!({"settings": {"curve": d.curve}})).unwrap();
    app.run("curve.savePreset", json!({"name": "Lift"})).unwrap();
    let file = dir.join("mine.lccurve");
    let r = app.run("file.exportCurvePresets", json!({"path": file, "names": ["Lift"]})).unwrap();
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(toast(&app), "Exported 1 point curve preset");
    assert!(file.is_file());

    app.run("curve.deletePreset", json!({"name": "Lift"})).unwrap();
    let junk = dir.join("junk.lccurve");
    std::fs::write(&junk, b"not a preset").unwrap();
    let r = app.run("file.importCurvePresets", json!({"paths": [file, junk]})).unwrap();
    assert_eq!(r["imported"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(toast(&app), "Imported 1 point curve preset, 1 file not readable");
    assert!(app.session.prefs.curve_presets.iter().any(|p| p.name == "Lift"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn develop_presets_export_and_import_again() {
    let mut app = app();
    let dir = temp("presets");
    app.run("develop.set", json!({"control": "light.exposure", "value": 0.7})).unwrap();
    let created = app.run("preset.create", json!({"name": "Bright", "group": "Mine"})).unwrap();
    let id = created["id"].as_str().map(str::to_string).unwrap_or_else(|| panic!("{created}"));
    let file = dir.join("Mine.lcpreset");
    let r = app.run("file.exportPresets", json!({"path": file, "ids": [id]})).unwrap();
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(toast(&app), "Exported 1 preset");

    app.run("preset.delete", json!({"id": id})).unwrap();
    app.ui.presets = false;
    let r = app.run("file.importPresets", json!({"paths": [file]})).unwrap();
    assert!(toast(&app).starts_with("Imported 1 preset"), "{}: {r}", toast(&app));
    assert!(app.ui.presets, "the Presets panel opens on what was imported");
    assert!(app.session.presets.iter().any(|p| p.name == "Bright"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mask_effects_export_to_a_file() {
    let mut app = app();
    let dir = temp("effects");
    app.run("mask.add", json!({"kind": "radial"})).unwrap();
    app.run("mask.adjust", json!({"values": {"exposure": 0.5}})).unwrap();
    app.run("mask.saveEffect", json!({"name": "Glow", "group": "Faces"})).unwrap();
    let file = dir.join("Faces.lceffect");
    let r = app.run("file.exportMaskEffects", json!({"path": file, "group": "Faces"})).unwrap();
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(toast(&app), "Exported 1 effect preset");
    assert!(file.is_file());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_tracklog_asks_for_the_camera_zone_then_tags_photos() {
    let mut app = app();
    let gpx = r#"<?xml version="1.0"?><gpx version="1.1"><trk><trkseg>
        <trkpt lat="38.72" lon="-9.14"><time>2000-01-01T00:00:00Z</time></trkpt>
        <trkpt lat="38.73" lon="-9.15"><time>2000-01-01T00:10:00Z</time></trkpt>
    </trkseg></trk></gpx>"#;
    let ids: Vec<u64> = app.session.visible().iter().take(2).map(|p| p.0).collect();
    // without an offset: the camera's time zone is asked for first
    assert_eq!(app.run("photo.tagFromTracklog", json!({"gpx": gpx, "ids": ids})).unwrap(), Value::Null);
    assert!(app.ui.dialog.is_some(), "the zone prompt opens");
    app.ui.dialog = None;
    let r = app.run("photo.tagFromTracklog", json!({"gpx": gpx, "ids": ids, "offset": "+00:00"})).unwrap();
    assert!(toast(&app).starts_with("Tagged "), "{}: {r}", toast(&app));
    if r["skipped"]["outside"].as_u64().unwrap_or(0) > 0 {
        assert!(toast(&app).contains("outside its time range"), "{}", toast(&app));
    }
}

#[test]
fn system_info_lists_the_preview_size_and_the_library() {
    let mut app = app();
    app.ui.settings.preview_limit = 1600;
    let rows = app.run("app.systemInfo", json!({"open": false})).unwrap();
    let value = |label: &str| rows.as_array().unwrap().iter().find(|r| r["label"] == label).map(|r| r["value"].clone());
    assert_eq!(value("Preview size"), Some(json!("1600 px")));
    assert!(value("GPU").is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty())));
    assert!(app.ui.dialog.is_none(), "open: false only reports");
    app.run("app.systemInfo", json!({})).unwrap();
    assert!(matches!(app.ui.dialog, Some(crate::state::Dialog::SystemInfo { .. })));
}

#[test]
fn merging_the_last_panorama_needs_photos() {
    let mut app = app();
    let first = app.session.visible()[0].0;
    app.session.execute("library.select", &json!({"ids": [first]})).unwrap();
    assert!(app.run("merge.panoramaLast", json!({})).is_err());
    assert!(app.run("merge.hdrPanoramaLast", json!({})).is_err());
}
