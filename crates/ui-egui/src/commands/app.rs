//! UI commands for files, the library, exports, merges, links and the app itself.

use serde_json::{Value, json};

use crate::LightcraftApp;
use crate::menus::*;
use crate::pick::{PickRequest, Picked};
use crate::state::Dialog;

pub(crate) fn library_select_only_active(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    let Some(active) = app.session.active() else { return Err("no active photo".into()) };
    app.run("library.select", json!({"ids": [active.0], "active": active.0}))
}

pub(crate) fn photo_set_reference(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let id = p.get("id").and_then(Value::as_u64).or_else(|| app.session.active().map(|a| a.0));
    app.ui.reference = id;
    Ok(json!({"reference": id}))
}

pub(crate) fn app_settings(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let tab = p.get("tab").and_then(Value::as_str).unwrap_or("general");
    if !crate::panels::settings::TABS.iter().any(|(id, _)| *id == tab) {
        let tabs: Vec<&str> = crate::panels::settings::TABS.iter().map(|(id, _)| *id).collect();
        return Err(format!("unknown settings tab `{tab}` ({})", tabs.join("|")));
    }
    app.ui.dialog = Some(Dialog::Settings { tab: tab.into() });
    Ok(Value::Null)
}

pub(crate) fn app_open_library(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    crate::panels::settings::open_library(app, p)
}

pub(crate) fn app_new_library(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    crate::panels::settings::new_library(app, p)
}

pub(crate) fn app_recent_libraries(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    crate::panels::settings::recent_libraries(app, p)
}

pub(crate) fn app_clear_recent_libraries(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::panels::settings::recent_libraries(app, &json!({"clear": true}))
}

pub(crate) fn app_display_profile(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    crate::panels::settings::display_profile(app, p)
}

pub(crate) fn file_backup_library(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let action = if id == "file.backupLibrary" { app.services.backup_library.as_mut() } else { app.services.restore_library.as_mut() };
    match action {
        Some(f) => f(&mut app.session),
        None => Err("not available here: on the desktop the library is a folder; back it up with your other files".into()),
    }
}

pub(crate) fn app_slideshow_export(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    crate::slideshow_export::start(app, id, p)
}

pub(crate) fn app_print(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    if app.export.is_some() {
        return Err("an export is already running".into());
    }
    let mut params = p.clone();
    if p.get("path").is_none() {
        let req = PickRequest::save("Print to PDF", "PDF", &["pdf"], "Print.pdf");
        let paths = match crate::pick::ask(app, id, p, "path", req, |_| None) {
            Picked::Now(paths) => paths,
            Picked::Later => return Ok(Value::Null),
            Picked::Unavailable => return Err("no save dialog on this platform; supply path".into()),
        };
        let Some(path) = paths.first() else { return Ok(Value::Null) };
        params["path"] = json!(path);
    }
    crate::export_task::start_print(app, &params)
}

pub(crate) fn app_contact_sheet(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    if app.export.is_some() {
        return Err("an export is already running".into());
    }
    let mut params = p.clone();
    if p.get("path").is_none() {
        let req = PickRequest::save("Contact Sheet PDF", "PDF", &["pdf"], "Contact Sheet.pdf");
        let paths = match crate::pick::ask(app, id, p, "path", req, |_| None) {
            Picked::Now(paths) => paths,
            Picked::Later => return Ok(Value::Null),
            Picked::Unavailable => return Err("no save dialog on this platform; supply path".into()),
        };
        let Some(path) = paths.first() else { return Ok(Value::Null) };
        params["path"] = json!(path);
    }
    crate::export_task::start_contact_sheet(app, &params)
}

pub(crate) fn merge_hdr_last(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::merge::start_last(app, "merge.hdr")
}

pub(crate) fn merge_hdr_batch(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    crate::merge::start_batch(app, p)
}

pub(crate) fn merge_panorama_last(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::merge::start_last(app, "merge.panorama")
}

pub(crate) fn merge_hdr_panorama_last(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::merge::start_last(app, "merge.hdrPanorama")
}

pub(crate) fn app_open_sam_model_folder(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    crate::panels::dialogs::show_model_folder(app)
}

pub(crate) fn app_about(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::About);
    Ok(Value::Null)
}

pub(crate) fn photo_edit_in_photo_craft(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    edit_in(app, &ctx, p, true)
}

pub(crate) fn photo_edit_in_external(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    edit_in(app, &ctx, p, false)
}

pub(crate) fn photo_open_original_in_external(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    open_original_in(app, &ctx, p)
}

pub(crate) fn app_whats_new(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::WhatsNew);
    Ok(json!({"text": crate::panels::dialogs::WHATS_NEW}))
}

pub(crate) fn app_system_info(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let info = app.session.execute("library.info", &json!({})).unwrap_or_default();
    let gpu = (lightcraft_engine::gpu::ready() && lightcraft_engine::gpu::available()).then(lightcraft_engine::gpu::adapter_name).flatten();
    let mb = |b: u64| format!("{:.0} MB", b as f64 / (1u64 << 20) as f64);
    let mut rows = vec![
        ("Version".to_string(), env!("CARGO_PKG_VERSION").to_string()),
        ("System".to_string(), format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH)),
        ("CPU threads".to_string(), std::thread::available_parallelism().map(|n| n.get().to_string()).unwrap_or_else(|_| "?".into())),
        (
            "GPU".to_string(),
            gpu.unwrap_or_else(|| match lightcraft_engine::gpu::unavailable_reason() {
                Some(why) => crate::i18n::tr_format!("none (CPU rendering): {why}", why = why),
                None => "none (CPU rendering)".into(),
            }),
        ),
        ("GPU rendering".to_string(), if app.ui.settings.gpu { "on".into() } else { "off".into() }),
        ("Memory budget".to_string(), mb(lightcraft_engine::memory::default_budget() as u64)),
        (
            "Preview size".to_string(),
            match app.ui.settings.preview_limit {
                0 => "automatic".to_string(),
                n => format!("{n} px"),
            },
        ),
        ("Photos".to_string(), info["photos"].to_string()),
        ("Albums".to_string(), info["albums"].to_string()),
    ];
    if let Some(dir) = info["dir"].as_str().or(info["path"].as_str()) {
        rows.push(("Library".into(), dir.to_string()));
    }
    rows.push(("Frame time".into(), format!("{:.1} ms ({:.0} fps)", app.perf.frame_ms, app.perf.fps)));
    rows.push((
        "Frame update".into(),
        format!("{:.1} ms (logic {:.1} ms; slowest {:.0} ms)", app.perf.update_ms, app.perf.logic_ms, app.perf.max_update_ms),
    ));
    rows.push(("Last loupe render".into(), format!("{:.0} ms", app.renderer.last_main_ms)));
    if let Some(f) = lightcraft_engine::gpu::last_fallback() {
        rows.push(("Last GPU fallback".into(), f));
    }
    let r = json!(rows.iter().map(|(k, v)| json!({"label": k, "value": v})).collect::<Vec<_>>());
    if p.get("open").and_then(Value::as_bool).unwrap_or(true) {
        app.ui.dialog = Some(Dialog::SystemInfo { rows });
    }
    Ok(r)
}

pub(crate) fn app_shortcuts(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.dialog = Some(Dialog::Shortcuts);
    Ok(Value::Null)
}

pub(crate) fn app_set_shortcut(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    crate::shortcuts::set_shortcut(app, p)
}

pub(crate) fn app_reset_shortcuts(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.settings.keymap.clear();
    Ok(Value::Null)
}

pub(crate) fn library_browse(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    if crate::lightroom_import::is_running(app) {
        return Err("finish Lightroom catalog import before browsing folders".into());
    }
    // listed and read in the background (see `import::browse`)
    let path = p.get("path").and_then(Value::as_str).ok_or("library.browse: missing `path`")?;
    crate::import::browse(app, path, p.get("subfolders").and_then(Value::as_bool))
}

pub(crate) fn file_add_photos(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    if crate::lightroom_import::is_running(app) {
        return Err("finish Lightroom catalog import before adding photos".into());
    }
    let paths = match p.get("paths").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        None => {
            let req = PickRequest::files(None, crate::i18n::tr("Photos"), lightcraft_engine::import::EXTENSIONS);
            match crate::pick::ask(app, id, p, "paths", req, |s| s.pick_files.as_mut().map(|f| f())) {
                Picked::Now(paths) => paths,
                Picked::Later | Picked::Unavailable => return Ok(Value::Null),
            }
        }
    };
    if paths.is_empty() {
        return Ok(Value::Null);
    }
    // review first: the import dialog lists what was found
    let r = crate::import::open(app, paths);
    import_into_album(app, p, r)
}

pub(crate) fn file_import_lightroom(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let path = match p.get("path").and_then(Value::as_str).map(str::to_string) {
        Some(path) => Some(path),
        None => {
            let req = PickRequest::file(crate::i18n::tr("Import Lightroom Catalog"), crate::i18n::tr("Lightroom Classic Catalog"), &["lrcat"]);
            match crate::pick::ask(app, id, p, "path", req, |s| s.pick_lightroom_catalog.as_mut().and_then(|f| f()).map(|x| vec![x])) {
                Picked::Now(v) => v.into_iter().next(),
                Picked::Later | Picked::Unavailable => None,
            }
        }
    };
    match path {
        Some(path) => app.run("library.importLightroom", json!({"path":path})),
        None => Ok(Value::Null),
    }
}

pub(crate) fn app_quit(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    app.ui.quit = true;
    Ok(Value::Null)
}

pub(crate) fn file_find_missing(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let within = match p.get("within") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) if !path.is_empty() => Some(path.clone()),
        Some(Value::String(_)) => return Err("file.findMissing: `within` must not be empty".into()),
        Some(value) => return Err(format!("file.findMissing: `within` must be a folder path, not {value}")),
    };
    let wait = p.get("wait").and_then(Value::as_bool).unwrap_or(false);
    let timeout = match p.get("timeout") {
        None | Some(Value::Null) => std::time::Duration::from_secs(600),
        Some(Value::Number(n)) => {
            if let Some(s) = n.as_f64() {
                if s.is_finite() && s >= 0.0 {
                    match std::time::Duration::try_from_secs_f64(s) {
                        Ok(d) => d,
                        Err(_) => return Err("file.findMissing: `timeout` is too large".into()),
                    }
                } else {
                    return Err("file.findMissing: `timeout` must be a non-negative number of seconds".into());
                }
            } else {
                return Err("file.findMissing: `timeout` must be a number of seconds".into());
            }
        }
        Some(other) => return Err(format!("file.findMissing: `timeout` must be a number of seconds, not {other}")),
    };
    let title = if within.is_some() { "Locate Missing Files…" } else { "Find Missing Photos…" };
    let folder = match p.get("folder").and_then(Value::as_str) {
        Some(f) => Some(f.to_string()),
        None => match crate::pick::ask(app, id, p, "folder", PickRequest::folder(crate::i18n::tr(title)), |s| {
            s.pick_folder.as_mut().and_then(|f| f()).map(|x| vec![x])
        }) {
            Picked::Now(v) => v.into_iter().next(),
            Picked::Later | Picked::Unavailable => None,
        },
    };
    let Some(folder) = folder else { return Ok(Value::Null) };
    // the search (checking every photo's file, walking the folder) runs on a worker thread;
    // the relinking happens back here, as one undo step
    const LABEL: &str = "Find Missing Photos";
    if app.tasks.is_running(LABEL) {
        return Err("Find Missing Photos is already searching".into());
    }
    let mut candidates = lightcraft_engine::cmd::missing::find_candidates(&app.session.catalog);
    if let Some(within) = within {
        candidates.retain(|candidate| lightcraft_catalog::query::folder_within(&candidate.path, &within));
    }
    #[cfg(test)]
    let test_delay = app.ui.find_missing_test_delay;
    let work = move || {
        #[cfg(test)]
        if let Some(d) = test_delay {
            std::thread::sleep(d);
        }
        lightcraft_engine::cmd::missing::plan_find_missing(&candidates, &folder)
    };
    let done = |app: &mut LightcraftApp, ctx: &egui::Context, plan: Result<lightcraft_engine::cmd::missing::FindPlan, String>| {
        // relinked under the session as it is now: photos relinked meanwhile and files
        // now in use are skipped
        let r = plan.and_then(|plan| app.run("library.findMissing", plan.to_json()));
        match r {
            Ok(v) => {
                let n = v["found"].as_array().map_or(0, Vec::len);
                let left = v["missing"].as_u64().unwrap_or(0);
                let unsure = v["ambiguous"].as_array().map_or(0, Vec::len);
                let unsure = if unsure > 0 {
                    crate::i18n::tr_format!(" ({unsure} with several look-alike files: use Locate)", unsure = unsure)
                } else {
                    String::new()
                };
                app.toast(
                    ctx,
                    crate::i18n::tr_format!(
                        "Found {n} missing photo{}; {left} still missing{unsure}",
                        if n == 1 { "" } else { "s" },
                        left = left,
                        n = n,
                        unsure = unsure
                    ),
                );
                app.ui.last_find_missing = Some(v.clone());
                app.ui.last_find_missing_result = Some(Ok(v));
            }
            Err(e) => {
                app.toast(ctx, e.clone());
                app.ui.last_find_missing_result = Some(Err(e));
            }
        }
    };
    app.ui.last_find_missing = None;
    app.ui.last_find_missing_result = None;
    crate::tasks::spawn(app, LABEL, Some("findMissing"), work, done)?;
    if wait {
        let ctx = app.tasks.repaint.clone().unwrap_or_default();
        let completed = crate::tasks::wait_for(app, &ctx, LABEL, timeout);
        if !completed {
            return Err("Find Missing Photos: search timed out (search continues in the background)".into());
        }
        return app.ui.last_find_missing_result.take().unwrap_or_else(|| Err("Find Missing Photos: worker terminated unexpectedly".into()));
    }
    Ok(json!({"background": true}))
}

pub(crate) fn photo_tag_from_tracklog(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    // a GPX file → GPS for the selected photos by capture time (one undo step)
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => Some(x.to_string()),
        None => {
            let req = PickRequest::file(crate::i18n::tr("Auto-Tag from Tracklog"), crate::i18n::tr("GPS Track Log"), &["gpx"]);
            match crate::pick::ask(app, id, p, "path", req, |s| s.pick_tracklog.as_mut().map(|f| f())) {
                Picked::Now(v) => v.into_iter().next(),
                Picked::Later | Picked::Unavailable => None,
            }
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = p.as_object().cloned().unwrap_or_default();
    params.insert("path".into(), json!(path));
    let ask_zone = !params.contains_key("offset");
    let params = Value::Object(params);
    if ask_zone {
        // GPX times are UTC, camera clocks are local: ask for the camera's zone, then come back here
        crate::panels::dialogs::prompt(
            app,
            "Auto-Tag from Tracklog",
            "Camera time zone, e.g. -07:00 (empty: UTC)",
            "",
            "photo.tagFromTracklog",
            params,
            "offset",
        );
        return Ok(Value::Null);
    }
    let r = app.run("photo.autoTagTracklog", params);
    if let Ok(v) = &r {
        let n = v["tagged"].as_u64().unwrap_or(0);
        let sk = &v["skipped"];
        let mut msg = crate::i18n::tr_format!("Tagged {n} photo{} from the tracklog", if n == 1 { "" } else { "s" }, n = n);
        let outside = sk["outside"].as_u64().unwrap_or(0);
        if outside > 0 {
            msg += &crate::i18n::tr_format!("; {outside} outside its time range", outside = outside);
        }
        let kept = sk["hasGps"].as_u64().unwrap_or(0);
        if kept > 0 {
            msg += &crate::i18n::tr_format!("; {kept} already had a location", kept = kept);
        }
        let ctx = app.tasks.repaint.clone().unwrap_or_default();
        app.toast(&ctx, msg);
    }
    r
}

pub(crate) fn photo_locate(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    let Some(id) = app.session.active() else { return Err("no photo selected".into()) };
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => Some(x.to_string()),
        None => {
            let req = PickRequest::file(crate::i18n::tr("Locate Missing File…"), crate::i18n::tr("Photos"), lightcraft_engine::import::EXTENSIONS);
            match crate::pick::ask(app, "photo.locate", p, "path", req, |s| s.pick_files.as_mut().map(|f| f())) {
                Picked::Now(v) => v.into_iter().next(),
                Picked::Later | Picked::Unavailable => None,
            }
        }
    };
    match path {
        Some(path) => app.run("photo.relink", json!({"id": id.0, "path": path})),
        None => Ok(Value::Null),
    }
}

pub(crate) fn file_add_from_device(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    // a camera / card: review its DCIM folder, copying into the library by default
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => x.to_string(),
        None => match lightcraft_engine::devices::devices_now().into_iter().next() {
            Some(d) => d.path,
            None => return Err("no camera or memory card found".into()),
        },
    };
    let r = crate::import::open(app, vec![path]);
    // only the scan just started (an error means another scan is running)
    if r.is_ok()
        && let Some(t) = &mut app.scan
    {
        t.copy = true;
    }
    import_into_album(app, p, r)
}

pub(crate) fn file_add_folder(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    // a folder (searched recursively) into the import review
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => Some(x.to_string()),
        None => match crate::pick::ask(app, id, p, "path", PickRequest::folder(crate::i18n::tr("Import from Folder…")), |s| {
            s.pick_folder.as_mut().and_then(|f| f()).map(|x| vec![x])
        }) {
            Picked::Now(v) => v.into_iter().next(),
            Picked::Later | Picked::Unavailable => None,
        },
    };
    match path {
        Some(path) => {
            let r = crate::import::open(app, vec![path]);
            import_into_album(app, p, r)
        }
        None => Ok(Value::Null),
    }
}

pub(crate) fn file_import_presets(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let paths = match p.get("paths").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        None => {
            let req = PickRequest::files(
                Some(crate::i18n::tr("Import Presets").into()),
                crate::i18n::tr("Presets & Profiles"),
                crate::pick::PRESET_EXTENSIONS,
            );
            match crate::pick::ask(app, id, p, "paths", req, |s| s.pick_preset_files.as_mut().map(|f| f())) {
                Picked::Now(paths) => paths,
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform".into()),
            }
        }
    };
    if paths.is_empty() {
        return Ok(Value::Null);
    }
    // .cube LUTs (and those inside zips / folders) become profiles
    let lut_paths: Vec<&String> = paths
        .iter()
        .filter(|p| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".cube") || l.ends_with(".zip") || std::path::Path::new(p.as_str()).is_dir()
        })
        .collect();
    // one toast for both halves (a second toast would replace the first)
    let mut parts: Vec<String> = Vec::new();
    // `name: reason` for each file that couldn't be imported
    let mut problems: Vec<String> = Vec::new();
    let mut profiles_result = Value::Null;
    if !lut_paths.is_empty() {
        match app.session.execute("profile.import", &json!({"paths": lut_paths})) {
            Ok(v) => {
                let profiles = v["imported"].as_array().map_or(0, Vec::len);
                if profiles > 0 {
                    parts.push(crate::i18n::tr_format!(
                        "Imported {profiles} profile{} (Profile browser ▸ their groups)",
                        if profiles == 1 { "" } else { "s" },
                        profiles = profiles
                    ));
                }
                problems.extend(v["failed"].as_array().into_iter().flatten().map(|f| {
                    let name = f.get(0).and_then(Value::as_str).unwrap_or_default();
                    let reason = f.get(1).and_then(Value::as_str).unwrap_or_default();
                    format!("{name}: {reason}")
                }));
                profiles_result = v;
            }
            Err(e) => {
                problems.push(e.to_string());
                profiles_result = json!({"error": e.to_string()});
            }
        }
    }
    let preset_paths: Vec<&String> = paths.iter().filter(|p| !p.to_ascii_lowercase().ends_with(".cube")).collect();
    let r = if preset_paths.is_empty() {
        Ok(json!({"imported": [], "failed": [], "skipped": 0}))
    } else {
        app.session.execute("preset.import", &json!({"paths": preset_paths})).map_err(|e| e.to_string())
    };
    if let (false, Ok(v)) = (preset_paths.is_empty(), &r) {
        let n = v["imported"].as_array().map_or(0, Vec::len);
        let failed = v["failed"].as_array().map_or(0, Vec::len);
        // Lightroom local adjustment presets land in the Masking panel's Effect menu: part of the same toast
        let effects = v["effects"].as_array().map_or(0, Vec::len);
        if effects > 0 {
            parts.push(crate::panels::mask_effects::import_toast(v));
        }
        let profile_news = !parts.is_empty() || !problems.is_empty();
        let msg = match (n, failed) {
            // a zip of LUTs (or only local presets): the other half already says what happened
            (0, 0) if profile_news => None,
            (0, 0) => Some("No new presets".to_string()),
            (n, 0) => Some(crate::i18n::tr_format!("Imported {n} preset{}", if n == 1 { "" } else { "s" }, n = n)),
            (n, f) => Some(crate::i18n::tr_format!(
                "Imported {n} preset{}, {f} file{} not readable",
                if n == 1 { "" } else { "s" },
                if f == 1 { "" } else { "s" },
                f = f,
                n = n
            )),
        };
        if let Some(mut msg) = msg {
            // settings with no counterpart here (the other editor's profiles, masks…)
            let mut skipped: Vec<&str> = v["imported"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|i| i["unmapped"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .collect();
            skipped.sort_unstable();
            skipped.dedup();
            if !skipped.is_empty() {
                let names: Vec<&str> = skipped.iter().take(3).copied().collect();
                msg += &crate::i18n::tr_format!(" — not carried over: {}{}", names.join(", "), if skipped.len() > 3 { "…" } else { "" });
            }
            parts.push(msg);
        }
        if n > 0 {
            app.ui.presets = true;
        }
    }
    let mut msg = parts.join(" · ");
    if !problems.is_empty() {
        let detail = format!("{}{}", problems.iter().take(2).cloned().collect::<Vec<_>>().join("; "), if problems.len() > 2 { "…" } else { "" });
        if msg.is_empty() {
            msg = crate::i18n::tr_format!("Import failed: {e}", e = detail);
        } else {
            msg += &crate::i18n::tr_format!(" · {} failed: {}", problems.len(), detail);
        }
        app.toast_error(&ctx, msg);
    } else if !msg.is_empty() {
        app.toast(&ctx, msg);
    }

    r.map(|mut v| {
        if !lut_paths.is_empty()
            && let Some(o) = v.as_object_mut()
        {
            o.insert("profiles".into(), profiles_result);
        }
        v
    })
}

pub(crate) fn file_import_keywords(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    // a keyword list file: Lightroom Classic's, Capture One's, Photo Supreme's (.utf8)
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => x.to_string(),
        None => {
            let req = PickRequest::file(crate::i18n::tr("Import Keywords"), crate::i18n::tr("Keyword Lists"), &["txt", "utf8"]);
            match crate::pick::ask(app, id, p, "path", req, |s| s.pick_keyword_list.as_mut().map(|f| f())) {
                Picked::Now(v) => match v.into_iter().next() {
                    Some(x) => x,
                    None => return Ok(Value::Null),
                },
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform".into()),
            }
        }
    };
    let r = app.session.execute("keyword.import", &json!({"path": path})).map_err(|e| e.to_string());
    match &r {
        Ok(v) => {
            let (added, updated) = (v["added"].as_u64().unwrap_or(0), v["updated"].as_u64().unwrap_or(0));
            let mut msg = crate::i18n::tr_format!("Added {n} keyword{}", if added == 1 { "" } else { "s" }, n = added);
            if updated > 0 {
                msg.push_str(&crate::i18n::tr_format!("; {n} gained synonyms", n = updated));
            }
            app.toast(&ctx, msg);
        }
        Err(e) => app.toast(&ctx, e.clone()),
    }
    r
}

pub(crate) fn file_export_keywords(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => x.to_string(),
        None => {
            let name = "Keywords.txt";
            let req = PickRequest::save(crate::i18n::tr("Export Keywords"), crate::i18n::tr("Keyword Lists"), &["txt"], name);
            match crate::pick::ask(app, id, p, "path", req, |s| s.save_keyword_list.as_mut().map(|f| f(name).into_iter().collect())) {
                Picked::Now(v) => match v.into_iter().next() {
                    Some(x) => x,
                    None => return Ok(Value::Null),
                },
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform".into()),
            }
        }
    };
    let r = app.session.execute("keyword.export", &json!({"path": path})).map_err(|e| e.to_string());
    if let Err(e) = &r {
        app.toast(&ctx, e.clone());
    }
    if let Ok(v) = &r {
        let n = v["keywords"].as_u64().unwrap_or(0);
        let mut msg = crate::i18n::tr_format!("Exported {n} keyword{}", if n == 1 { "" } else { "s" }, n = n);
        // Capture One's importer refuses ; , < > in a list: say which to rename first
        let refuses: Vec<&str> = v["captureOneRefuses"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if !refuses.is_empty() {
            msg.push_str(&crate::i18n::tr_format!(
                " — Capture One won't import a list with ; , < or > in a keyword: {names}",
                names = refuses.iter().take(5).copied().collect::<Vec<_>>().join(" · ")
            ));
        }
        // what the format can't hold (a name in brackets, a line break…) isn't in the file
        let left: Vec<&str> = v["unwritable"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if !left.is_empty() {
            msg.push_str(&crate::i18n::tr_format!(
                " — left out, as a keyword list can't hold them: {names}",
                // escaped: a line break in a name would break the toast
                names = left.iter().take(5).map(|n| n.escape_debug().to_string()).collect::<Vec<_>>().join(" · ")
            ));
        }
        app.toast(&ctx, msg);
    }
    r
}

pub(crate) fn file_export_mask_effects(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => Some(x.to_string()),
        None => {
            let name = format!("{}.lceffect", p.get("group").and_then(Value::as_str).unwrap_or("Effect Presets"));
            let req = PickRequest::save(
                crate::i18n::tr("Export Effect Presets"),
                crate::i18n::tr("LightCraft Effect Presets"),
                &["lceffect"],
                name.clone(),
            );
            match crate::pick::ask(app, id, p, "path", req, |s| s.save_preset_file.as_mut().map(|f| f(&name).into_iter().collect())) {
                Picked::Now(v) => v.into_iter().next(),
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform".into()),
            }
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = json!({"path": path});
    for k in ["names", "group"] {
        if let Some(v) = p.get(k) {
            params[k] = v.clone();
        }
    }
    let r = app.session.execute("mask.exportEffects", &params).map_err(|e| e.to_string());
    match &r {
        Ok(v) => app.toast(&ctx, crate::i18n::tr_format!("Exported {} effect preset{}", v["count"], if v["count"] == 1 { "" } else { "s" })),
        Err(e) => app.toast_error(&ctx, e.clone()),
    }
    r
}

pub(crate) fn file_export_presets(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let group = p.get("group").and_then(Value::as_str).map(str::to_string);
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => Some(x.to_string()),
        None => {
            let name = format!("{}.lcpreset", group.as_deref().unwrap_or("LightCraft Presets"));
            let req = PickRequest::save(crate::i18n::tr("Export Presets"), crate::i18n::tr("LightCraft Preset"), &["lcpreset"], name.clone());
            match crate::pick::ask(app, id, p, "path", req, |s| s.save_preset_file.as_mut().map(|f| f(&name).into_iter().collect())) {
                Picked::Now(v) => v.into_iter().next(),
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform".into()),
            }
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = json!({"path": path});
    if let Some(g) = group {
        params["group"] = json!(g);
    }
    if let Some(ids) = p.get("ids") {
        params["ids"] = ids.clone();
    }
    let r = app.session.execute("preset.export", &params).map_err(|e| e.to_string());
    if let Ok(v) = &r {
        app.toast(&ctx, crate::i18n::tr_format!("Exported {} preset{}", v["count"], if v["count"] == 1 { "" } else { "s" }));
    }
    r
}

pub(crate) fn file_import_curve_presets(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let paths: Vec<String> = match p.get("paths").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        None => {
            let req = PickRequest::files(
                Some(crate::i18n::tr("Import Point Curve Presets").into()),
                crate::i18n::tr("Point Curve Presets"),
                crate::pick::CURVE_PRESET_EXTENSIONS,
            );
            match crate::pick::ask(app, id, p, "paths", req, |s| s.pick_curve_preset_files.as_mut().map(|f| f())) {
                Picked::Now(paths) => paths,
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform".into()),
            }
        }
    };
    if paths.is_empty() {
        return Ok(Value::Null);
    }
    let r = app.session.execute("curve.importPresets", &json!({"paths": paths})).map_err(|e| e.to_string());
    if let Ok(v) = &r {
        let n = v["imported"].as_array().map_or(0, Vec::len);
        let failed = v["failed"].as_array().map_or(0, Vec::len);
        let mut msg = crate::i18n::tr_format!("Imported {n} point curve preset{}", if n == 1 { "" } else { "s" }, n = n);
        if failed > 0 {
            msg += &crate::i18n::tr_format!(", {failed} file{} not readable", if failed == 1 { "" } else { "s" }, failed = failed);
        }
        app.toast(&ctx, msg);
    }
    r
}

pub(crate) fn file_export_curve_presets(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let p = c.params;
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    let path = match p.get("path").and_then(Value::as_str) {
        Some(x) => Some(x.to_string()),
        None => {
            let req = PickRequest::save(
                crate::i18n::tr("Export Point Curve Presets"),
                crate::i18n::tr("Point Curve Presets"),
                &["lccurve"],
                "Point Curves.lccurve",
            );
            match crate::pick::ask(app, id, p, "path", req, |s| {
                s.save_curve_preset_file.as_mut().map(|f| f("Point Curves.lccurve").into_iter().collect())
            }) {
                Picked::Now(v) => v.into_iter().next(),
                Picked::Later => return Ok(Value::Null),
                Picked::Unavailable => return Err("no file dialog on this platform".into()),
            }
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = json!({"path": path});
    if let Some(n) = p.get("names") {
        params["names"] = n.clone();
    }
    let r = app.session.execute("curve.exportPresets", &params).map_err(|e| e.to_string());
    if let Ok(v) = &r {
        app.toast(&ctx, crate::i18n::tr_format!("Exported {} point curve preset{}", v["count"], if v["count"] == 1 { "" } else { "s" }));
    }
    r
}

pub(crate) fn app_export(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let p = c.params;
    crate::control::export_active(app, p)
}

pub(crate) fn app_show_in_finder(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    show_in_finder(app)
}

pub(crate) fn app_open_log_folder(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    open_log_folder(app)
}

pub(crate) fn app_discord(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let id = c.id;
    let url = crate::links::url_of(id).unwrap_or(crate::links::WEBSITE);
    crate::links::open(app, url)
}

pub(crate) fn app_export_previous(app: &mut LightcraftApp, _: &Call) -> Result<Value, String> {
    match app.session.last_export.clone() {
        Some(prev) => crate::control::export_active(app, &lightcraft_engine::export::ExportOptions::known_keys_only(&prev)),
        None => Err("nothing exported yet — use Export…".into()),
    }
}

/// `app.language.*`: the language of the menu command.
pub(crate) fn language(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    let language = language_from_command(c.id).ok_or_else(|| format!("{}: no such language", c.id))?;
    app.ui.language = language;
    // Immediately, not on the next frame: the reply and anything else run this frame
    // (menus rebuilt from it, toasts) are already in the new language.
    crate::i18n::set_language(app.ui.language);
    Ok(json!(app.ui.language))
}

/// `library.inspectLightroom` / `library.importLightroom`, run by the app (in the background, with its dialogs).
pub(crate) fn lightroom(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    // the app's own context: a fresh one's repaints reach no window and its clock starts at zero
    let ctx = app.tasks.repaint.clone().unwrap_or_default();
    crate::lightroom_import::command(app, c.id, c.params, &ctx)
}

/// `modelSetup.cancel {kind: denoise}`: forget the photo action waiting for a model to be installed.
pub(crate) fn model_setup_cancel(app: &mut LightcraftApp, c: &Call) -> Result<Value, String> {
    if c.params.get("kind").and_then(Value::as_str) != Some("denoise") {
        return Err("modelSetup.cancel: use kind denoise".into());
    }
    app.model_setup.cancel_denoise();
    app.ui.status = crate::i18n::tr("Pending photo action cancelled").into();
    Ok(json!({"cancelled": true}))
}
