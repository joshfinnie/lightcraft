//! Export presets: built-in ones ([`crate::export::builtin_presets`]) and the user's, saved with the
//! library. `app.export {preset}` (desktop app, MCP, `lightcraft-cli run`) starts from a preset's
//! params; the call's own params override them (see [`Session::export_params`]).

use serde_json::{Value, json};

use super::params::{Named, NoParams, Patch, params};
use super::{CommandSpec, always, bad, cmd};
use crate::export::{ExportPreset, builtin_presets};
use crate::{Result, Session};

impl Session {
    /// Built-in presets followed by the user's.
    pub fn all_export_presets(&self) -> Vec<(ExportPreset, bool)> {
        builtin_presets().into_iter().map(|p| (p, true)).chain(self.export_presets.iter().cloned().map(|p| (p, false))).collect()
    }

    /// `app.export` params with a named `preset` expanded: the preset's params, overridden by the
    /// call's own (`preset` itself removed). Unknown preset names are an error.
    pub fn export_params(&self, p: &Value) -> std::result::Result<Value, String> {
        let Some(name) = p.get("preset").and_then(Value::as_str) else { return Ok(p.clone()) };
        let (preset, _) = self
            .all_export_presets()
            .into_iter()
            .find(|(x, _)| x.name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| format!("unknown export preset `{name}` (see export.presets)"))?;
        // a preset saved by another version may hold keys this one doesn't know: not the caller's typo
        let mut out = crate::export::ExportOptions::known_keys_only(&preset.params);
        if let (Some(o), Some(own)) = (out.as_object_mut(), p.as_object()) {
            for (k, v) in own.iter().filter(|(k, _)| *k != "preset") {
                o.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
    }
}

fn list(s: &Session) -> Value {
    Value::Array(s.all_export_presets().into_iter().map(|(p, builtin)| json!({"name": p.name, "builtin": builtin, "params": p.params})).collect())
}

/// Params that describe a destination or a selection, not the export settings.
const NOT_SETTINGS: &[&str] = &["ids", "path", "dir", "preset"];

params! {
    /// `export.savePreset`: `params` are app.export params, checked by its options.
    struct SavePreset {
        name: Option<String>,
        params: Option<Value>,
    }
}

params! {
    /// `export.checkTarget`
    struct Target {
        path: Option<String>,
    }
}

params! {
    /// `export.addToPhotos`
    #[cfg(not(target_arch = "wasm32"))]
    struct AddToPhotos {
        paths: Option<Vec<String>>,
        album: Option<String>,
        wait: Option<bool>,
    }
}

params! {
    /// `export.photosImports`
    #[cfg(not(target_arch = "wasm32"))]
    struct PhotosImports {
        job: Option<u64>,
    }
}

fn save(s: &mut Session, p: SavePreset) -> Result<Value> {
    const ID: &str = "export.savePreset";
    let name = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(ID, "missing `name`"))?;
    if builtin_presets().iter().any(|b| b.name.eq_ignore_ascii_case(name)) {
        return Err(bad(ID, format!("`{name}` is a built-in preset; choose another name")));
    }
    let mut params = match &p.params {
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return Err(bad(ID, "`params` must be an object of app.export params")),
        None => s.last_export.clone().ok_or_else(|| bad(ID, "no `params` and no previous export to save"))?,
    };
    if let Some(o) = params.as_object_mut() {
        o.retain(|k, _| !NOT_SETTINGS.contains(&k.as_str()));
    }
    // a preset that app.export would refuse is refused here, where the typo is on screen
    crate::export::ExportOptions::validate(ID, &params)?;
    let preset = ExportPreset { name: name.to_string(), params };
    match s.export_presets.iter_mut().find(|x| x.name.eq_ignore_ascii_case(name)) {
        Some(x) => *x = preset,
        None => s.export_presets.push(preset),
    }
    s.save_prefs()?;
    Ok(list(s))
}

fn delete(s: &mut Session, p: Named) -> Result<Value> {
    const ID: &str = "export.deletePreset";
    let name = p.name(ID)?;
    let name = name.as_str();
    let before = s.export_presets.len();
    s.export_presets.retain(|x| !x.name.eq_ignore_ascii_case(name.trim()));
    if s.export_presets.len() == before {
        return Err(bad(ID, format!("no user export preset `{name}`")));
    }
    s.save_prefs()?;
    Ok(list(s))
}

/// `export.addToPhotos`: there is a runner (the desktop app and lightcraft-cli on macOS).
#[cfg(not(target_arch = "wasm32"))]
fn photos_enabled(s: &Session) -> std::result::Result<(), String> {
    match &s.apple_photos {
        Some(_) => Ok(()),
        None => Err(crate::apple_photos::unavailable().into()),
    }
}

/// `export.addToPhotos {paths, album?, wait?}`: add files already on disk to Apple Photos, as a
/// job: here (the default without a window: lightcraft-cli, the MCP server, scripts), or on a
/// worker thread (the default in the desktop app, which answers control requests on its UI
/// thread and must not wait minutes for Photos; see [`Session::apple_photos_background`]).
#[cfg(not(target_arch = "wasm32"))]
fn add_to_photos(s: &mut Session, p: AddToPhotos) -> Result<Value> {
    const ID: &str = "export.addToPhotos";
    let runner = s.apple_photos.clone().ok_or_else(|| bad(ID, crate::apple_photos::unavailable()))?;
    let paths = p.paths.ok_or_else(|| bad(ID, "missing `paths` (an array of absolute file paths)"))?;
    let album = p.album.as_deref().unwrap_or("");
    // a window (UI thread) doesn't wait by default; a one-shot process must, or it would end
    // in the middle of the import
    let wait = match p.wait {
        None => !s.apple_photos_background,
        // the desktop app answers on its window's thread, which never waits for Photos
        Some(true) if s.apple_photos_background => {
            return Err(bad(
                ID,
                "`wait: true` isn't available in the desktop app: the import runs in the background; its outcome comes from export.photosImports {job}",
            ));
        }
        Some(b) => b,
    };
    let job = crate::apple_photos::import_job(&runner, &s.apple_photos_imports, paths, album, !wait, Some(&s.activity)).map_err(|e| bad(ID, e))?;
    match job.outcome() {
        Some(crate::apple_photos::Outcome::Failed(e)) => Err(bad(ID, e)),
        _ => Ok(job.json()),
    }
}

/// `export.photosImports {job?}`: this session's Apple Photos imports, or one of them.
#[cfg(not(target_arch = "wasm32"))]
fn photos_imports(s: &mut Session, p: PhotosImports) -> Result<Value> {
    const ID: &str = "export.photosImports";
    match p.job {
        None => Ok(json!({"imports": s.apple_photos_imports.all().iter().map(|j| j.json()).collect::<Vec<_>>()})),
        Some(id) => s.apple_photos_imports.get(id).map(|j| j.json()).ok_or_else(|| bad(ID, format!("no import {id} (see export.photosImports)"))),
    }
}

pub fn specs() -> Vec<CommandSpec> {
    #[allow(unused_mut)]
    let mut v = vec![
        cmd!(query "export.contactSheet", "Export Contact Sheet PDF", [], None,
            "{path, ids?: photo ids (default selection), paper?: a4|letter, landscape?: bool, columns?: 1..8, rows?: 1..10, captions?: bool} → {path, pages, photos, bytes}; replaces an existing output, never an original",
            Patch, always, |s, p| {
                // (contact_sheet::prepare checks the keys: its options refuse unknown ones)
                let p = p.value();
                let p = &p;
                let path = p.get("path").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or_else(|| bad("export.contactSheet", "missing path"))?;
                s.check_write_target(path).map_err(|e| bad("export.contactSheet", e))?;
                let doc = crate::contact_sheet::prepare(s, p).and_then(|work| work.run(&mut |_, _| true)).map_err(|e| bad("export.contactSheet", e))?;
                s.check_write_target(path).map_err(|e| bad("export.contactSheet", e))?;
                crate::export::write_file(path, &doc.bytes).map_err(|e| bad("export.contactSheet", e))?;
                Ok(json!({"path": path, "pages": doc.pages, "photos": doc.photos, "bytes": doc.bytes.len()}))
            }
        ),
        cmd!(
            "export.presets",
            "Export Presets",
            [],
            None,
            "{} → [{name, builtin, params}] (use with app.export {preset: name, …overrides})",
            NoParams,
            always,
            |s, _| Ok(list(s))
        ),
        cmd!(
            "export.savePreset",
            "Save Export Preset",
            [],
            None,
            "{name, params?: app.export params (default: the last export's)} — adds or replaces a user preset, saved with the library → presets",
            SavePreset,
            always,
            save
        ),
        cmd!("export.deletePreset", "Delete Export Preset", [], None, "{name} — removes a user preset → presets", Named, always, delete),
        cmd!(
            query "export.checkTarget",
            "Check Output Path",
            [],
            None,
            "{path} — fails when writing `path` would replace a photo's original (or its XMP sidecar) in the library; front ends call it before saving a render or screenshot to a user-given path → {path}",
            Target,
            always,
            |s, p| {
                let path = p.path.as_deref().ok_or_else(|| bad("export.checkTarget", "missing `path`"))?;
                s.check_write_target(path).map_err(|e| bad("export.checkTarget", e))?;
                Ok(json!({"path": path}))
            }
        ),
    ];
    // Apple Events to Photos' import command (macOS); not in the web build. Not journaled: a
    // replay must not import again.
    #[cfg(not(target_arch = "wasm32"))]
    v.push(cmd!(
        query "export.addToPhotos",
        "Add to Apple Photos",
        [],
        None,
        "{paths: [absolute file paths], album?: name (an album at the top level of Photos, made when missing; default none), wait?} → once Photos is done, {job, running: false, requested, album, imported, ids: [Photos media item ids], warning?}; with wait: false, at once {job, running: true, requested, album} and the outcome later from export.photosImports {job}. wait defaults to true without a window (lightcraft-cli, the MCP server) and to false in the desktop app, where wait: true is refused (its control requests are answered on the UI thread, which never waits for Photos). macOS: asks Photos to import the files (the first time, macOS asks the user to allow LightCraft to control Photos). One import runs at a time, and an export adding to Photos reserves it before writing: another is refused until it ends. app.export does this for the files it writes with `addToPhotos: true`",
        AddToPhotos,
        photos_enabled,
        add_to_photos
    ));
    #[cfg(not(target_arch = "wasm32"))]
    v.push(cmd!(
        query "export.photosImports",
        "Apple Photos Imports",
        [],
        None,
        "{job?} → {imports: [{job, running, exporting?, requested, album, imported?, ids?, warning?, error?, skipped?}]} (this session's, oldest first), or the one job: how Add to Apple Photos is getting on (exporting: reserved by an export still writing its files)",
        PhotosImports,
        always,
        photos_imports
    ));
    v
}
