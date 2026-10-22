//! The Edit panel's buttons, menus and flyouts as the user meets them: the header's Auto / B&W / HDR, the profile
//! menu, white balance and vignette choices, the mixer and the Optics section, and the curve's menus.
use std::time::Duration;

use lightcraft_develop::{DevelopSettings, VignetteStyle, WbMode};
use serde_json::{Value, json};

use crate::headless::Headless;
use crate::state::RightPanel;
use crate::{LightcraftApp, Services};

const T: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(120);

/// Detail view of a demo photo with the Edit panel open, showing `sections` and `flyouts` only. Tall, so that what
/// they hold is on screen without scrolling.
fn edit(sections: &[&str], flyouts: &[&str]) -> Headless {
    let services = Services { png: None, ..Default::default() };
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    let mut h = Headless::new(app, [1300.0, 1800.0], 1.0);
    let r = h.request("ui.set", json!({"view": "detail"}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.app.ui.right = RightPanel::Edit;
    h.app.ui.open_sections = sections.iter().map(|s| s.to_string()).collect();
    h.app.ui.open_flyouts = flyouts.iter().map(|s| s.to_string()).collect();
    h.settle(SETTLE);
    h
}

fn click_with(h: &mut Headless, id: &str, extra: Value) {
    let mut params = json!({"id": id});
    if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    let r = h.request("ui.clickWidget", params, T);
    assert_eq!(r["ok"], true, "{id}: {r}");
    h.step();
    h.step();
}

fn click(h: &mut Headless, id: &str) {
    click_with(h, id, json!({}));
}

/// Clicks the last piece of text painted as exactly `text` (a popup is painted over the panel, so after it).
fn click_text(h: &mut Headless, text: &str) {
    let at = h.text_center(text).unwrap_or_else(|| panic!("no text {text:?} painted: {:?}", h.painted_text()));
    let r = h.request("ui.click", json!({"x": at.x, "y": at.y}), T);
    assert_eq!(r["ok"], true, "{text}: {r}");
    h.step();
    h.step();
}

fn has_widget(h: &Headless, id: &str) -> bool {
    h.app.widgets.iter().any(|(w, _)| w == id)
}

fn develop(h: &Headless) -> DevelopSettings {
    let id = h.app.session.active().expect("active photo");
    (*h.app.session.develop_of(id).unwrap_or_default()).clone()
}

// Scenario: Auto applies settings and says so; B&W and HDR switch on and off from the header, and HDR adds the SDR
// Rendition flyout to Light
#[test]
fn the_header_applies_auto_and_switches_bw_and_hdr() {
    let mut h = edit(&["light"], &[]);
    click(&mut h, "button:auto");
    assert!(h.app.ui.toast.as_ref().is_some_and(|t| t.0.contains("Auto settings applied")), "{:?}", h.app.ui.toast);

    click(&mut h, "button:bw");
    assert!(crate::is_bw(&develop(&h)));
    h.settle(SETTLE);
    click(&mut h, "button:bw");
    assert!(!crate::is_bw(&develop(&h)));

    assert!(!has_widget(&h, "flyout:sdrRendition"));
    click(&mut h, "button:hdr");
    assert!(develop(&h).hdr.enabled);
    h.settle(SETTLE);
    click(&mut h, "flyout:sdrRendition");
    assert!(h.app.ui.flyout_open("sdrRendition"));
    h.settle(SETTLE);
    assert!(h.painted_text().iter().any(|s| s == "SDR Rendition"));
    click(&mut h, "button:hdr");
    assert!(!develop(&h).hdr.enabled);
}

// Scenario: with more than one photo selected and Auto Sync on, the panel says edits apply to all of them and turns
// Auto Sync off
#[test]
fn the_auto_sync_line_turns_it_off() {
    let mut h = edit(&["light"], &[]);
    let ids: Vec<u64> = h.app.session.visible().iter().take(3).map(|p| p.0).collect();
    h.app.session.execute("library.select", &json!({"ids": ids, "active": ids[0]})).unwrap();
    h.app.session.execute("develop.autoSync", &json!({"on": true})).unwrap();
    h.settle(SETTLE);
    assert!(h.painted_text().iter().any(|s| s == "Auto Sync: edits apply to 3 photos"), "{:?}", h.painted_text());
    click(&mut h, "button:autoSyncOff");
    assert!(!h.app.session.auto_sync);
}

// Scenario: the profile menu marks a favourite, lists it under Favorites, previews a profile on hover, applies one
// from its group, and Browse… opens the profile browser
#[test]
fn the_profile_menu_favourites_previews_and_applies_a_profile() {
    let mut h = edit(&["light"], &[]);
    let before = develop(&h).profile.id;
    click(&mut h, "dropdown:profile");
    assert!(has_widget(&h, "profileMenu:browse"));
    click(&mut h, "profileMenu:toggleFavorite");
    assert!(h.app.session.profile_favorites.contains(&before), "{:?}", h.app.session.profile_favorites);

    h.settle(SETTLE);
    if !has_widget(&h, "profileMenu:browse") {
        click(&mut h, "dropdown:profile");
    }
    assert!(h.painted_text().iter().any(|s| s == "Favorites"), "{:?}", h.painted_text());
    assert!(h.painted_text().iter().any(|s| s.starts_with("Remove “")), "{:?}", h.painted_text());

    let group =
        h.app.widgets.iter().find_map(|(w, _)| w.strip_prefix("profileMenu:groupMenu:").map(str::to_string)).expect("a profile group submenu");
    let r = h.request("ui.hoverWidget", json!({"id": format!("profileMenu:groupMenu:{group}")}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.step();
    if !h.app.widgets.iter().any(|(w, _)| w.starts_with("profileMenu:group:")) {
        click(&mut h, &format!("profileMenu:groupMenu:{group}"));
    }
    let other = h
        .app
        .widgets
        .iter()
        .find_map(|(w, _)| w.strip_prefix("profileMenu:group:").filter(|id| *id != before).map(str::to_string))
        .expect("another profile in the group");
    let r = h.request("ui.hoverWidget", json!({"id": format!("profileMenu:group:{other}")}), T);
    assert_eq!(r["ok"], true, "{r}");
    h.step();
    h.step();
    let preview = h.app.hover_preview.as_ref().expect("hovering a profile previews it");
    assert_eq!(preview.settings.profile.id, other);
    assert!(preview.label.starts_with("Profile: "), "{}", preview.label);

    click(&mut h, &format!("profileMenu:group:{other}"));
    assert_eq!(develop(&h).profile.id, other);

    h.settle(SETTLE);
    click(&mut h, "dropdown:profile");
    click(&mut h, "profileMenu:browse");
    assert_eq!(h.app.ui.right, RightPanel::Profiles);
}

// Scenario: White Balance's menu sets a preset, and the vignette's style buttons choose its style
#[test]
fn white_balance_and_vignette_style_are_chosen_from_the_panel() {
    let mut h = edit(&["color", "effects"], &[]);
    click(&mut h, "dropdown:wbMode");
    click_text(&mut h, "Daylight");
    assert_eq!(develop(&h).wb.mode, WbMode::Daylight);

    click(&mut h, "button:vignetteStyle-paintOverlay");
    assert_eq!(develop(&h).vignette.style, VignetteStyle::PaintOverlay);
    click(&mut h, "button:vignetteStyle-colorPriority");
    assert_eq!(develop(&h).vignette.style, VignetteStyle::ColorPriority);
}

// Scenario: the mixer's colour dots choose the band its sliders edit; Point Color and Defringe open from their rows
#[test]
fn the_mixer_point_color_and_defringe_flyouts_open() {
    let mut h = edit(&["color", "optics"], &["mixer"]);
    let band = lightcraft_develop::MIXER_BANDS[2];
    click(&mut h, &format!("mixerBand:{band}"));
    assert_eq!(h.app.ui.mixer_mode, band);

    click(&mut h, "flyout:pointColor");
    assert!(h.app.ui.flyout_open("pointColor"));
    click(&mut h, "flyout:defringe");
    assert!(h.app.ui.flyout_open("defringe"));
    h.settle(SETTLE);
    assert!(h.painted_text().iter().any(|s| s == "Lateral Chromatic Aberration"), "{:?}", h.painted_text());

    // B&W has a B&W Mixer and no Point Color
    click(&mut h, "button:bw");
    h.settle(SETTLE);
    assert!(h.painted_text().iter().any(|s| s == "B&W Mixer"));
    assert!(!has_widget(&h, "flyout:pointColor"));
}

// Scenario: Optics' checkboxes switch chromatic aberration removal and lens corrections, which say when the file has
// no lens data
#[test]
fn optics_checkboxes_switch_ca_removal_and_lens_corrections() {
    let mut h = edit(&["optics"], &[]);
    let before = develop(&h).optics;
    click_text(&mut h, "Remove Chromatic Aberration");
    assert_eq!(develop(&h).optics.remove_ca, !before.remove_ca);
    click_text(&mut h, "Enable Lens Corrections");
    let lens = develop(&h).optics.lens_profile;
    assert_eq!(lens, !before.lens_profile);
    h.settle(SETTLE);
    if lens {
        assert!(h.painted_text().iter().any(|s| s == "No lens data embedded in this file."), "{:?}", h.painted_text());
    }
}

// Scenario: right-clicking the curve offers to reset its channel or all curves; the parametric curve shows its
// region sliders
#[test]
fn the_curve_menu_resets_a_channel_or_all_curves() {
    let mut h = edit(&["light"], &["curve"]);
    h.app.ui.curve_channel = "red".into();
    let mut curve = develop(&h).curve;
    curve.highlights = 40.0;
    curve.red = serde_json::from_value(json!([{"x": 0.0, "y": 0.0}, {"x": 0.5, "y": 0.7}, {"x": 1.0, "y": 1.0}])).unwrap();
    h.app.session.execute("develop.merge", &json!({"settings": {"curve": curve}})).unwrap();
    h.settle(SETTLE);
    click_with(&mut h, "curve", json!({"button": "right"}));
    assert!(h.painted_text().iter().any(|s| s == "Reset Red Channel"), "{:?}", h.painted_text());
    click(&mut h, "curveMenu:resetChannel");
    let d = develop(&h);
    assert_eq!(d.curve.red, DevelopSettings::default().curve.red);
    assert_eq!(d.curve.highlights, 40.0, "only the red channel");

    h.app.ui.curve_channel = "parametric".into();
    h.settle(SETTLE);
    click_with(&mut h, "curve", json!({"button": "right"}));
    assert!(h.painted_text().iter().any(|s| s == "Reset Parametric Curve"), "{:?}", h.painted_text());
    click(&mut h, "curveMenu:resetAll");
    assert_eq!(develop(&h).curve, DevelopSettings::default().curve);
}
