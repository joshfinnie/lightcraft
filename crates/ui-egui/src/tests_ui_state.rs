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
        keywordingView language layoutImage leftPanel leftWidth localRoots maskOverlay maskOverlayAll maskOverlayColor \
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
