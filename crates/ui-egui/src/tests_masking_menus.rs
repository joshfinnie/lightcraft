//! The Masking panel's tiles, menus and settings as the user meets them: new masks of each kind, a mask's and a
//! component's right-click menus, renaming in place, the brush settings and the overlay options.
use std::time::Duration;

use lightcraft_develop::{Mask, MaskOp, MaskShape};
use serde_json::{Value, json};

use crate::headless::Headless;
use crate::{LightcraftApp, Services};

const T: Duration = Duration::from_secs(20);
const SETTLE: Duration = Duration::from_secs(120);

/// Detail view of a demo photo with the Masking panel open, tall enough to show a mask's whole panel.
fn masking() -> Headless {
    let services = Services { png: None, ..Default::default() };
    let app = LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    let mut h = Headless::new(app, [1300.0, 1800.0], 1.0);
    for (method, params) in [("ui.set", json!({"view": "detail"})), ("engine.execute", json!({"command": "panel.masking"}))] {
        let r = h.request(method, params, T);
        assert_eq!(r["ok"], true, "{r}");
    }
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

fn right_click(h: &mut Headless, id: &str) {
    ask(h, "ui.clickWidget", json!({"id": id, "button": "right"}));
}

fn click_text(h: &mut Headless, text: &str) {
    let at = h.text_center(text).unwrap_or_else(|| panic!("no text {text:?} painted: {:?}", h.painted_text()));
    ask(h, "ui.click", json!({"x": at.x, "y": at.y}));
}

fn masks(h: &Headless) -> Vec<Mask> {
    let id = h.app.session.active().expect("active photo");
    h.app.session.develop_of(id).unwrap_or_default().masks.clone()
}

fn active(h: &Headless) -> Mask {
    let id = h.app.session.active_mask.expect("a selected mask");
    masks(h).into_iter().find(|m| m.id == id).expect("the selected mask")
}

fn add(h: &mut Headless, kind: &str) -> Mask {
    click(h, &format!("maskNew:{kind}"));
    h.settle(SETTLE);
    active(h)
}

// Scenario: each tile adds its kind of mask; the gradients and Color pick up their tool, and Color says how to sample
#[test]
fn the_new_mask_tiles_add_each_kind_and_pick_up_its_tool() {
    let mut h = masking();
    assert!(h.painted_text().iter().any(|s| s == "No masks yet. Choose a mask type above."));
    let m = add(&mut h, "linear");
    assert!(matches!(m.components[0].shape, MaskShape::Linear { .. }), "{:?}", m.components[0].shape);
    assert_eq!(h.app.ui.tool, "linear");
    let m = add(&mut h, "radial");
    assert!(matches!(m.components[0].shape, MaskShape::Radial { .. }), "{:?}", m.components[0].shape);
    assert_eq!(h.app.ui.tool, "radial");
    let m = add(&mut h, "luminanceRange");
    assert!(matches!(m.components[0].shape, MaskShape::LuminanceRange { .. }), "{:?}", m.components[0].shape);
    assert!(h.app.widgets.iter().any(|(w, _)| w.starts_with("lumRange:")), "the range's controls show");
    let m = add(&mut h, "colorRange");
    assert!(matches!(m.components[0].shape, MaskShape::ColorRange { .. }), "{:?}", m.components[0].shape);
    assert_eq!(h.app.ui.tool, "colorRange");
    assert!(h.app.ui.toast.as_ref().is_some_and(|t| t.0.contains("Click the photo to pick a colour")), "{:?}", h.app.ui.toast);
    assert_eq!(masks(&h).len(), 4);
}

// Scenario: a mask's right-click menu duplicates, inverts, hides, moves and deletes it
#[test]
fn the_mask_menu_duplicates_inverts_hides_moves_and_deletes() {
    let mut h = masking();
    let first = add(&mut h, "linear");
    right_click(&mut h, &format!("mask:{}", first.id));
    click_text(&mut h, "Duplicate Mask");
    assert_eq!(masks(&h).len(), 2);
    h.settle(SETTLE);

    right_click(&mut h, &format!("mask:{}", first.id));
    click_text(&mut h, "Invert Mask");
    assert!(masks(&h).iter().find(|m| m.id == first.id).unwrap().invert);
    h.settle(SETTLE);

    right_click(&mut h, &format!("mask:{}", first.id));
    click_text(&mut h, "Hide Mask");
    assert!(!masks(&h).iter().find(|m| m.id == first.id).unwrap().visible);
    h.settle(SETTLE);

    right_click(&mut h, &format!("mask:{}", first.id));
    click_text(&mut h, "Move Down");
    assert_eq!(masks(&h).last().unwrap().id, first.id);
    h.settle(SETTLE);

    right_click(&mut h, &format!("mask:{}", first.id));
    click_text(&mut h, "Delete Mask");
    assert!(masks(&h).iter().all(|m| m.id != first.id));
    assert_eq!(masks(&h).len(), 1);
}

// Scenario: Rename… in a mask's menu, and a double-click on a component, edit the name in place; Enter keeps it
#[test]
fn masks_and_components_are_renamed_in_place() {
    let mut h = masking();
    let m = add(&mut h, "radial");
    right_click(&mut h, &format!("mask:{}", m.id));
    click_text(&mut h, "Rename…");
    assert!(h.app.ui.mask_edit.renaming_mask.is_some());
    h.settle(SETTLE);
    assert!(h.app.widgets.iter().any(|(w, _)| *w == format!("maskRename:{}", m.id)));
    ask(&mut h, "ui.key", json!({"key": "a", "cmd": true}));
    ask(&mut h, "ui.text", json!({"text": "Face light"}));
    ask(&mut h, "ui.key", json!({"key": "Enter"}));
    assert_eq!(active(&h).name, "Face light");

    h.settle(SETTLE);
    ask(&mut h, "ui.clickWidget", json!({"id": "component:0", "count": 2}));
    assert!(h.app.ui.mask_edit.renaming_component.is_some());
    h.settle(SETTLE);
    ask(&mut h, "ui.key", json!({"key": "a", "cmd": true}));
    ask(&mut h, "ui.text", json!({"text": "Glow"}));
    ask(&mut h, "ui.key", json!({"key": "Enter"}));
    assert_eq!(active(&h).components[0].name.as_deref(), Some("Glow"));
}

// Scenario: a component's … menu inverts and duplicates it, sets a later one's mode, and deleting the last
// component deletes the mask
#[test]
fn the_component_menu_inverts_duplicates_changes_mode_and_deletes() {
    let mut h = masking();
    add(&mut h, "linear");
    click(&mut h, "button:componentMenu0");
    click_text(&mut h, "Invert");
    assert!(active(&h).components[0].invert);
    h.settle(SETTLE);

    click(&mut h, "button:componentMenu0");
    click_text(&mut h, "Duplicate \"Linear Gradient\"");
    assert_eq!(active(&h).components.len(), 2);
    h.settle(SETTLE);

    click(&mut h, "button:componentMenu1");
    click_text(&mut h, "Mode");
    click_text(&mut h, "Subtract");
    assert_eq!(active(&h).components[1].op, MaskOp::Subtract);
    h.settle(SETTLE);
    assert!(h.painted_text().iter().any(|s| s.starts_with("− ")), "{:?}", h.painted_text());

    click(&mut h, "button:componentMenu1");
    click_text(&mut h, "Delete \"Linear Gradient\"");
    assert_eq!(active(&h).components.len(), 1);
    h.settle(SETTLE);
    click(&mut h, "button:componentMenu0");
    click_text(&mut h, "Delete \"Linear Gradient\" (and the mask)");
    assert!(masks(&h).is_empty());
}

// Scenario: the Add / Subtract buttons under a mask add components of a kind, and Invert and the bin act on the mask
#[test]
fn the_buttons_under_a_mask_add_components_invert_and_delete() {
    let mut h = masking();
    add(&mut h, "radial");
    click(&mut h, "button:maskAddComp");
    click(&mut h, "maskComp:add:linear");
    assert_eq!(active(&h).components.len(), 2);
    h.settle(SETTLE);
    click(&mut h, "button:maskSubComp");
    click(&mut h, "maskComp:subtract:luminanceRange");
    let m = active(&h);
    assert_eq!(m.components.len(), 3);
    assert_eq!(m.components[2].op, MaskOp::Subtract);
    h.settle(SETTLE);
    click(&mut h, "button:maskInvert");
    assert!(active(&h).invert);
    click(&mut h, "icon:maskDelete");
    assert!(masks(&h).is_empty());
}

// Scenario: the brush's settings switch between adding and erasing, auto mask and pen pressure
#[test]
fn the_brush_settings_switch_erase_auto_mask_and_pressure() {
    let mut h = masking();
    add(&mut h, "brush");
    assert_eq!(h.app.ui.tool, "brush");
    click(&mut h, "button:brushErase");
    assert!(h.app.ui.brush.erase);
    click(&mut h, "button:brushAdd");
    assert!(!h.app.ui.brush.erase);
    let (auto, pressure) = (h.app.ui.brush.auto_mask, h.app.ui.brush.pressure);
    click_text(&mut h, "Auto Mask");
    assert_eq!(h.app.ui.brush.auto_mask, !auto);
    click_text(&mut h, "Pen Pressure");
    assert_eq!(h.app.ui.brush.pressure, !pressure);
}

// Scenario: the overlay buttons and options: show it, show all, its mode, colour and pins
#[test]
fn the_overlay_options_set_mode_colour_and_pins() {
    let mut h = masking();
    add(&mut h, "radial");
    let (on, all) = (h.app.ui.overlay.on, h.app.ui.overlay.all);
    click(&mut h, "icon:maskOverlay");
    assert_eq!(h.app.ui.overlay.on, !on);
    click(&mut h, "icon:maskOverlayAll");
    assert_eq!(h.app.ui.overlay.all, !all);

    click(&mut h, "dropdown:maskOverlayMode");
    click_text(&mut h, "Image on Black");
    assert_eq!(h.app.ui.overlay.mode, "imageOnBlack");
    h.settle(SETTLE);
    assert!(!h.app.widgets.iter().any(|(w, _)| w.starts_with("maskOverlayColor:")), "no colour for an image overlay");
    click(&mut h, "dropdown:maskOverlayMode");
    click_text(&mut h, "Color Overlay");
    assert_eq!(h.app.ui.overlay.mode, "color");
    h.settle(SETTLE);
    click(&mut h, "maskOverlayColor:28c846");
    assert_eq!(h.app.ui.overlay.color, [40, 200, 70]);

    let pins = h.app.ui.overlay.pins;
    click_text(&mut h, "Show Pins");
    assert_eq!(h.app.ui.overlay.pins, !pins);
}
