//! Preset files: import `.lcpreset`, XMP, `.lrtemplate`, DNG-preset, Luminar looks and zip bundles; export `.lcpreset`.

use serde_json::{Value, json};

use super::mask_effect_files as mef;
use super::params::params;
use super::{CommandSpec, always, bad, cmd};
use crate::preset_import::{group_from_dir, read_presets};
use crate::presets::{LCPRESET_EXT, expand_preset_paths, to_lcpreset};
use crate::{Result, Session};

params! {
    /// `preset.import`
    struct Import {
        paths: Option<Vec<String>>,
        group: Option<String>,
        #[serde(default)]
        dry_run: bool,
    }
}

params! {
    /// `preset.export`
    struct Export {
        path: Option<String>,
        ids: Option<Vec<String>>,
        group: Option<String>,
    }
}

fn import(s: &mut Session, p: Import) -> Result<Value> {
    let paths = p.paths.filter(|v| !v.is_empty()).ok_or_else(|| bad("preset.import", "no paths"))?;
    let group = p.group.as_deref().map(str::trim).filter(|g| !g.is_empty()).map(str::to_string);
    let dry = p.dry_run;
    let mut imported = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0usize;
    // local adjustment (brush) presets — Lightroom's .lrtemplate, Camera Raw's local .xmp, our
    // .lceffect, also inside zips — become mask effects
    let mut effects = Vec::new();
    let mut effect_failed = Vec::new();
    for top in &paths {
        let from_dir = std::path::Path::new(top).is_dir();
        for f in expand_preset_paths(std::slice::from_ref(top)) {
            // presets in a folder go to a group named after it (unless the file names its own)
            // the path from the imported folder (inclusive) to the file's folder
            let base = std::path::Path::new(top).parent().unwrap_or(std::path::Path::new(""));
            let rel = std::path::Path::new(&f).parent().and_then(|d| d.strip_prefix(base).ok()).map(|d| d.to_string_lossy().to_string());
            let rel = rel.filter(|_| from_dir);
            let bytes = match mef::read_capped(&f) {
                Ok(b) => b,
                Err(e) => {
                    failed.push(json!([f, e]));
                    continue;
                }
            };
            let lower = f.to_ascii_lowercase();
            let maybe_local = [".lrtemplate", ".xmp", ".zip", ".lceffect"].iter().any(|x| lower.ends_with(x));
            let before = effects.len() + effect_failed.len();
            let local = maybe_local && mef::found_in(&f, &bytes, rel.as_deref().and_then(mef::group_for_dir), &mut effects, &mut effect_failed);
            // a zip may hold develop presets as well
            if local && !lower.ends_with(".zip") {
                continue;
            }
            let dir_group = rel.and_then(|d| group_from_dir(&d));
            let parsed = read_presets(&f, &bytes, dir_group);
            if local && parsed.is_err() && effects.len() + effect_failed.len() > before {
                continue;
            }
            match parsed {
                Ok(list) => {
                    for mut it in list {
                        if let Some(g) = &group {
                            it.preset.group = g.clone();
                        }
                        // the profile it names (issue #1001): one the user imported, or ours for a basic one
                        let mut profile = Value::Null;
                        if let Some((r, _)) = crate::profile_refs::apply(&mut it.preset.settings, &s.prefs.lut_profiles) {
                            let look = it.preset.settings["profile"]["look"].clone();
                            it.unmapped.retain(|u| u != "Look");
                            it.unmapped.extend(crate::profile_refs::report(&look, &r));
                            profile = json!({"name": look["name"], "id": it.preset.settings["profile"].get("id")});
                        }
                        let (name, grp) = (it.preset.name.clone(), it.preset.group.clone());
                        let id = if dry { Some(it.preset.id.clone()) } else { s.add_presets(vec![it.preset]).pop() };
                        match id {
                            Some(id) => {
                                imported.push(json!({"id": id, "name": name, "group": grp, "file": f, "unmapped": it.unmapped, "profile": profile}))
                            }
                            None => skipped += 1,
                        }
                    }
                }
                Err(e) => failed.push(json!([f, e])),
            }
        }
    }
    failed.extend(effect_failed.into_iter().map(|(f, e)| json!([f, e])));
    let (effects, dividers) = mef::add_found(s, effects, group.as_deref(), dry)?;
    Ok(json!({"imported": imported, "skipped": skipped, "failed": failed, "effects": effects, "effectDividers": dividers}))
}

fn export(s: &mut Session, p: Export) -> Result<Value> {
    let path = p.path.as_deref().filter(|x| !x.trim().is_empty()).ok_or_else(|| bad("preset.export", "missing path"))?;
    let mut path = std::path::PathBuf::from(path);
    if path.extension().is_none() {
        path.set_extension(LCPRESET_EXT);
    }
    let ids = p.ids;
    let group = p.group.as_deref();
    let chosen: Vec<_> = s
        .presets
        .iter()
        .filter(|x| match (&ids, group) {
            (Some(ids), _) => ids.contains(&x.id),
            (None, Some(g)) => x.group == g,
            (None, None) => !x.builtin,
        })
        .cloned()
        .collect();
    if chosen.is_empty() {
        return Err(bad("preset.export", "no presets to export (create one first, or pass ids/group)"));
    }
    let target = path.to_string_lossy();
    s.write_user_file("preset.export", &target, to_lcpreset(&chosen).as_bytes(), true)?;
    Ok(json!({"path": path.display().to_string(), "count": chosen.len()}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "preset.import",
            "Import Presets",
            [],
            None,
            "{paths: [file or folder], group?, dryRun?} — .lcpreset, XMP presets, .lrtemplate, photos carrying edits (DNG/JPEG/TIFF \"DNG presets\"), Luminar looks (.lmp, .mplumpack collections) and .zip bundles of these; folders give their name as group; crs: fields mapped per docs/xmp-interop.md; local adjustment presets (Lightroom .lrtemplate, Camera Raw local .xmp, .lceffect) become mask effects (`mask.effects`; divider presets become their groups); a profile the preset names applies when it was imported with profile.import (before or after) → {imported: [{id,name,group,file,unmapped,profile: {name, id}|null}], skipped, failed, effects: [{name,group,file,unmapped}], effectDividers: [{name,group,file}]}",
            Import,
            always,
            import
        ),
        cmd!(
            "preset.export",
            "Export Presets",
            [],
            None,
            "{path, ids?: [presetId], group?: name} (default: all user presets) → {path, count}; writes a .lcpreset JSON file",
            Export,
            always,
            export
        ),
    ]
}
