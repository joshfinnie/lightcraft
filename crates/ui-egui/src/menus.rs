//! UI-level commands (views, panels, zoom, tools, dialogs) and the menu model shared by the native
//! menu bar, the shortcut handler and the control channel.

use serde::Serialize;
use serde_json::{Value, json};

use crate::LightcraftApp;
use crate::state::{Dialog, RightPanel, ViewMode};
use lightcraft_engine::cmd::params::NoParams;

/// A UI command: its menu entry and shortcut, the params it takes and how it runs. The menu bar,
/// the shortcuts, the control channel and `engine.commands` all read [`ui_commands`].
pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    /// Default shortcut (`Cmd+Shift+D`), mapped per platform.
    pub shortcut: Option<&'static str>,
    /// Menu path (`Window>Tools`); empty = not in menus.
    pub menu: &'static str,
    /// The top-level keys `run` takes; `None` for any (a command not yet on typed params).
    pub keys: fn() -> Option<&'static [&'static str]>,
    pub run: Run,
}

/// A call of a UI command: its id (one function can serve several) and its params as sent (a
/// command that asks for a file runs again with the answer added to them).
pub struct Call<'a> {
    pub id: &'a str,
    pub params: &'a Value,
}

/// How a UI command runs.
pub type Run = fn(&mut LightcraftApp, &Call) -> Result<Value, String>;

/// A UI command. `ui!(id, label, shortcut, menu, Params, run)` reads its params into `Params` (a
/// `lightcraft_engine::params!` struct: an unknown key or a bad value is an error before `run`);
/// `ui!(…, raw run)` is one still reading raw JSON.
macro_rules! ui {
    ($id:literal, $label:expr, $sc:expr, $menu:literal, raw $run:path) => {
        UiCommand { id: $id, label: $label, shortcut: $sc, menu: $menu, keys: || None, run: $run }
    };
    ($id:literal, $label:expr, $sc:expr, $menu:literal, $ty:ty, $run:expr) => {
        UiCommand {
            id: $id,
            label: $label,
            shortcut: $sc,
            menu: $menu,
            keys: lightcraft_engine::cmd::params::keys_of::<$ty>,
            run: {
                const RUN: fn(&mut LightcraftApp, &Call, $ty) -> Result<Value, String> = $run;
                |app, c| RUN(app, c, lightcraft_engine::cmd::params::parse_as::<$ty>(c.id, c.params).map_err(|e| e.to_string())?)
            },
        }
    };
}

/// The "Edit → Language" entries, one per language in [`crate::i18n::Locale::ALL`], so a language
/// has a matching menu command (also listed through the control channel). Tests check coverage.
pub const LANGUAGE_COMMANDS: &[UiCommand] = &[
    ui!("app.language.english", crate::i18n::Locale::En.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.simplifiedChinese", crate::i18n::Locale::ZhHans.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.traditionalChinese", crate::i18n::Locale::ZhHant.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.japanese", crate::i18n::Locale::Ja.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.portuguese", crate::i18n::Locale::PtBr.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.french", crate::i18n::Locale::Fr.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.spanish", crate::i18n::Locale::Es.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.german", crate::i18n::Locale::De.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.russian", crate::i18n::Locale::Ru.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.ukrainian", crate::i18n::Locale::Uk.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.polish", crate::i18n::Locale::Pl.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.czech", crate::i18n::Locale::Cs.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.italian", crate::i18n::Locale::It.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
    ui!("app.language.georgian", crate::i18n::Locale::Ka.name(), None, "Edit>Language", NoParams, crate::commands::app::language),
];

/// Every UI command: the languages, then everything else. `xtask parity` reads both tables from
/// this file, so an id listed in `docs/parity-checklist.md` is checked wherever it is declared.
pub fn ui_commands() -> impl Iterator<Item = &'static UiCommand> {
    LANGUAGE_COMMANDS.iter().chain(UI_COMMANDS)
}

/// The language a Language-menu command selects, if the id is one. The engine and the UI both go
/// through here, so the menu, the settings row and the control channel agree on the mapping.
pub fn language_from_command(id: &str) -> Option<crate::i18n::Locale> {
    match id {
        "app.language.english" => Some(crate::i18n::Locale::En),
        "app.language.simplifiedChinese" => Some(crate::i18n::Locale::ZhHans),
        "app.language.traditionalChinese" => Some(crate::i18n::Locale::ZhHant),
        "app.language.japanese" => Some(crate::i18n::Locale::Ja),
        "app.language.portuguese" => Some(crate::i18n::Locale::PtBr),
        "app.language.french" => Some(crate::i18n::Locale::Fr),
        "app.language.spanish" => Some(crate::i18n::Locale::Es),
        "app.language.german" => Some(crate::i18n::Locale::De),
        "app.language.russian" => Some(crate::i18n::Locale::Ru),
        "app.language.ukrainian" => Some(crate::i18n::Locale::Uk),
        "app.language.polish" => Some(crate::i18n::Locale::Pl),
        "app.language.czech" => Some(crate::i18n::Locale::Cs),
        "app.language.italian" => Some(crate::i18n::Locale::It),
        "app.language.georgian" => Some(crate::i18n::Locale::Ka),
        _ => None,
    }
}

pub const UI_COMMANDS: &[UiCommand] = &[
    ui!("ui.dock", "Arrange Panel", None, "", lightcraft_engine::cmd::params::Keys<crate::commands::view::DockKeys>, crate::commands::view::dock),
    ui!(
        "ui.dock.reset",
        "Reset Panel Layout",
        None,
        "Window",
        lightcraft_engine::cmd::params::Keys<crate::commands::view::DockKeys>,
        crate::commands::view::dock
    ),
    ui!("modelSetup.cancel", "Cancel Pending Photo Action", None, "", raw crate::commands::app::model_setup_cancel),
    ui!("view.photoGrid", "Photo Grid", None, "View", NoParams, crate::commands::view::view_photo_grid),
    ui!("view.squareGrid", "Square Grid", None, "View", NoParams, crate::commands::view::view_square_grid),
    // G: Photo Grid ↔ Square Grid (from other views: the photo grid)
    ui!("view.gridToggle", "Grid", Some("G"), "", NoParams, crate::commands::view::view_grid_toggle),
    ui!(
        "library.selectOnlyActive",
        "Select Only Active Photo",
        Some("Cmd+Shift+D"),
        "Edit",
        NoParams,
        crate::commands::app::library_select_only_active
    ),
    ui!("tool.guidedUpright", "Guided Upright", Some("Shift+G"), "Window>Tools", crate::commands::view::Tool, crate::commands::view::tool),
    ui!("view.detail", "Detail", Some("D"), "View", NoParams, crate::commands::view::view_detail),
    ui!("view.compare", "Compare", Some("Shift+C"), "View", NoParams, crate::commands::view::view_compare),
    ui!("view.survey", "Survey", Some("N"), "View", NoParams, crate::commands::view::view_survey),
    ui!("view.people", "People", None, "View", NoParams, crate::commands::view::view_people),
    ui!("view.person", "Show Person", None, "", crate::commands::view::Person, crate::commands::view::view_person),
    ui!("view.faceBoxes", "Face Boxes", None, "View", crate::commands::view::Show, crate::commands::view::view_face_boxes),
    ui!("view.focusPoint", "Show Focus Point", Some("Alt+F"), "View", crate::commands::view::On, crate::commands::view::view_focus_point),
    ui!("view.zoomToFocus", "Zoom to Focus Point", None, "View", crate::commands::view::On, crate::commands::view::view_zoom_to_focus),
    ui!("view.reference", "Reference View", Some("Shift+R"), "View", NoParams, crate::commands::view::view_reference),
    ui!("photo.setReference", "Set as Reference Photo", None, "", raw crate::commands::app::photo_set_reference),
    ui!("compare.swap", "Swap Compare Photos", None, "View", NoParams, crate::commands::view::compare_swap),
    ui!("compare.set", "Show in Compare", None, "", crate::commands::view::CompareSet, crate::commands::view::compare_set),
    ui!("compare.makeSelect", "Make Candidate the Select", None, "View", NoParams, crate::commands::view::compare_make_select),
    ui!("view.autoAdvance", "Auto Advance", None, "Photo", NoParams, crate::commands::view::view_auto_advance),
    ui!("view.filmstrip", "Filmstrip", Some("/"), "View", NoParams, crate::commands::view::view_filmstrip),
    ui!("view.leftPanel", "My Photos Panel", Some("Cmd+Shift+L"), "View", crate::commands::view::Show, crate::commands::view::view_left_panel),
    ui!("view.beforeAfter", "Compare Before and After", Some("Y"), "View", NoParams, crate::commands::view::view_before_after),
    ui!("view.beforeAfterSplit", "Before/After Split", Some("Shift+Y"), "View", NoParams, crate::commands::view::view_before_after_split),
    ui!("view.beforeAfterTopBottom", "Before/After Top/Bottom", Some("Alt+Y"), "View", NoParams, crate::commands::view::view_before_after_top_bottom),
    ui!(
        "view.beforeAfterSplitTopBottom",
        "Before/After Split Top/Bottom",
        Some("Alt+Shift+Y"),
        "View",
        NoParams,
        crate::commands::view::view_before_after_split_top_bottom
    ),
    ui!("view.showOriginal", "Show Original", Some("\\"), "View", NoParams, crate::commands::view::view_show_original),
    ui!("view.zoomFit", "Zoom to Fit", Some("Cmd+0"), "View", NoParams, crate::commands::view::view_zoom_fit),
    ui!("view.zoom100", "Zoom 100%", Some("Cmd+Alt+0"), "View", NoParams, crate::commands::view::view_zoom100),
    ui!("view.zoomToggle", "Toggle Zoom", Some("Z"), "View", NoParams, crate::commands::view::view_zoom_toggle),
    // the ratio a click (and Z / Space) zooms to
    ui!("view.clickZoom", "Click Zoom Ratio", None, "", crate::commands::view::ClickZoom, crate::commands::view::view_click_zoom),
    ui!("view.navigate", "Set Image Zoom and Pan", None, "", crate::commands::view::Navigate, crate::commands::view::view_navigate),
    ui!("view.zoomIn", "Zoom In", Some("Cmd+="), "View", NoParams, crate::commands::view::view_zoom_in),
    ui!("view.zoomOut", "Zoom Out", Some("Cmd+-"), "View", NoParams, crate::commands::view::view_zoom_in),
    // Library grids only (Photo Grid, Square Grid, People): the bottom-bar slider's step by key
    ui!("view.thumbSizeUp", "Increase Grid Thumbnail Size", Some("="), "View", NoParams, crate::commands::view::view_thumb_size_up),
    ui!("view.thumbSizeDown", "Decrease Grid Thumbnail Size", Some("-"), "View", NoParams, crate::commands::view::view_thumb_size_up),
    ui!("view.clipping", "Show Clipping", Some("J"), "View", NoParams, crate::commands::view::view_clipping),
    ui!("view.visualizeHdr", "Visualize HDR", None, "View", crate::commands::view::On, crate::commands::view::view_visualize_hdr),
    // in grids S expands/collapses stacks (the engine command it shadows)
    ui!("view.softProof", "Soft Proofing", Some("S"), "View", crate::commands::view::SoftProof, crate::commands::view::view_soft_proof),
    ui!("view.histogram", "Histogram", Some("Cmd+Shift+H"), "View", NoParams, crate::commands::view::view_histogram),
    ui!("view.maskOverlay", "Show Mask Overlay", Some("O"), "View", crate::commands::view::Show, crate::commands::view::view_mask_overlay),
    // Ctrl+Shift+O / Cmd+Shift+O: every mask's region at once, not just the selected one
    ui!("view.maskOverlayAll", "Show All Mask Overlays", Some("Cmd+Shift+O"), "View", NoParams, crate::commands::view::view_mask_overlay_all),
    // Shift+O in the Masking panel (elsewhere it cycles the crop overlay)
    ui!("view.maskOverlayMode", "Cycle Mask Overlay Mode", None, "View", crate::commands::view::Mode, crate::commands::view::view_mask_overlay_mode),
    ui!(
        "view.maskOverlayColor",
        "Cycle Mask Overlay Color",
        None,
        "View",
        crate::commands::view::MaskOverlayColor,
        crate::commands::view::view_mask_overlay_color
    ),
    ui!("view.maskPins", "Show Mask Pins", None, "View", crate::commands::view::Show, crate::commands::view::view_mask_pins),
    // {mode?: auto | always | selected | never}: when spot and mask pins show (steps to the next
    // mode when omitted; Shift+H, like Classic's H for pins, since H opens Remove). View ▸ Tool
    // Overlay and the Remove panel list the modes.
    ui!("view.toolOverlay", "Cycle Tool Overlay", Some("Shift+H"), "", crate::commands::view::Mode, crate::commands::view::view_tool_overlay),
    ui!("view.visualizeSpots", "Visualize Spots", Some("A"), "View", NoParams, crate::commands::view::view_visualize_spots),
    ui!("view.cropOverlay", "Cycle Crop Overlay", Some("Shift+O"), "View", NoParams, crate::commands::view::view_crop_overlay),
    ui!(
        "view.cropOverlayOrientation",
        "Cycle Crop Overlay Orientation",
        None,
        "View",
        NoParams,
        crate::commands::view::view_crop_overlay_orientation
    ),
    ui!("view.back", "Back to Grid", Some("Escape"), "", NoParams, crate::commands::view::view_back),
    ui!("tool.done", "Done", Some("Enter"), "", NoParams, crate::commands::view::tool_done),
    ui!("view.filterBar", "Filter Bar", Some("Shift+F"), "View", NoParams, crate::commands::view::view_filter_bar),
    ui!("local.addRoot", "Add Folder to Local", None, "", crate::commands::view::PathParam, crate::commands::view::local_add_root),
    ui!("local.hide", "Remove from Local", None, "", crate::commands::view::PathParam, crate::commands::view::local_hide),
    ui!(
        "local.restoreHidden",
        "Show Hidden Local Locations",
        None,
        "",
        crate::commands::view::PathParam,
        crate::commands::view::local_restore_hidden
    ),
    ui!("view.fullScreenPreview", "Full Screen Preview", Some("F"), "View", NoParams, crate::commands::view::view_full_screen_preview),
    ui!("view.lightsOut", "Lights Out", Some("Shift+L"), "View", crate::commands::view::Mode, crate::commands::view::view_lights_out),
    ui!(
        "view.enterFullScreen",
        "Enter Full Screen",
        Some("Cmd+Shift+F"),
        "View",
        crate::commands::view::On,
        crate::commands::view::view_enter_full_screen
    ),
    ui!("view.infoOverlay", "Cycle Info Overlay", Some("Cmd+I"), "View", crate::commands::view::Mode, crate::commands::view::view_info_overlay),
    ui!(
        "view.chooseLayoutImage",
        "Choose Layout Image…",
        None,
        "View",
        crate::commands::view::PathParam,
        crate::commands::view::view_choose_layout_image
    ),
    ui!("view.layoutImage", "Show Layout Image", Some("Cmd+Alt+O"), "View", crate::commands::view::Show, crate::commands::view::view_layout_image),
    ui!(
        "view.layoutImageOpacity",
        "Layout Image Opacity",
        None,
        "",
        crate::commands::view::LayoutImageLevel,
        crate::commands::view::view_layout_image_opacity
    ),
    ui!(
        "view.layoutImageMatte",
        "Layout Image Matte",
        None,
        "",
        crate::commands::view::LayoutImageLevel,
        crate::commands::view::view_layout_image_opacity
    ),
    ui!("view.navigator", "Navigator", None, "View", NoParams, crate::commands::view::view_navigator),
    ui!("panel.edit", "Edit", Some("E"), "Window", NoParams, crate::commands::view::panel_edit),
    ui!("panel.profiles", "Profile Browser", None, "Window", NoParams, crate::commands::view::panel_profiles),
    ui!("panel.crop", "Crop & Rotate", Some("C"), "Window", NoParams, crate::commands::view::panel_crop),
    ui!("panel.remove", "Remove", Some("H"), "Window", NoParams, crate::commands::view::panel_remove),
    ui!("panel.masking", "Masking", Some("M"), "Window", NoParams, crate::commands::view::panel_masking),
    ui!("panel.redeye", "Red Eye", None, "Window", NoParams, crate::commands::view::panel_redeye),
    ui!("panel.presets", "Presets", Some("Shift+P"), "Window", NoParams, crate::commands::view::panel_presets),
    ui!("panel.info", "Info", Some("I"), "Window", NoParams, crate::commands::view::panel_info),
    ui!("panel.keywords", "Keywords", Some("K"), "Window", NoParams, crate::commands::view::panel_keywords),
    ui!("panel.versions", "Versions", Some("Shift+V"), "Window", NoParams, crate::commands::view::panel_versions),
    ui!("panel.activity", "History", None, "Window", NoParams, crate::commands::view::panel_activity),
    ui!("panel.close", "Close Panel", None, "", NoParams, crate::commands::view::panel_close),
    // {solo?}: true opens just that section (⌥-click on its header); otherwise opens or closes it
    ui!("section.light", "Light", Some("Cmd+1"), "Window>Edit Sections", crate::commands::view::Section, crate::commands::view::section),
    ui!("section.color", "Color", Some("Cmd+2"), "Window>Edit Sections", crate::commands::view::Section, crate::commands::view::section),
    ui!("section.effects", "Effects", Some("Cmd+3"), "Window>Edit Sections", crate::commands::view::Section, crate::commands::view::section),
    ui!("section.detail", "Detail", Some("Cmd+4"), "Window>Edit Sections", crate::commands::view::Section, crate::commands::view::section),
    ui!("section.optics", "Optics", Some("Cmd+5"), "Window>Edit Sections", crate::commands::view::Section, crate::commands::view::section),
    ui!("section.calibration", "Calibration", None, "", crate::commands::view::Section, crate::commands::view::section),
    // {on?}; no params toggles. Opening one Edit section closes the others.
    ui!("view.soloSections", "Solo Mode", None, "Window>Edit Sections", crate::commands::view::On, crate::commands::view::view_solo_sections),
    ui!("tool.brush", "Brush", Some("B"), "Window>Tools", crate::commands::view::Tool, crate::commands::view::tool),
    ui!("tool.linear", "Linear Gradient", Some("L"), "Window>Tools", crate::commands::view::Tool, crate::commands::view::tool),
    ui!("tool.radial", "Radial Gradient", Some("R"), "Window>Tools", crate::commands::view::Tool, crate::commands::view::tool),
    ui!("tool.wbPicker", "White Balance Selector", Some("W"), "Window>Tools", crate::commands::view::Tool, crate::commands::view::tool),
    ui!("tool.none", "No Tool", None, "", crate::commands::view::Tool, crate::commands::view::tool),
    // brush size / feather of the active brush (Masking brush, Remove tool and its selected spot)
    ui!("brush.smaller", "Decrease Brush Size", Some("["), "Window>Tools", NoParams, crate::commands::view::brush_smaller),
    ui!("brush.larger", "Increase Brush Size", Some("]"), "Window>Tools", NoParams, crate::commands::view::brush_smaller),
    ui!("brush.featherLess", "Decrease Brush Feather", Some("Shift+["), "Window>Tools", NoParams, crate::commands::view::brush_smaller),
    ui!("brush.featherMore", "Increase Brush Feather", Some("Shift+]"), "Window>Tools", NoParams, crate::commands::view::brush_smaller),
    ui!("dialog.newAlbum", "New Album…", Some("Cmd+N"), "File", raw crate::commands::dialogs::dialog_new_album),
    ui!("dialog.newFolder", "New Folder…", Some("Cmd+Shift+N"), "File", raw crate::commands::dialogs::dialog_new_folder),
    ui!("dialog.smartAlbum", "New Smart Album…", None, "File", raw crate::commands::dialogs::dialog_smart_album),
    ui!("view.photoCounts", "Show Photo Counts", None, "View", crate::commands::view::Show, crate::commands::view::view_photo_counts),
    ui!("view.sidebarSection", "Show Sidebar Section", None, "", crate::commands::view::SidebarSection, crate::commands::view::view_sidebar_section),
    ui!(
        "view.sidebarMoveSection",
        "Move Sidebar Section",
        None,
        "",
        crate::commands::view::SidebarMove,
        crate::commands::view::view_sidebar_move_section
    ),
    ui!("view.sidebarReset", "Reset Sidebar Sections", None, "", NoParams, crate::commands::view::view_sidebar_reset),
    ui!(
        "view.slideshow",
        "Slideshow",
        Some("Cmd+Alt+Enter"),
        "View",
        lightcraft_engine::cmd::params::Keys<crate::commands::view::SlideshowKeys>,
        crate::commands::view::view_slideshow
    ),
    ui!("view.slideshowOptions", "Slideshow Options…", None, "View", NoParams, crate::commands::view::view_slideshow_options),
    ui!("dialog.slideshowExport", "Export Slideshow…", None, "View", raw crate::commands::dialogs::dialog_slideshow_export),
    ui!("app.slideshowExport", "Export Slideshow", None, "", raw crate::commands::app::app_slideshow_export),
    // the slideshow's playback controls (Space, ←/→ and Esc while it plays)
    ui!("slideshow.togglePause", "Pause / Resume Slideshow", None, "", NoParams, crate::commands::view::slideshow_toggle_pause),
    ui!("slideshow.next", "Next Slide", None, "", NoParams, crate::commands::view::slideshow_toggle_pause),
    ui!("slideshow.previous", "Previous Slide", None, "", NoParams, crate::commands::view::slideshow_toggle_pause),
    ui!("slideshow.end", "End Slideshow", None, "", NoParams, crate::commands::view::slideshow_toggle_pause),
    ui!("view.secondWindow", "Second Window", Some("Cmd+F11"), "Window", crate::commands::view::Show, crate::commands::view::view_second_window),
    ui!("tool.keywordPainter", "Keyword Painter", None, "", crate::commands::view::KeywordPainter, crate::commands::view::tool_keyword_painter),
    ui!("view.gridInfo", "Grid Info", None, "", crate::commands::view::GridInfo, crate::commands::view::view_grid_info),
    // {mode?: auto | light | dark}: Appearance Mode (cycles Auto, Light, Dark when omitted; the
    // top bar's appearance button). View ▸ Appearance lists the modes.
    ui!("view.appearance", "Next Appearance Mode", None, "", crate::commands::view::Mode, crate::commands::view::view_appearance),
    // {theme: charcoal | midnight | silver | paper}: pick a theme and fix the mode to its family.
    ui!("view.theme", "Theme", None, "", crate::commands::view::Theme, crate::commands::view::view_theme),
    ui!("dialog.allMetadata", "All Metadata…", None, "Photo", raw crate::commands::dialogs::dialog_all_metadata),
    ui!("dialog.faceModel", "Add Face Model…", None, "", raw crate::commands::dialogs::dialog_face_model),
    ui!("dialog.newSmartAlbum", "New Smart Album from Filter…", Some("Cmd+Alt+N"), "File", raw crate::commands::dialogs::dialog_new_smart_album),
    ui!("dialog.createPreset", "Create Preset…", Some("Cmd+Shift+P"), "Photo", NoParams, crate::commands::dialogs::dialog_create_preset),
    ui!("dialog.autoStack", "Auto-Stack by Capture Time…", None, "Photo>Stack", raw crate::commands::dialogs::dialog_auto_stack),
    ui!(
        "dialog.copySettings",
        "Choose Edit Settings to Copy…",
        Some("Cmd+Shift+C"),
        "Edit",
        NoParams,
        crate::commands::dialogs::dialog_copy_settings
    ),
    ui!("dialog.pasteSettings", "Paste Selected Settings…", Some("Cmd+Shift+V"), "Edit", NoParams, crate::commands::dialogs::dialog_paste_settings),
    ui!("view.focusSearch", "Find…", Some("Cmd+F"), "Edit", NoParams, crate::commands::view::view_focus_search),
    ui!("dialog.export", "Export…", None, "File", NoParams, crate::commands::dialogs::dialog_export),
    ui!("dialog.print", "Print…", Some("Cmd+P"), "File", NoParams, crate::commands::dialogs::dialog_print),
    ui!("app.print", "Print to PDF", None, "", raw crate::commands::app::app_print),
    ui!("dialog.contactSheet", "Contact Sheet PDF…", None, "File", NoParams, crate::commands::dialogs::dialog_contact_sheet),
    ui!("app.contactSheet", "Export Contact Sheet PDF", None, "", raw crate::commands::app::app_contact_sheet),
    ui!("photo.editInPhotoCraft", "Edit in PhotoCraft", Some("Cmd+Shift+E"), "Photo>Edit In", raw crate::commands::app::photo_edit_in_photo_craft),
    ui!("photo.editInExternal", "Edit in Other Editor", Some("Cmd+Alt+E"), "Photo>Edit In", raw crate::commands::app::photo_edit_in_external),
    // {ids?, app?}: the original files themselves (a raw stays a raw: no render, no copy), opened
    // in the other editor, e.g. a raw developer
    ui!("photo.openOriginalInExternal", "Open Original in Other Editor (RAW, no copy)", None, "Photo>Edit In", raw crate::commands::app::photo_open_original_in_external),
    ui!("dialog.mergeHdr", "HDR…", Some("Ctrl+H"), "Photo>Photo Merge", NoParams, crate::commands::dialogs::dialog_merge_hdr),
    ui!("dialog.mergePanorama", "Panorama…", Some("Ctrl+M"), "Photo>Photo Merge", NoParams, crate::commands::dialogs::dialog_merge_panorama),
    ui!("dialog.mergeHdrPanorama", "HDR Panorama…", None, "Photo>Photo Merge", NoParams, crate::commands::dialogs::dialog_merge_hdr_panorama),
    ui!("merge.hdrLast", "HDR with Last Settings", Some("Ctrl+Shift+H"), "Photo>Photo Merge", NoParams, crate::commands::app::merge_hdr_last),
    ui!(
        "merge.panoramaLast",
        "Panorama with Last Settings",
        Some("Ctrl+Shift+M"),
        "Photo>Photo Merge",
        NoParams,
        crate::commands::app::merge_panorama_last
    ),
    ui!(
        "merge.hdrPanoramaLast",
        "HDR Panorama with Last Settings",
        None,
        "Photo>Photo Merge",
        NoParams,
        crate::commands::app::merge_hdr_panorama_last
    ),
    ui!("file.addPhotos", "Import Photos…", Some("Cmd+Shift+I"), "File", raw crate::commands::app::file_add_photos),
    ui!("file.addFolder", "Import from Folder…", None, "File", raw crate::commands::app::file_add_folder),
    ui!("file.importLightroom", "Import Lightroom Catalog…", None, "File", raw crate::commands::app::file_import_lightroom),
    ui!("file.addFromDevice", "Import from Device", None, "", raw crate::commands::app::file_add_from_device),
    ui!("file.findMissing", "Find Missing Photos…", None, "File", raw crate::commands::app::file_find_missing),
    ui!("file.backupLibrary", "Back Up Library…", None, "File", NoParams, crate::commands::app::file_backup_library),
    ui!("file.restoreLibrary", "Restore Library from Backup…", None, "File", NoParams, crate::commands::app::file_backup_library),
    ui!("photo.locate", "Locate Missing File…", None, "", raw crate::commands::app::photo_locate),
    ui!("dialog.saveMetadataPreset", "Save Metadata Preset…", None, "", NoParams, crate::commands::dialogs::dialog_save_metadata_preset),
    ui!("app.quit", "Quit LightCraft", Some("Cmd+Q"), "File", NoParams, crate::commands::app::app_quit),
    ui!("file.importPresets", "Import Profiles & Presets…", None, "File", raw crate::commands::app::file_import_presets),
    ui!("file.exportPresets", "Export Presets…", None, "File", raw crate::commands::app::file_export_presets),
    // Masking panel ▸ Effect menu
    ui!(
        "dialog.saveMaskEffect",
        "Save Current Settings as New Effect Preset…",
        None,
        "",
        NoParams,
        crate::commands::dialogs::dialog_save_mask_effect
    ),
    ui!("dialog.manageMaskEffects", "Manage Effect Presets…", None, "", NoParams, crate::commands::dialogs::dialog_manage_mask_effects),
    ui!("file.exportMaskEffects", "Export Effect Presets…", None, "", raw crate::commands::app::file_export_mask_effects),
    ui!("file.importKeywords", "Import Keywords…", None, "File", raw crate::commands::app::file_import_keywords),
    ui!("file.exportKeywords", "Export Keywords…", None, "File", raw crate::commands::app::file_export_keywords),
    // Edit panel ▸ Curve ▸ Point Curve dropdown
    ui!("file.importCurvePresets", "Import Point Curve Presets…", None, "", raw crate::commands::app::file_import_curve_presets),
    ui!("file.exportCurvePresets", "Export Point Curve Presets…", None, "", raw crate::commands::app::file_export_curve_presets),
    ui!("app.settings", "Settings…", Some("Cmd+,"), "Edit", raw crate::commands::app::app_settings),
    ui!("app.newLibrary", "New Library…", None, "File", raw crate::commands::app::app_new_library),
    ui!("app.openLibrary", "Open Library…", Some("Cmd+O"), "File", raw crate::commands::app::app_open_library),
    ui!("app.recentLibraries", "Recent Libraries", None, "", raw crate::commands::app::app_recent_libraries),
    ui!("app.clearRecentLibraries", "Clear Recent Libraries", None, "", NoParams, crate::commands::app::app_clear_recent_libraries),
    // Settings ▸ Display
    ui!("app.displayProfile", "Display Profile", None, "", raw crate::commands::app::app_display_profile),
    ui!("app.about", "About LightCraft", None, "Help", NoParams, crate::commands::app::app_about),
    ui!("app.systemInfo", "System Info…", None, "Help", raw crate::commands::app::app_system_info),
    ui!("app.openLogFolder", "Open Log Folder", None, "Help", NoParams, crate::commands::app::app_open_log_folder),
    ui!("app.whatsNew", "What's New", None, "Help", NoParams, crate::commands::app::app_whats_new),
    ui!("dialog.cull", "Assisted Culling…", None, "Photo", NoParams, crate::commands::dialogs::dialog_cull),
    ui!("app.help", "LightCraft Help", Some("F1"), "Help", NoParams, crate::commands::app::app_discord),
    ui!("app.discord", "Join the ArtCraft Discord…", None, "Help", NoParams, crate::commands::app::app_discord),
    ui!("app.feedback", "Send Feedback…", None, "Help", NoParams, crate::commands::app::app_discord),
    ui!("app.website", "LightCraft Website", None, "Help", NoParams, crate::commands::app::app_discord),
    ui!("app.github", "LightCraft on GitHub", None, "Help", NoParams, crate::commands::app::app_discord),
    ui!("app.artcraft", "ArtCraft Website", None, "Help", NoParams, crate::commands::app::app_discord),
    ui!("app.shortcuts", "Keyboard Shortcuts", Some("Cmd+/"), "Help", NoParams, crate::commands::app::app_shortcuts),
    ui!("app.setShortcut", "Set Keyboard Shortcut", None, "", raw crate::commands::app::app_set_shortcut),
    ui!("app.resetShortcuts", "Reset All Keyboard Shortcuts", None, "", NoParams, crate::commands::app::app_reset_shortcuts),
    ui!("app.export", "Export Now", None, "", raw crate::commands::app::app_export),
    ui!("app.showInFinder", "Show in Finder", Some("Cmd+R"), "Photo", NoParams, crate::commands::app::app_show_in_finder),
    ui!("app.openSamModelFolder", "Show in File Manager", None, "", NoParams, crate::commands::app::app_open_sam_model_folder),
    ui!("dialog.rename", "Rename Photos…", Some("F2"), "Photo", raw crate::commands::dialogs::dialog_rename),
    ui!("dialog.labelNames", "Edit Color Label Names…", None, "", NoParams, crate::commands::dialogs::dialog_label_names),
    ui!("dialog.captureTime", "Edit Capture Time…", None, "Photo", raw crate::commands::dialogs::dialog_capture_time),
    ui!("photo.tagFromTracklog", "Auto-Tag from Tracklog…", None, "Photo", raw crate::commands::app::photo_tag_from_tracklog),
    ui!("app.exportPrevious", "Export with Previous", Some("Cmd+Alt+Shift+E"), "File", NoParams, crate::commands::app::app_export_previous),
];

pub(crate) fn panel(app: &mut LightcraftApp, ctx: &egui::Context, p: RightPanel, name: &str) {
    if app.ui.right == p && crate::docking::exposed(app, crate::docking::Panel::Tools) {
        app.ui.right = RightPanel::None;
        app.toast(ctx, crate::i18n::tr_format!("{name} Off", name = crate::i18n::tr(name)));
    } else {
        app.ui.right = p;
        app.toast(ctx, crate::i18n::tr_format!("{name} On", name = crate::i18n::tr(name)));
        if p.is_edit_tool() && !matches!(app.ui.view, ViewMode::Detail) {
            app.ui.view = ViewMode::Detail;
        }
    }
    if p != RightPanel::Masking && app.ui.tool != "wbPicker" {
        app.ui.tool.clear();
    }
    let _ = app.session.end_interaction();
}

/// `[` / `]` (size ×`k`) and ⇧`[` / ⇧`]` (feather +`df`) for the brush in use: the Remove tool's
/// (and its selected spot's) or the Masking brush's.
pub(crate) fn adjust_brush(app: &mut LightcraftApp, k: f32, df: f32) -> Value {
    if app.ui.right == RightPanel::Remove {
        app.ui.remove_size = (app.ui.remove_size * k).clamp(0.001, 0.25);
        app.ui.remove_feather = (app.ui.remove_feather + df).clamp(0.0, 100.0);
        if app.session.active_spot.is_some() {
            let mut p = json!({});
            if k != 1.0 {
                p["size"] = json!(app.ui.remove_size);
            }
            if df != 0.0 {
                p["feather"] = json!(app.ui.remove_feather);
            }
            let _ = app.run("spot.update", p);
        }
        json!({"size": app.ui.remove_size, "feather": app.ui.remove_feather})
    } else {
        app.ui.brush_size = (app.ui.brush_size * k).clamp(0.002, 0.5);
        app.ui.brush_feather = (app.ui.brush_feather + df).clamp(0.0, 100.0);
        json!({"size": app.ui.brush_size, "feather": app.ui.brush_feather})
    }
}

/// The `parent` folder id of a `dialog.new*` command (none: the top level).
pub(crate) fn parent_param(p: &Value) -> Option<u64> {
    p.get("parent").and_then(Value::as_u64)
}

/// `file.addPhotos` / `file.addFolder` / `file.addFromDevice` `{album?}`: the review of the scan
/// just started opens with that album chosen (an album's context menu ▸ Import ▸ …). An error
/// means another scan is running.
pub(crate) fn import_into_album(app: &mut LightcraftApp, p: &Value, r: Result<Value, String>) -> Result<Value, String> {
    if r.is_ok()
        && let Some(t) = &mut app.scan
    {
        t.album = p.get("album").and_then(Value::as_u64);
    }
    r
}

/// An sRGB colour from `"#rrggbb"` or `[r, g, b]` (0..255).
pub fn parse_rgb(v: &Value) -> Option<[u8; 3]> {
    if let Some(s) = v.as_str() {
        let h = s.strip_prefix('#').unwrap_or(s);
        if h.len() != 6 {
            return None;
        }
        let c = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        return Some([c(0)?, c(2)?, c(4)?]);
    }
    let a = v.as_array()?;
    let c = |i: usize| a.get(i)?.as_f64().map(|x| x.clamp(0.0, 255.0).round() as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

/// The left-sidebar section a `view.sidebar*` command names (`section`), which this build must have.
/// What Shift+H says it switched to (the pins themselves may be off screen).
pub(crate) fn tool_overlay_toast(mode: crate::state::ToolOverlay) -> String {
    use crate::state::ToolOverlay;
    crate::i18n::tr(match mode {
        ToolOverlay::Auto => "Tool Overlay: Auto",
        ToolOverlay::Always => "Tool Overlay: Always",
        ToolOverlay::Selected => "Tool Overlay: Selected",
        ToolOverlay::Never => "Tool Overlay: Never",
    })
    .to_string()
}

pub(crate) fn appearance_json(app: &LightcraftApp) -> Value {
    let s = &app.ui.settings;
    json!({"mode": s.appearance_mode, "darkTheme": s.dark_theme, "lightTheme": s.light_theme})
}

/// `view.appearance {mode?}`: set Appearance Mode, or step to the next one (Auto, Light, Dark).
/// The theme follows on the next frame ([`LightcraftApp::sync_theme`]).
pub(crate) fn appearance_command(app: &mut LightcraftApp, mode: Option<&str>) -> Result<Value, String> {
    use crate::state::AppearanceMode;
    let mode = match mode {
        None => app.ui.settings.appearance_mode.next(),
        Some("auto") => AppearanceMode::Auto,
        Some("light") => AppearanceMode::Light,
        Some("dark") => AppearanceMode::Dark,
        Some(m) => return Err(format!("view.appearance: unknown mode {m:?} (auto|light|dark)")),
    };
    app.ui.settings.appearance_mode = mode;
    Ok(appearance_json(app))
}

/// `view.theme {theme}`: pick a theme; the mode becomes its family's (Dark or Light).
pub(crate) fn theme_command(app: &mut LightcraftApp, theme: Option<&str>) -> Result<Value, String> {
    let ids = crate::theme::ThemeKind::ALL.map(|k| k.id()).join("|");
    let Some(id) = theme else {
        return Err(format!("view.theme: `theme` is required ({ids})"));
    };
    let Some(kind) = crate::theme::ThemeKind::ALL.into_iter().find(|k| k.id() == id) else {
        return Err(format!("view.theme: unknown theme `{id}` ({ids})"));
    };
    app.ui.settings.select_theme(kind);
    Ok(appearance_json(app))
}

/// Engine commands the app runs its own way (on a worker thread, with its dialogs): not listed
/// again, run in place of the engine's.
fn ui_override(id: &str) -> Option<Run> {
    match id {
        "library.inspectLightroom" | "library.importLightroom" => Some(crate::commands::app::lightroom),
        "merge.hdrBatch" => Some(crate::commands::app::merge_hdr_batch),
        // listed and read in the background (the web build browses through the engine)
        "library.browse" if !cfg!(target_arch = "wasm32") => Some(crate::commands::app::library_browse),
        _ => None,
    }
}

/// Run a UI command (or the app's own way of running an engine command); `None` means "not a UI
/// command — send it to the engine".
pub fn run_ui_command(app: &mut LightcraftApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let run = ui_commands().find(|c| c.id == id).map(|c| c.run).or_else(|| ui_override(id))?;
    Some(run(app, &Call { id, params: p }))
}

/// Views whose bottom bar has the thumbnail-size slider and that draw tiles of that size.
pub(crate) fn thumb_size_view(app: &LightcraftApp) -> bool {
    matches!(app.ui.view, ViewMode::PhotoGrid | ViewMode::SquareGrid | ViewMode::People)
}

pub fn ui_enabled(app: &LightcraftApp, id: &str) -> bool {
    match id {
        "library.selectOnlyActive" => app.session.active().is_some(),
        "view.thumbSizeUp" | "view.thumbSizeDown" => thumb_size_view(app),
        "dialog.contactSheet" | "app.contactSheet" | "dialog.print" | "app.print" => {
            !cfg!(target_arch = "wasm32") && app.session.active().is_some() && app.export.is_none()
        }
        "dialog.slideshowExport" | "app.slideshowExport" => {
            !cfg!(target_arch = "wasm32") && !app.session.slideshow_export.as_ref().is_some_and(|r| r.running())
        }
        s if s.starts_with("panel.") || s.starts_with("tool.") || s.starts_with("section.") => app.session.active().is_some() || s == "panel.close",
        "app.export" | "dialog.export" | "dialog.createPreset" | "dialog.rename" | "dialog.captureTime" | "dialog.copySettings" => {
            app.session.active().is_some()
        }
        "photo.tagFromTracklog" => app.session.active().is_some() && app.services.pick_tracklog.is_some(),
        "photo.editInPhotoCraft" | "photo.editInExternal" => app.session.active().is_some() && app.services.open_with.is_some(),
        "app.exportPrevious" => app.session.active().is_some() && app.session.last_export.is_some(),
        "dialog.pasteSettings" => app.session.active().is_some() && app.session.clipboard.is_some(),
        "photo.openOriginalInExternal" => {
            // only a file type LightCraft reads can be handed over (#2135); the extension alone
            // decides, so no disk or share is touched while a menu is drawn
            app.services.open_with.is_some()
                && app.session.active().and_then(|id| app.session.catalog.photo(id)).is_some_and(|p| match &p.source {
                    lightcraft_engine::catalog::Source::File { path } => lightcraft_engine::open_path::is_media(std::path::Path::new(path)),
                    _ => false,
                })
        }
        "app.showInFinder" => {
            app.services.reveal.is_some()
                && app
                    .session
                    .active()
                    .and_then(|id| app.session.catalog.photo(id))
                    .is_some_and(|p| matches!(p.source, lightcraft_engine::catalog::Source::File { .. }))
        }
        "app.openSamModelFolder" => app.session.segmenter.dir.is_some() && app.services.open_with.is_some(),
        "file.exportPresets" => app.session.presets.iter().any(|p| !p.builtin),
        "file.exportMaskEffects" => !app.session.mask_effects.is_empty(),
        "dialog.saveMaskEffect" => app.session.active().is_some() && app.session.active_mask.is_some(),
        "file.exportCurvePresets" => !app.session.curve_presets.is_empty(),
        "view.compare" => app.session.catalog.len() > 1,
        "view.fullScreenPreview" | "view.infoOverlay" | "view.navigator" => app.session.active().is_some() || app.ui.fullscreen,
        "app.openLibrary" | "app.newLibrary" => app.services.pick_folder.is_some() && crate::panels::settings::can_switch_library(app).is_ok(),
        "file.addFolder" => app.services.pick_folder.is_some() && !crate::lightroom_import::is_running(app),
        "file.backupLibrary" => app.services.backup_library.is_some(),
        "app.openLogFolder" => app.services.reveal.is_some() && app.services.log_file.is_some(),
        "file.restoreLibrary" => app.services.restore_library.is_some(),
        "app.clearRecentLibraries" => !app.ui.settings.recent_libraries.is_empty(),
        "compare.swap" | "compare.makeSelect" | "compare.set" => app.ui.view == ViewMode::Compare,
        "slideshow.togglePause" | "slideshow.next" | "slideshow.previous" | "slideshow.end" => app.ui.slideshow.is_some(),
        s if s.starts_with("dialog.merge") || (s.starts_with("merge.") && s.ends_with("Last")) => {
            app.session.selected().len() >= 2 && app.merge.final_task.is_none()
        }
        _ => true,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MenuEntry {
    pub id: String,
    pub label: String,
    pub menu: Vec<String>,
    pub shortcut: Option<String>,
    pub enabled: bool,
}

/// The flattened menu model (UI commands + engine commands with menu paths).
pub fn menu_entries(app: &LightcraftApp) -> Vec<MenuEntry> {
    let mut v: Vec<MenuEntry> = ui_commands()
        .filter(|c| !c.menu.is_empty())
        .map(|UiCommand { id, label, shortcut: sc, menu: m, .. }| MenuEntry {
            id: id.to_string(),
            label: label.to_string(),
            menu: m.split('>').map(str::to_string).collect(),
            shortcut: crate::shortcuts::binding(&app.ui.settings.keymap, id, *sc).map(str::to_string),
            enabled: ui_enabled(app, id),
        })
        .collect();
    for c in app.session.commands() {
        if !c.menu.is_empty() {
            v.push(MenuEntry {
                id: c.id.into(),
                label: c.label.into(),
                menu: c.menu.iter().map(|s| s.to_string()).collect(),
                shortcut: crate::shortcuts::shortcut_of(&app.ui.settings.keymap, c.id).map(str::to_string),
                enabled: c.enabled,
            });
        }
    }
    v
}

/// The Delete key in an album: a normal album loses the selected photos (`album.removePhotos`),
/// which stay in the library and their folder; a smart album keeps them (its rules decide) and
/// says so. True when the key was handled here, false outside albums (then it deletes the photos
/// from the library). Photo ▸ Delete Photo still deletes from the library everywhere.
pub fn delete_in_album(app: &mut LightcraftApp, ctx: &egui::Context) -> bool {
    let lightcraft_engine::LibrarySource::Album(aid) = app.session.source else { return false };
    let Some(album) = app.session.catalog.album(aid) else { return false };
    if album.is_smart() {
        app.toast(ctx, crate::i18n::tr("Smart albums update automatically: change their rules, or use Photo ▸ Delete Photo"));
        return true;
    }
    if app.session.selected().is_empty() {
        return true;
    }
    match app.run("album.removePhotos", json!({"id": aid.0})) {
        Ok(r) => {
            let n = r["removed"].as_u64().unwrap_or(0);
            let name = r["album"].as_str().unwrap_or_default().to_string();
            let what = crate::i18n::tr_format!("{n} photo{}", if n == 1 { "" } else { "s" }, n = n);
            app.toast(ctx, crate::i18n::tr_format!("Removed {what} from {name}", what = what, name = name));
        }
        Err(e) => app.toast(ctx, e),
    }
    true
}

/// With Settings → General → "Confirm before deleting" on, open the confirmation dialog instead
/// of deleting; true when it did (the dialog's OK runs `photo.delete`).
pub fn confirm_delete(app: &mut LightcraftApp) -> bool {
    if !app.ui.settings.confirm_delete {
        return false;
    }
    let count = app.session.selected().len();
    if count == 0 {
        return false;
    }
    app.ui.dialog = Some(Dialog::ConfirmDelete { count });
    true
}

/// Platform-appropriate label for revealing a file in the system file manager.
pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else {
        "Show in File Manager"
    }
}

/// Help ▸ Open Log Folder: reveal the host's log file in the system file manager, so it can be
/// attached to a report without hunting for the settings folder (#260).
pub(crate) fn open_log_folder(app: &mut LightcraftApp) -> Result<Value, String> {
    let path = app.services.log_file.clone().ok_or("this session keeps no log file")?;
    let reveal = app.services.reveal.as_mut().ok_or("not available here")?;
    reveal(&path)?;
    Ok(json!({"path": path}))
}

/// Reveal the active photo's original in the system file manager.
pub(crate) fn show_in_finder(app: &mut LightcraftApp) -> Result<Value, String> {
    let id = app.session.active().ok_or("no photo selected")?;
    let path = match app.session.catalog.photo(id).map(|p| p.source.clone()) {
        Some(lightcraft_engine::catalog::Source::File { path }) => path,
        _ => return Err("this photo has no file (demo scene)".into()),
    };
    let reveal = app.services.reveal.as_mut().ok_or("not available here")?;
    reveal(&path)?;
    Ok(json!({"path": path}))
}

/// Photo ▸ Edit In: render a copy of each selected photo with its edits (stacked on its original)
/// as Settings ▸ External Editing asks for that editor, then open them there. `p` may override the
/// file (`format`, `colorSpace`, `bitDepth`) and the editor (`app`). PhotoCraft is checked first,
/// so a missing editor leaves no copy behind.
pub(crate) fn edit_in(app: &mut LightcraftApp, ctx: &egui::Context, p: &Value, photocraft: bool) -> Result<Value, String> {
    let settings = &app.ui.settings;
    let (file, configured) = if photocraft {
        let path = settings.photocraft_path.trim();
        (settings.photocraft_file, if path.is_empty() { crate::state::PHOTOCRAFT.to_string() } else { path.to_string() })
    } else {
        (settings.external_file, settings.external_editor.clone())
    };
    let editor = p.get("app").and_then(Value::as_str).map_or(configured, str::to_string);
    if let Some(check) = app.services.check_editor.as_ref()
        && let Err(e) = check(&editor)
    {
        app.toast(ctx, e.clone());
        return Err(e);
    }
    let mut params = file.params();
    if let (Some(o), Some(given)) = (params.as_object_mut(), p.as_object()) {
        for (k, v) in given.iter().filter(|(k, _)| k.as_str() != "app") {
            o.insert(k.clone(), v.clone());
        }
    }
    let r = app.session.execute("photo.editExternal", &params).map_err(|e| e.to_string())?;
    // every selected photo got a copy (#920); each opens in the editor
    let copies: Vec<(String, Option<u64>)> = r["copies"]
        .as_array()
        .map(|c| c.iter().map(|c| (c["path"].as_str().unwrap_or_default().to_string(), c["id"].as_u64())).collect())
        .unwrap_or_else(|| vec![(r["path"].as_str().unwrap_or_default().to_string(), r["id"].as_u64())]);
    let mut failed = None;
    for (path, id) in &copies {
        if let Some(id) = id {
            app.ui.external_edits.push(*id);
        }
        // the copy is LightCraft's own render, but it goes through the same gate as any other
        // path handed to the system opener (#2135)
        match lightcraft_engine::open_path::openable(path, "") {
            Err(e) => failed = Some(e),
            Ok(absolute) => {
                if let Some(Err(e)) = app.services.open_with.as_mut().map(|f| f(&absolute, &editor)) {
                    failed = Some(e);
                }
            }
        }
    }
    match (failed, copies.as_slice()) {
        (Some(e), _) => app.toast(ctx, crate::i18n::tr_format!("Couldn't open the editor: {e}", e = e)),
        (None, [(path, _)]) => {
            let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            app.toast(ctx, crate::i18n::tr_format!("{name} opened for editing; it is stacked with the original", name = name));
        }
        _ => app.toast(ctx, crate::i18n::tr_format!("{n} photos opened for editing; each is stacked with its original", n = copies.len())),
    }
    Ok(r)
}

/// Most originals one Open Original hands the editor (each starts it once).
const MAX_ORIGINALS: usize = 50;

/// Photo ▸ Edit In ▸ Open Original in Other Editor: hand each selected photo's original file
/// itself (a raw stays a raw: nothing is rendered, copied or added to the library) to the editor
/// set in Settings ▸ External Editing, e.g. a raw developer. `p` may give `ids` and the editor
/// (`app`). Demo photos and missing files are skipped and listed; when no photo has a file to
/// open, it fails saying why.
pub(crate) fn open_original_in(app: &mut LightcraftApp, ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    let r = open_originals(app, p);
    if let Err(e) = &r {
        app.toast(ctx, e.clone());
    }
    let r = r?;
    let opened = r["opened"].as_array().map_or(0, Vec::len);
    match (r["failed"].as_str(), r["opened"][0]["path"].as_str()) {
        (Some(e), _) => app.toast(ctx, crate::i18n::tr_format!("Couldn't open the editor: {e}", e = e)),
        (None, Some(path)) if opened == 1 => {
            let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            app.toast(ctx, crate::i18n::tr_format!("{name} opened in the other editor", name = name));
        }
        _ => app.toast(ctx, crate::i18n::tr_format!("{n} originals opened in the other editor", n = opened)),
    }
    Ok(r)
}

pub(crate) fn open_originals(app: &mut LightcraftApp, p: &Value) -> Result<Value, String> {
    let mut ids = app.session.targets(p).map_err(|e| e.to_string())?;
    ids.dedup();
    if ids.is_empty() {
        return Err("no photo selected".into());
    }
    if ids.len() > MAX_ORIGINALS {
        return Err(format!("{} photos selected: open at most {MAX_ORIGINALS} originals at a time", ids.len()));
    }
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for id in ids {
        // the path may come from an imported catalog: only a photo file LightCraft reads is
        // handed to the opener, never a program (#2135)
        match app.session.catalog.photo(id).map(|ph| (ph.file_name.clone(), ph.source.clone())) {
            Some((name, lightcraft_engine::catalog::Source::File { path })) => match lightcraft_engine::open_path::openable(&path, &name) {
                Ok(absolute) => files.push((id, absolute)),
                Err(reason) => skipped.push(json!({"id": id.0, "reason": reason})),
            },
            Some(_) => skipped.push(json!({"id": id.0, "reason": "a demo photo has no original file"})),
            None => skipped.push(json!({"id": id.0, "reason": "no such photo"})),
        }
    }
    if files.is_empty() {
        let why = skipped.first().and_then(|s| s["reason"].as_str()).unwrap_or("no original file");
        return Err(if skipped.len() == 1 {
            format!("nothing to open: {why}")
        } else {
            format!("nothing to open: none of these photos has its original file ({why})")
        });
    }
    let editor = p.get("app").and_then(Value::as_str).map_or_else(|| app.ui.settings.external_editor.clone(), str::to_string);
    if let Some(check) = app.services.check_editor.as_ref() {
        check(&editor)?;
    }
    // all at once where the host can: an editor that keeps one window (DxO PhotoLab) loses the
    // files that arrive while it is still starting
    if let Some(open_all) = app.services.open_all_with.as_mut() {
        let paths: Vec<String> = files.iter().map(|(_, path)| path.clone()).collect();
        open_all(&paths, &editor).map_err(|e| format!("Couldn't open the editor: {e}"))?;
        let opened: Vec<Value> = files.iter().map(|(id, path)| json!({"id": id.0, "path": path})).collect();
        return Ok(json!({"opened": opened, "skipped": skipped, "app": editor, "failed": null}));
    }
    let open = app.services.open_with.as_mut().ok_or("opening another application isn't available here")?;
    let mut opened = Vec::new();
    let mut failed = None;
    for (id, path) in files {
        match open(&path, &editor) {
            Ok(()) => opened.push(json!({"id": id.0, "path": path})),
            Err(e) => failed = Some(e),
        }
    }
    if opened.is_empty()
        && let Some(e) = failed
    {
        return Err(format!("Couldn't open the editor: {e}"));
    }
    Ok(json!({"opened": opened, "skipped": skipped, "app": editor, "failed": failed}))
}
