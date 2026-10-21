use crate::state::UiState;

/// The settings file's UiState keys. Grouping fields in Rust must not move or rename them, and a
/// rename or AI mask still open when the app quits is not saved.
#[test]
fn ui_state_json_keeps_its_keys() {
    let mut ui = UiState::default();
    ui.mask_edit.renaming_component = Some((1, 0, "Sky".into()));
    ui.mask_edit.renaming_mask = Some((1, "Sky".into()));
    let v = serde_json::to_value(&ui).unwrap();
    let mut keys: Vec<_> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    let want = "autoAdvance beforeAfter brushAutoMask brushErase brushFeather brushFlow brushPressure brushSize clickZoom \
        cropDefaultAspect cropOverlay cropOverlayOrient curveChannel docking eye eyePet faceBoxes filmstrip filterBar \
        foldedSections fullscreen gradingMode gridInfo hdrVisualize hiddenLocations histogram infoOverlay keywordListOpen \
        keywordingView layoutImage leftPanel leftWidth localRoots maskOverlay maskOverlayAll maskOverlayColor \
        maskOverlayMode maskOverlayOpacity maskPins mixerMode navigator openFlyouts openSections pan pointColor \
        pointColorVisualize presetThumbs presets proof proofPaperInk removeFeather removeOpacity removeSize right \
        rightWidth search secondWindow settings showClipping showCounts showFilenames sidebar singlePanel softProof \
        spotsThreshold thumbSize tool toolOverlay view visualizeSpots zoom";
    assert_eq!(keys, want.split_whitespace().collect::<Vec<_>>());
}

/// Settings saved before the component rename stopped being saved still read, without the rename.
#[test]
fn an_old_saved_component_rename_is_ignored() {
    let ui: UiState = serde_json::from_str(r#"{"renamingComponent": [1, 0, "Sky"], "leftWidth": 300.0}"#).unwrap();
    assert!(ui.mask_edit.renaming_component.is_none());
    assert_eq!(ui.left_width, 300.0);
}

/// `ui.set` changes the settings it names and nothing else: what is never saved (lights out, the
/// People page, a compare pair, a soft-proofing profile) stays as it was.
#[test]
fn ui_set_keeps_what_is_not_saved() {
    use crate::headless::Headless;
    use crate::state::LightsOut;
    let services = crate::Services { png: None, ..Default::default() };
    let app = crate::LightcraftApp::new(lightcraft_engine::Session::with_demo(), services);
    let mut h = Headless::new(app, [1200.0, 800.0], 1.0);
    let t = std::time::Duration::from_secs(20);
    let r = h.request("engine.execute", serde_json::json!({"command": "view.lightsOut", "params": {"mode": "off"}}), t);
    assert_eq!(r["ok"], true, "{r}");
    h.app.ui.people.person_page = Some("Ann".into());
    h.app.ui.compare = Some((1, 2));
    h.app.ui.proof_profile = Some(("paper.icc".into(), "Paper".into()));
    let r = h.request("ui.set", serde_json::json!({"thumbSize": 200}), t);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.ui.thumb_size, 200.0);
    assert_eq!(h.app.ui.lights_out, LightsOut::Off);
    assert_eq!(h.app.ui.people.person_page.as_deref(), Some("Ann"));
    assert_eq!(h.app.ui.compare, Some((1, 2)));
    assert_eq!(h.app.ui.proof_profile, Some(("paper.icc".into(), "Paper".into())));
}

/// The brush, Remove brush and mask overlay groups read back from the keys they save under.
#[test]
fn grouped_settings_read_back() {
    let ui: UiState =
        serde_json::from_str(r#"{"brushSize": 0.1, "brushPressure": false, "removeOpacity": 40.0, "maskOverlayMode": "white", "maskPins": false}"#)
            .unwrap();
    assert_eq!((ui.brush.size, ui.brush.pressure, ui.brush.flow), (0.1, false, 60.0));
    assert_eq!((ui.remove_brush.opacity, ui.remove_brush.size), (40.0, 0.02));
    assert_eq!((ui.overlay.mode.as_str(), ui.overlay.pins, ui.overlay.on), ("white", false, true));
    let back: UiState = serde_json::from_value(serde_json::to_value(&ui).unwrap()).unwrap();
    assert_eq!((back.brush.size, back.remove_brush.opacity, back.overlay.mode.as_str()), (0.1, 40.0, "white"));
}
