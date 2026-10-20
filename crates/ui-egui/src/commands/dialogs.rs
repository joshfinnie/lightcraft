//! UI commands that open dialogs.

use serde_json::{Value, json};

use crate::LightcraftApp;
use crate::menus::*;
use crate::state::Dialog;
use lightcraft_engine::cmd::params::NoParams;

pub(crate) fn dialog_new_folder(app: &mut LightcraftApp, c: &Call, p: NewAlbum) -> Result<Value, String> {
    if p.include_selected.is_some() {
        return Err(format!("{}: a folder holds albums, not photos (`includeSelected` is for dialog.newAlbum)", c.id));
    }
    app.ui.dialog = Some(Dialog::NewAlbum { name: p.name.unwrap_or_default(), folder: true, parent: p.parent, include_selected: false });
    Ok(Value::Null)
}

pub(crate) fn dialog_new_album(app: &mut LightcraftApp, _: &Call, p: NewAlbum) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::NewAlbum {
        name: p.name.unwrap_or_default(),
        folder: false,
        parent: p.parent,
        include_selected: p.include_selected.unwrap_or(true),
    });
    Ok(Value::Null)
}

pub(crate) fn dialog_auto_stack(app: &mut LightcraftApp, _: &Call, p: AutoStack) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::AutoStack { gap: p.gap.unwrap_or(60.0) as f32 });
    Ok(Value::Null)
}

pub(crate) fn dialog_all_metadata(app: &mut LightcraftApp, _: &Call, p: OnePhoto) -> Result<Value, String> {
    let rows = app.session.execute("photo.allMetadata", &json!({"id": p.id})).map_err(|e| e.to_string())?;
    let photo = p.id.or_else(|| app.session.active());
    let title = photo.and_then(|id| app.session.catalog.photo(id)).map(|p| p.file_name.clone()).unwrap_or_default();
    app.ui.dialog = Some(Dialog::AllMetadata { title, rows, search: String::new() });
    Ok(Value::Null)
}

pub(crate) fn dialog_smart_album(app: &mut LightcraftApp, _: &Call, p: SmartAlbum) -> Result<Value, String> {
    // {id?}: edit that smart album's rules; without: a new one starting at Rating ≥ 3
    let album = p.id.and_then(|id| app.session.catalog.album(lightcraft_catalog::AlbumId(id)).filter(|a| a.is_smart()).cloned());
    app.ui.dialog = Some(match album {
        Some(a) => Dialog::SmartRules {
            id: Some(a.id.0),
            name: a.name.clone(),
            // older rules read as they always matched (RuleSet::upgrade)
            rules: a
                .smart
                .as_ref()
                .and_then(|f| f.rule_set.clone())
                .map(|mut r| {
                    r.upgrade();
                    r
                })
                .unwrap_or_default(),
            parent: None,
        },
        None => Dialog::SmartRules {
            id: None,
            name: p.name.unwrap_or_default(),
            rules: lightcraft_catalog::RuleSet { rules: vec![crate::panels::rules_editor::new_rule()], ..Default::default() },
            parent: p.parent,
        },
    });
    Ok(Value::Null)
}

pub(crate) fn dialog_new_smart_album(app: &mut LightcraftApp, _: &Call, p: NewSmartAlbum) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::NewSmartAlbum { name: p.name.unwrap_or_default(), parent: p.parent });
    Ok(Value::Null)
}

pub(crate) fn dialog_capture_time(app: &mut LightcraftApp, _: &Call, p: CaptureTime) -> Result<Value, String> {
    let time = app.session.active().and_then(|id| app.session.catalog.photo(id)).map(|p| p.date().replace('T', " ")).unwrap_or_default();
    let mode = p.mode.unwrap_or_else(|| "set".into());
    app.ui.dialog = Some(Dialog::CaptureTime { mode, time: time.get(..19).unwrap_or(&time).to_string(), days: 0, hours: 0, minutes: 0, zone: 0.0 });
    Ok(Value::Null)
}

pub(crate) fn dialog_label_names(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let names = lightcraft_catalog::ColorLabel::ALL.iter().map(|l| app.session.catalog.custom_label_name(*l).unwrap_or("").to_string()).collect();
    app.ui.dialog = Some(Dialog::LabelNames { names, save_as: String::new() });
    Ok(Value::Null)
}

pub(crate) fn dialog_rename(app: &mut LightcraftApp, _: &Call, p: Rename) -> Result<Value, String> {
    let template = p.template.unwrap_or_else(|| "{name}".into());
    app.ui.dialog = Some(Dialog::Rename { template, start: p.start.unwrap_or(1) });
    Ok(Value::Null)
}

pub(crate) fn dialog_create_preset(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::create_preset());
    Ok(Value::Null)
}

pub(crate) fn dialog_paste_settings(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let groups = app.session.copy_groups.iter().filter_map(|g| serde_json::to_value(g).ok().and_then(|v| v.as_str().map(str::to_string))).collect();
    app.ui.dialog = Some(Dialog::PasteSettings { groups });
    Ok(Value::Null)
}

pub(crate) fn dialog_copy_settings(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let groups = app.session.copy_groups.iter().filter_map(|g| serde_json::to_value(g).ok().and_then(|v| v.as_str().map(str::to_string))).collect();
    app.ui.dialog = Some(Dialog::CopySettings { groups });
    Ok(Value::Null)
}

pub(crate) fn dialog_slideshow_export(app: &mut LightcraftApp, _: &Call, p: SlideshowExport) -> Result<Value, String> {
    use lightcraft_engine::slideshow_export::{ExportSettings, Format};
    let format = match p.format.as_deref() {
        Some("jpeg") => Format::Jpeg,
        Some("video") => Format::Video,
        None | Some("pdf") => Format::Pdf,
        Some(f) => return Err(format!("dialog.slideshowExport: unknown format {f:?} (pdf|jpeg|video)")),
    };
    app.ui.dialog = Some(Dialog::SlideshowExport { settings: ExportSettings::for_format(format) });
    Ok(Value::Null)
}

pub(crate) fn dialog_contact_sheet(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::ContactSheet { options: lightcraft_engine::contact_sheet::Options::default() });
    Ok(Value::Null)
}

pub(crate) fn dialog_print(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.print = Default::default();
    app.ui.dialog = Some(Dialog::Print);
    Ok(Value::Null)
}

pub(crate) fn dialog_export(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    let prev = app.session.last_export.clone().unwrap_or_default();
    let u = |k: &str, d: u64| prev.get(k).and_then(Value::as_u64).unwrap_or(d);
    let dir = prev.get("dir").and_then(Value::as_str).map(str::to_string).unwrap_or_else(crate::control::default_export_dir);
    let opts = lightcraft_engine::export::ExportOptions::from_json(&prev);
    // no previous export: 2048 px long edge; a previous full-size export: full size
    let full_size = opts.resize.is_none() && lightcraft_engine::export::ExportOptions::has_size_param(&prev);
    let resize = opts.resize.unwrap_or_default();
    app.ui.dialog = Some(Dialog::Export { opts, full_size, resize, preset_name: String::new(), limit_kb: u("limitKb", 0) as u32, dir });
    Ok(Value::Null)
}

pub(crate) fn dialog_merge_hdr(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    crate::merge::open(app, "merge.hdr")
}

pub(crate) fn dialog_face_model(app: &mut LightcraftApp, _: &Call, p: FaceModel) -> Result<Value, String> {
    let path = match p.path {
        Some(x) => Some(x),
        None => app.services.pick_model_file.as_mut().and_then(|f| f().into_iter().next()),
    };
    match path {
        Some(path) => crate::panels::faces::open_dialog(app, &path),
        None => Ok(Value::Null),
    }
}

pub(crate) fn dialog_merge_panorama(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    crate::merge::open(app, "merge.panorama")
}

pub(crate) fn dialog_merge_hdr_panorama(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    crate::merge::open(app, "merge.hdrPanorama")
}

pub(crate) fn dialog_cull(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::Cull { reject_below: 0.0, pick_best: true });
    Ok(Value::Null)
}

pub(crate) fn dialog_save_metadata_preset(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    // from the active photo's copyright, creator and place
    crate::panels::dialogs::prompt(app, "Save Metadata Preset", "Preset name", "", "metadata.savePreset", json!({}), "name");
    Ok(Value::Null)
}

pub(crate) fn dialog_save_mask_effect(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    if app.session.active_mask.is_none() {
        return Err("select a mask first".into());
    }
    crate::panels::mask_effects::open_save(app);
    Ok(Value::Null)
}

pub(crate) fn dialog_manage_mask_effects(app: &mut LightcraftApp, _: &Call, _: NoParams) -> Result<Value, String> {
    crate::panels::mask_effects::open_manage(app);
    Ok(Value::Null)
}

lightcraft_engine::params! {
    /// `dialog.newAlbum` (`includeSelected`, default true) / `dialog.newFolder`: the name and folder to start with.
    pub(crate) struct NewAlbum {
        name: Option<String>,
        parent: Option<u64>,
        include_selected: Option<bool>,
    }
}

lightcraft_engine::params! {
    /// `dialog.newSmartAlbum`
    pub(crate) struct NewSmartAlbum {
        name: Option<String>,
        parent: Option<u64>,
    }
}

lightcraft_engine::params! {
    /// `dialog.smartAlbum`: `id` edits that smart album; without, a new one (`name`, `parent`).
    pub(crate) struct SmartAlbum {
        id: Option<u64>,
        name: Option<String>,
        parent: Option<u64>,
    }
}

lightcraft_engine::params! {
    /// `dialog.autoStack`
    pub(crate) struct AutoStack {
        gap: Option<f64>,
    }
}

lightcraft_engine::params! {
    /// `dialog.allMetadata`
    pub(crate) struct OnePhoto {
        id: Option<lightcraft_catalog::PhotoId>,
    }
}

lightcraft_engine::params! {
    /// `dialog.captureTime`
    pub(crate) struct CaptureTime {
        mode: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `dialog.rename`
    pub(crate) struct Rename {
        template: Option<String>,
        start: Option<u32>,
    }
}

lightcraft_engine::params! {
    /// `dialog.slideshowExport`
    pub(crate) struct SlideshowExport {
        format: Option<String>,
    }
}

lightcraft_engine::params! {
    /// `dialog.faceModel`
    pub(crate) struct FaceModel {
        path: Option<String>,
    }
}
