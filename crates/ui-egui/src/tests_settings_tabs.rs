//! Settings tabs as the user meets them, in a library on disk: Import (camera defaults, metadata preset, XMP
//! sidecars, cards, Auto Import), Performance (the smart previews folder) and Slideshow (fade and playback choices).
use std::path::PathBuf;
use std::time::Duration;

use lightcraft_catalog::{MediaKind, Op, Photo, PhotoId, Source};
use serde_json::{Value, json};

use crate::headless::Headless;
use crate::state::Dialog;
use crate::{LightcraftApp, Services};

const T: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(120);

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lc-ui-settings-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Settings open on `tab`, in a new library in `dir`; a folder picker answers with `picked`.
fn settings(dir: &std::path::Path, tab: &str, picked: PathBuf) -> Headless {
    let mut session = lightcraft_engine::Session::new().with_fs();
    session.open_library(dir.join("library"), true).unwrap();
    let services = Services { png: None, pick_folder: Some(Box::new(move || Some(picked.to_string_lossy().into_owned()))), ..Default::default() };
    let mut app = LightcraftApp::new(session, services);
    app.ui.dialog = Some(Dialog::Settings { tab: tab.into() });
    let mut h = Headless::new(app, [1300.0, 1400.0], 1.0);
    h.settle(SETTLE);
    h
}

fn ask(h: &mut Headless, method: &str, params: Value) {
    let r = h.request(method, params.clone(), T);
    assert_eq!(r["ok"], true, "{method} {params}: {r}");
    h.step();
    h.step();
}

fn click(h: &mut Headless, id: &str) {
    ask(h, "ui.clickWidget", json!({"id": id}));
}

fn click_text(h: &mut Headless, text: &str) {
    let at = h.text_center(text).unwrap_or_else(|| panic!("no text {text:?} painted: {:?}", h.painted_text()));
    ask(h, "ui.click", json!({"x": at.x, "y": at.y}));
}

fn painted(h: &Headless, text: &str) -> bool {
    h.painted_text().iter().any(|s| s.contains(text))
}

fn toast(h: &Headless) -> String {
    h.app.ui.toast.as_ref().map(|t| t.0.to_string()).unwrap_or_default()
}

// Scenario: with camera-specific defaults on, a camera in the library gets its own row, set to the LightCraft
// default and back to the raw default
#[test]
fn a_camera_in_the_library_gets_its_own_default() {
    let dir = temp("camera");
    let mut h = settings(&dir, "import", dir.clone());
    let id = PhotoId(h.app.session.catalog.photos().map(|p| p.id.0).max().unwrap_or(0) + 1);
    let mut raw = Photo::new(id, Source::File { path: dir.join("a.cr3").to_string_lossy().into_owned() }, "a.cr3", "CR3", 60, 40, "2026-10-01");
    raw.kind = MediaKind::Raw;
    raw.meta.camera = "Canon EOS R5".into();
    h.app.session.commit("fixture", Op::AddPhoto { photo: Box::new(raw) }).unwrap();
    h.settle(SETTLE);
    if !h.app.session.prefs.import_defaults.per_camera {
        click(&mut h, "check:settings.perCamera");
        h.settle(SETTLE);
    }
    assert!(h.app.session.prefs.import_defaults.per_camera);
    assert!(painted(&h, "Canon EOS R5"), "{:?}", h.painted_text());

    click(&mut h, "combo:settingsCamera-0");
    click_text(&mut h, "LightCraft Default");
    let cams = &h.app.session.prefs.import_defaults.cameras;
    assert!(cams.iter().any(|c| c.camera == "Canon EOS R5" && c.preset.is_none()), "{cams:?}");
    h.settle(SETTLE);

    click(&mut h, "combo:settingsCamera-0");
    click_text(&mut h, "Same as raw default");
    assert!(h.app.session.prefs.import_defaults.cameras.is_empty(), "{:?}", h.app.session.prefs.import_defaults.cameras);
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: a saved metadata preset is offered for imports; XMP and card choices are the library's preferences
#[test]
fn metadata_preset_xmp_and_card_choices_are_kept() {
    let dir = temp("import");
    let mut h = settings(&dir, "import", dir.clone());
    h.app.session.execute("metadata.savePreset", &json!({"name": "Studio", "fields": {"creator": "J. Doe"}})).unwrap();
    h.settle(SETTLE);
    click(&mut h, "combo:settingsMetaPreset");
    click_text(&mut h, "Studio");
    assert_eq!(h.app.session.prefs.import_defaults.metadata_preset.as_deref(), Some("Studio"));

    let auto = h.app.session.prefs.xmp.auto_write;
    click(&mut h, "check:settings.autoWriteXmp");
    assert_eq!(h.app.session.prefs.xmp.auto_write, !auto);
    click(&mut h, "button:settingsXmpNaming-1");
    assert_eq!(h.app.session.prefs.xmp.naming, lightcraft_engine::sidecar::SidecarNaming::Full);
    click(&mut h, "button:settingsXmpNaming-0");
    assert_eq!(h.app.session.prefs.xmp.naming, lightcraft_engine::sidecar::SidecarNaming::Stem);

    let on_card = h.app.session.prefs.import_defaults.show_on_card;
    click(&mut h, "check:settings.importOnCard");
    assert_eq!(h.app.session.prefs.import_defaults.show_on_card, !on_card);
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: Auto Import watches the chosen folder, can copy into the library, and Turn Off stops it
#[test]
fn auto_import_watches_a_chosen_folder_until_turned_off() {
    let dir = temp("auto");
    let watched = dir.join("tether");
    std::fs::create_dir_all(&watched).unwrap();
    let mut h = settings(&dir, "import", watched.clone());
    click(&mut h, "button:settingsAutoFolder");
    assert_eq!(h.app.session.prefs.import_defaults.auto_folder.as_deref(), Some(watched.to_string_lossy().as_ref()));
    h.settle(SETTLE);
    let copy = h.app.session.prefs.import_defaults.auto_copy;
    click_text(&mut h, "Copy into the library (else use the files where they are)");
    assert_eq!(h.app.session.prefs.import_defaults.auto_copy, !copy);
    click_text(&mut h, "Turn Off");
    assert!(h.app.session.prefs.import_defaults.auto_folder.is_none());
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: smart previews move to a chosen folder and back to the library's, saying so
#[test]
fn the_smart_previews_folder_moves_and_comes_back() {
    let dir = temp("smart");
    let elsewhere = dir.join("proxies");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let mut h = settings(&dir, "performance", elsewhere.clone());
    assert!(painted(&h, "(library default)"), "{:?}", h.painted_text());
    click(&mut h, "button:settingsSmartChoose");
    assert!(toast(&h).starts_with("Smart previews folder changed"), "{}", toast(&h));
    let loc = h.app.run("library.smartPreviewsLocation", json!({})).unwrap();
    assert_eq!(loc["custom"], true, "{loc}");
    h.settle(SETTLE);
    h.app.ui.toast = None;
    click(&mut h, "button:settingsSmartReset");
    assert!(toast(&h).starts_with("Smart previews folder changed"), "{}", toast(&h));
    assert_eq!(h.app.run("library.smartPreviewsLocation", json!({})).unwrap()["custom"], false);
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

// Scenario: the slideshow's fade goes through black, white or a colour and back to a crossfade; repeat and
// preparing previews are switched
#[test]
fn slideshow_fade_and_playback_choices_are_kept() {
    let dir = temp("slideshow");
    let mut h = settings(&dir, "slideshow", dir.clone());
    let fade = |h: &Headless| h.app.session.prefs.slideshow.fade_color;
    click(&mut h, "button:settingsSlideshowFade-1");
    assert_eq!(fade(&h), Some([0, 0, 0]));
    click(&mut h, "button:settingsSlideshowFade-2");
    assert_eq!(fade(&h), Some([255, 255, 255]));
    click(&mut h, "button:settingsSlideshowFade-3");
    assert_eq!(fade(&h), Some([64, 64, 64]));
    h.settle(SETTLE);
    assert!(h.app.widgets.iter().any(|(w, _)| w == "button:slideshowFadeColor"), "a colour has its swatch");
    click(&mut h, "button:settingsSlideshowFade-0");
    assert_eq!(fade(&h), None);

    let s = h.app.session.prefs.slideshow.clone();
    click(&mut h, "check:settings.slideshowRepeat");
    click(&mut h, "check:settings.slideshowPrepare");
    assert_eq!(h.app.session.prefs.slideshow.repeat, !s.repeat);
    assert_eq!(h.app.session.prefs.slideshow.prepare_previews, !s.prepare_previews);
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}
