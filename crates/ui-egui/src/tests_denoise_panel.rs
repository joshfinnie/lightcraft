//! The AI Denoise panel as the user meets it: the Settings tab with a model installed, the line under the Detail
//! panel's slider for each state a photo goes through, and the model dialog's terms.
use std::time::Duration;

use lightcraft_catalog::PhotoId;
use lightcraft_engine::denoise::PhotoState;
use serde_json::{Value, json};

use crate::headless::Headless;
use crate::state::Dialog;
use crate::tests_model_setup::{install_denoise, setup};

const T: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(120);
const MODEL: &str = "test-smoothing";

fn settings(name: &str) -> (Headless, std::path::PathBuf, PhotoId) {
    let (mut app, dir, raw) = setup(name);
    install_denoise(&mut app, &dir);
    app.ui.dialog = Some(Dialog::Settings { tab: "denoise".into() });
    let mut h = Headless::new(app, [1300.0, 1000.0], 1.0);
    h.settle(SETTLE);
    (h, dir, raw)
}

fn click(h: &mut Headless, id: &str) {
    let r = h.request("ui.clickWidget", json!({"id": id}), T);
    assert_eq!(r["ok"], true, "{id}: {r}");
    h.step();
    h.step();
}

fn has_widget(h: &Headless, id: &str) -> bool {
    h.app.widgets.iter().any(|(w, _)| w == id)
}

fn painted(h: &Headless, text: &str) -> bool {
    h.painted_text().iter().any(|s| s.contains(text))
}

fn listed(h: &mut Headless) -> Value {
    h.app.session.execute("denoise.models.list", &json!({})).unwrap()
}

fn model(h: &mut Headless) -> Value {
    listed(h)["models"].as_array().unwrap().iter().find(|m| m["id"] == MODEL).cloned().unwrap()
}

// Scenario: an installed model is listed as in use, and its buttons turn it off, back on and remove it
#[test]
fn the_settings_tab_turns_an_installed_model_off_on_and_removes_it() {
    let (mut h, dir, _) = settings("panel-model");
    assert!(painted(&h, "Synthetic smoothing"), "{:?}", h.painted_text());
    assert!(painted(&h, "in use"));
    assert!(painted(&h, "The processor does the work."), "runOn cpu says so: {:?}", h.painted_text());
    assert!(painted(&h, "0 photos have their denoised picture."));
    assert!(painted(&h, "0 cached pictures"));

    click(&mut h, &format!("denoise:off:{MODEL}"));
    assert_eq!(model(&mut h)["selected"], false);
    h.settle(SETTLE);
    assert!(painted(&h, "AI Denoise is off: press Use on a model."), "{:?}", h.painted_text());
    assert!(painted(&h, "installed"));

    click(&mut h, &format!("denoise:use:{MODEL}"));
    assert_eq!(model(&mut h)["selected"], true);
    h.settle(SETTLE);
    assert!(!painted(&h, "AI Denoise is off"));

    click(&mut h, &format!("denoise:remove:{MODEL}"));
    assert!(listed(&mut h)["models"].as_array().unwrap().iter().all(|m| m["id"] != MODEL), "{}", listed(&mut h));
    h.settle(SETTLE);
    assert!(!has_widget(&h, &format!("denoise:remove:{MODEL}")));
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: Speed and Photos choices in the tab are the engine's settings, and Clear cache empties it
#[test]
fn the_settings_tab_changes_where_and_when_denoise_runs() {
    let (mut h, dir, _) = settings("panel-speed");
    assert_eq!(listed(&mut h)["runOn"], "cpu");
    click(&mut h, "button:denoiseRunOn-1");
    assert_eq!(listed(&mut h)["runOn"], "gpu");
    click(&mut h, "button:denoiseRunOn-0");
    assert_eq!(listed(&mut h)["runOn"], "auto");
    h.settle(SETTLE);
    assert!(!painted(&h, "The processor does the work."), "{:?}", h.painted_text());

    let auto = listed(&mut h)["auto"].as_bool().unwrap();
    click(&mut h, "check:settings.denoiseAuto");
    assert_eq!(listed(&mut h)["auto"], !auto);
    click(&mut h, "check:settings.denoiseAuto");
    assert_eq!(listed(&mut h)["auto"], auto);

    click(&mut h, "denoise:clear");
    assert_eq!(h.app.session.execute("denoise.status", &json!({})).unwrap()["cache"]["files"], 0);
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: with pictures not made on their own, a photo with an Amount offers Make it now; a photo whose file is
// gone then fails, says why and offers Try again
#[test]
fn the_detail_line_follows_a_photo_from_waiting_to_failed() {
    let (mut app, dir, raw) = setup("panel-detail");
    install_denoise(&mut app, &dir);
    app.run("denoise.settings", json!({"auto": false})).unwrap();
    app.run("library.select", json!({"ids": [raw.0]})).unwrap();
    // the Amount alone, as a slider sets it: the switch would queue the photo at once
    app.run("develop.set", json!({"ids": [raw.0], "control": "enhance.denoise", "value": 50})).unwrap();
    assert!(app.session.develop_of(raw).unwrap().denoise_amount() > 0.0);
    app.ui.view = crate::state::ViewMode::Detail;
    app.ui.open_sections = vec!["detail".to_string()];
    let mut h = Headless::new(app, [1400.0, 900.0], 1.0);
    h.settle(SETTLE);
    assert_eq!(h.app.session.denoise_photo_state(raw), PhotoState::Idle);
    assert!(has_widget(&h, "denoise:enabled"));
    assert!(has_widget(&h, "denoise:make"), "{:?}", h.painted_text());
    assert!(painted(&h, "pictures are not made on their own"));

    click(&mut h, "denoise:make");
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let failed = loop {
        let _ = h.app.session.execute("denoise.pump", &json!({"pace": "full"}));
        match h.app.session.denoise_photo_state(raw) {
            PhotoState::Failed { why, unsupported } => break (why, unsupported),
            s => assert!(std::time::Instant::now() < deadline, "still {s:?}"),
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(!failed.1, "a missing file is worth trying again: {failed:?}");
    h.settle(SETTLE);
    h.step();
    assert!(has_widget(&h, "denoise:retry"), "{:?}", h.painted_text());
    assert!(painted(&h, crate::panels::denoise::sentence(&failed.0).as_str()));

    click(&mut h, "denoise:retry");
    assert!(
        !matches!(h.app.session.denoise_photo_state(raw), PhotoState::Failed { .. } | PhotoState::Idle),
        "Try again queues it: {:?}",
        h.app.session.denoise_photo_state(raw)
    );
    click(&mut h, "denoise:enabled");
    assert_eq!(h.app.session.develop_of(raw).unwrap().denoise_amount(), 0.0, "the switch turns it off");
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: Download shows the model's terms first and fetches nothing; its page opens in the browser
#[test]
fn download_shows_the_terms_before_anything_is_fetched() {
    let (mut h, dir, _) = settings("panel-download");
    let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let o = opened.clone();
    h.app.services.open_url = Some(Box::new(move |u: &str| {
        o.lock().unwrap().push(u.to_string());
        Ok(())
    }));
    let Some(known) = listed(&mut h)["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["downloadHost"].is_string() && m["installed"] != true)
        .and_then(|m| m["id"].as_str().map(str::to_string))
    else {
        lightcraft_testkit::skip!(Model, "this build offers no model downloads");
        return;
    };
    click(&mut h, &format!("denoise:download:{known}"));
    let Some(Dialog::DenoiseModel { info, accepted: false }) = h.app.ui.dialog.clone() else {
        panic!("Download opens the terms: {:?}", h.app.ui.dialog)
    };
    assert_eq!(info["download"], known.as_str());
    h.settle(SETTLE);
    assert!(painted(&h, "Nothing is fetched until you accept."), "{:?}", h.painted_text());
    assert!(painted(&h, "starts using it for AI Denoise"));
    let downloads = h.app.session.execute("denoise.models.downloads", &json!({})).unwrap();
    assert!(downloads["downloads"].as_array().unwrap().iter().all(|d| d["id"] != known.as_str()), "{downloads}");

    click(&mut h, "denoiseModel:page");
    assert_eq!(opened.lock().unwrap().len(), 1, "the model's page opens");
    assert!(crate::panels::denoise::install(&mut h.app, &info, false).is_err(), "not without accepting");
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: the dialog for a file LightCraft cannot use says why, and one for an unknown model lists what it assumed
#[test]
fn the_model_dialog_explains_unusable_and_unknown_files() {
    let (mut h, dir, _) = settings("panel-dialog");
    h.app.ui.dialog = Some(Dialog::DenoiseModel {
        info: json!({"kind": "unsupported", "fileName": "odd.onnx", "sizeBytes": 2_500_000, "reason": "it takes one input, not two"}),
        accepted: false,
    });
    h.settle(SETTLE);
    assert!(painted(&h, "This file cannot be used yet"));
    assert!(painted(&h, "It takes one input, not two."));
    assert!(painted(&h, "odd.onnx · 2.5 MB"), "{:?}", h.painted_text());

    h.app.ui.dialog = Some(Dialog::DenoiseModel {
        info: json!({
            "kind": "draft",
            "fileName": "mine.onnx",
            "sizeBytes": 40_000_000,
            "path": "/nowhere/mine.onnx",
            "alreadyInstalled": true,
            "model": {
                "name": "My denoiser",
                "licence": {"name": "MIT", "commercial": "yes", "notice": "Copyright the author"},
                "provenance": "my own photos",
            },
            "assumptions": ["4 colour planes in", "3 colour planes out"],
        }),
        accepted: false,
    });
    h.settle(SETTLE);
    for text in [
        "My denoiser",
        "40 MB",
        "Already installed.",
        "MIT · commercial use allowed",
        "Copyright the author",
        "Trained on: my own photos",
        "LightCraft does not know this model, so it assumed:",
        "•  4 colour planes in",
        "The model is kept on this computer only.",
    ] {
        assert!(painted(&h, text), "{text}: {:?}", h.painted_text());
    }
    assert!(!has_widget(&h, "denoiseModel:page"), "no source, no link");
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}
