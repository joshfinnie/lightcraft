//! Develop commands. All operate on the active photo unless `ids` are given (sync).

use lightcraft_catalog::{Op, PhotoId, Version};
use lightcraft_develop::{DevelopSettings, Preset, ProcessVersion, Section, SettingsGroup, Treatment, Upright, WbMode, controls};
use lightcraft_geom::{CropGeometry, CropHandle, Point, Rect, crop_fit_angle, drag_crop};
use serde_json::{Value, json};

use super::params::{NoParams, params, whole};
use super::{CommandSpec, always, bad, cmd, has_active, has_clipboard, has_selection, ok};
use crate::{Result, Session, media::SourceLevel};

fn active(s: &Session, c: &str) -> Result<PhotoId> {
    s.active().ok_or_else(|| bad(c, "no active photo"))
}

/// Modify the active photo's settings with `f` and commit (or preview during an interaction).
fn edit(s: &mut Session, c: &str, label: &str, f: impl FnOnce(&mut DevelopSettings) -> Result<()>) -> Result<Value> {
    let id = active(s, c)?;
    let mut d = (*s.develop_of(id).unwrap_or_default()).clone();
    f(&mut d)?;
    s.set_develop(id, d, label)?;
    ok()
}

fn crop_dims(s: &Session, id: PhotoId) -> (f64, f64) {
    let p = s.catalog.photo(id);
    let (w, h) = p.map(|p| (p.width.max(1) as f64, p.height.max(1) as f64)).unwrap_or((3.0, 2.0));
    let o = p.map(|p| p.develop.orientation).unwrap_or_default();
    if o.swaps_axes() { (h, w) } else { (w, h) }
}

/// After a generic control write (`develop.set`, `develop.adjust`, …) that touched `crop.angle`:
/// refit the crop as `crop.straighten` does, from the crop it had `before`, so the box never takes in
/// canvas outside the photo (issue #742).
fn refit_crop_angle(touched: bool, before: CropGeometry, d: &mut DevelopSettings, (w, h): (f64, f64)) {
    if touched {
        d.crop.geometry = before.with_angle(d.crop.geometry.angle, w, h);
    }
}

const CROP_ANGLE: &str = "crop.angle";

/// `v` as a list of numbers; `None` unless it is an array of nothing but numbers.
fn numbers(v: &Value) -> Option<Vec<f64>> {
    v.as_array()?.iter().map(Value::as_f64).collect()
}

/// A width × height lock stored as hundredths. Rejects non-finite, non-positive and
/// extreme (beyond 20:1) shapes, which would give an empty or unusable crop.
fn aspect_tuple(c: &str, x: f64, y: f64) -> Result<(u32, u32)> {
    let ratio = x / y;
    if !(x.is_finite()
        && y.is_finite()
        && x > 0.0
        && y > 0.0
        && ratio.is_finite()
        && (1.0 / lightcraft_geom::MAX_RATIO..=lightcraft_geom::MAX_RATIO).contains(&ratio))
    {
        return Err(bad(c, "aspect needs two positive numbers with a ratio between 1:20 and 20:1"));
    }
    let to = |v: f64| (v * 100.0).round();
    let (a, b) = (to(x), to(y));
    if a < 1.0 || b < 1.0 || a > f64::from(u32::MAX) || b > f64::from(u32::MAX) {
        return Err(bad(c, "aspect numbers are out of range"));
    }
    Ok((a as u32, b as u32))
}

fn set_aspect(d: &mut DevelopSettings, w: f64, h: f64, aspect: Option<(u32, u32)>) {
    d.crop.aspect = aspect;
    let a = aspect.map(|(aw, ah)| {
        // orientation of the crop follows the image (landscape image → landscape crop)
        let (aw, ah) = (aw as f64, ah as f64);
        if (w >= h) == (aw >= ah) { aw / ah } else { ah / aw }
    });
    let angle = d.crop.geometry.angle;
    d.crop.geometry = crop_fit_angle(w, h, angle, a);
}

/// Photo `id` is on an older rendering process than [`ProcessVersion::LATEST`].
fn outdated(s: &Session, id: PhotoId) -> bool {
    s.catalog.photo(id).is_some_and(|p| p.develop.process.is_outdated())
}

fn already_current() -> String {
    format!("already on the current process (version {})", ProcessVersion::LATEST.0)
}

/// Update to Current Process applies to the selection (the selected photos, else the active
/// photo): one of them is on an older rendering process. Calls that name their photos are
/// checked by the command instead (`explicit_targets`).
fn has_outdated_process(s: &Session) -> std::result::Result<(), String> {
    has_selection(s)?;
    let sel = &s.selection;
    let targets = if sel.ids.is_empty() { sel.active.as_slice() } else { sel.ids.as_slice() };
    if targets.iter().any(|id| outdated(s, *id)) { Ok(()) } else { Err(already_current()) }
}

/// The photos a call names (`ids`, or `id`), each of which must be in the library; `None` when it
/// names none (the selection applies).
fn named_photos(s: &Session, p: &Targets, c: &str) -> Result<Option<Vec<PhotoId>>> {
    let given = match (&p.ids, p.id) {
        (Some(ids), _) => ids.clone(),
        (None, Some(id)) => vec![id],
        (None, None) => return Ok(None),
    };
    if given.is_empty() {
        return Err(bad(c, "`ids` names no photos"));
    }
    if let Some(id) = given.iter().find(|id| s.catalog.photo(**id).is_none()) {
        return Err(bad(c, format!("no such photo {}", id.0)));
    }
    Ok(Some(given))
}

/// The user preset `pid` (built-ins can't be changed).
fn user_preset<'a>(s: &'a mut Session, pid: &str, c: &str) -> Result<&'a mut Preset> {
    let pr = s.presets.iter_mut().find(|x| x.id == pid).ok_or_else(|| bad(c, format!("unknown preset `{pid}`")))?;
    if pr.builtin {
        return Err(bad(c, "built-in presets can't be changed"));
    }
    Ok(pr)
}

/// Change version `i` of the active photo (`f` gets it and the current settings).
fn edit_version(s: &mut Session, i: usize, c: &str, label: &str, f: impl FnOnce(&mut Version, std::sync::Arc<DevelopSettings>)) -> Result<Value> {
    let id = active(s, c)?;
    let ph = s.catalog.photo(id).ok_or_else(|| bad(c, "no photo"))?;
    let mut versions = ph.versions.clone();
    let v = versions.get_mut(i).ok_or_else(|| bad(c, "no such version"))?;
    f(v, ph.develop.clone());
    s.commit(label, Op::SetVersions { id, versions })?;
    ok()
}

params! {
    /// `develop.set`
    struct Set {
        control: Option<String>,
        value: Option<f64>,
        values: Option<std::collections::BTreeMap<String, f64>>,
        ids: Option<Vec<PhotoId>>,
    }
}

params! {
    /// `develop.adjust`
    struct Nudge {
        control: String,
        delta: f64,
    }
}

params! {
    /// `develop.merge`: `settings` is a partial `DevelopSettings`, checked when merged.
    struct Merge {
        settings: Value,
        label: Option<String>,
    }
}

params! {
    /// Develop commands that take only their targets.
    struct Targets with targets {}
}

params! {
    /// `develop.resetSection`
    struct ResetSection {
        section: Section,
    }
}

params! {
    /// `develop.resetControl`
    struct Control {
        control: String,
    }
}

params! {
    /// `develop.sectionEnabled`
    struct SectionEnabled {
        section: String,
        enabled: Option<bool>,
    }
}

params! {
    /// `develop.wb`
    struct Wb {
        mode: Option<WbMode>,
        temp: Option<f64>,
        tint: Option<f64>,
    }
}

params! {
    /// A normalized image point.
    struct Xy {
        x: f64,
        y: f64,
    }
}

params! {
    /// `develop.treatment`
    struct TreatmentParams {
        bw: Option<bool>,
    }
}

params! {
    /// `develop.profile`
    struct Profile {
        id: String,
        amount: Option<f64>,
    }
}

params! {
    /// `profile.favorite`, `preset.favorite`
    struct Favorite {
        id: String,
        favorite: Option<bool>,
    }
}

params! {
    /// `develop.curve`
    struct Curve {
        channel: Option<String>,
        points: Vec<[f64; 2]>,
    }
}

params! {
    /// `crop.set`
    struct CropSet {
        rect: Option<[f64; 4]>,
        angle: Option<f64>,
    }
}

/// A crop handle by name or index (0..8).
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum HandleRef {
    Name(String),
    Index(u64),
}

params! {
    /// `crop.drag`
    struct CropDrag {
        handle: HandleRef,
        from: [f64; 2],
        to: [f64; 2],
        start: Option<[f64; 4]>,
        #[serde(default)]
        keep_ratio: bool,
    }
}

params! {
    /// `crop.straighten`
    struct Straighten {
        angle: f64,
    }
}

params! {
    /// `crop.aspect`: a name, `"WxH"` or `[w, h]`.
    struct Aspect {
        aspect: Option<Value>,
    }
}

params! {
    /// `develop.hdr`
    struct HdrParams {
        enabled: Option<bool>,
        max_ev: Option<f64>,
    }
}

params! {
    /// `geometry.upright`
    struct UprightParams {
        mode: Option<Upright>,
    }
}

params! {
    /// `geometry.guides`
    struct Guides {
        #[serde(default)]
        guides: Vec<[f64; 4]>,
        #[serde(default)]
        add: bool,
    }
}

params! {
    /// `develop.copy`, `develop.sync`
    struct Groups {
        groups: Option<Vec<SettingsGroup>>,
    }
}

params! {
    /// `develop.paste`, `develop.pastePrevious`
    struct TargetGroups with targets {
        groups: Option<Vec<SettingsGroup>>,
    }
}

params! {
    /// `develop.quickAdjust`
    struct QuickAdjust with targets {
        control: String,
        delta: f64,
    }
}

params! {
    /// `develop.autoSync`
    struct On {
        on: Option<bool>,
    }
}

params! {
    /// `preset.apply`: `id` is the preset, the photos are `ids`.
    struct PresetApply {
        id: String,
        amount: Option<f64>,
        ids: Option<Vec<PhotoId>>,
    }
}

params! {
    /// `preset.create`
    struct PresetCreate {
        name: String,
        group: Option<String>,
        groups: Option<Vec<SettingsGroup>>,
    }
}

params! {
    /// `preset.delete`
    struct PresetRef {
        id: String,
    }
}

params! {
    /// `preset.rename`
    struct PresetRename {
        id: String,
        name: Option<String>,
    }
}

params! {
    /// `preset.move`
    struct PresetMove {
        id: String,
        group: Option<String>,
    }
}

params! {
    /// `preset.update`
    struct PresetUpdate {
        id: String,
        groups: Option<Vec<SettingsGroup>>,
    }
}

params! {
    /// `version.create`
    struct Name {
        name: Option<String>,
    }
}

params! {
    /// A version or History step.
    struct Index {
        #[serde(deserialize_with = "whole")]
        index: usize,
    }
}

params! {
    /// `version.rename`
    struct VersionRename {
        #[serde(deserialize_with = "whole")]
        index: usize,
        name: Option<String>,
    }
}

params! {
    /// `develop.beginInteraction`
    struct Label {
        label: Option<String>,
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "develop.set",
            "Set Develop Values",
            [],
            None,
            "{control?: controlId, value?: number, values?: {controlId: number}, ids?: [photo]} — see develop.controls; `crop.angle` turns the crop as crop.straighten does (kept if it still fits, else the largest of its aspect inside the photo)",
            Set,
            has_active,
            |s, p| {
                let mut vals: Vec<(String, f64)> = Vec::new();
                match (p.control, p.value) {
                    (Some(c), Some(v)) => vals.push((c, v)),
                    (None, None) => {}
                    _ => return Err(bad("develop.set", "`control` and `value` go together")),
                }
                vals.extend(p.values.into_iter().flatten());
                if vals.is_empty() {
                    return Err(bad("develop.set", "give `control`+`value` or `values`"));
                }
                for (k, _) in &vals {
                    if controls::find(k).is_none() {
                        return Err(bad("develop.set", format!("unknown control `{k}`")));
                    }
                }
                let label = if vals.len() == 1 { controls::find(&vals[0].0).map(|c| c.label).unwrap_or("Edit").to_string() } else { "Edit".into() };
                let angle = vals.iter().any(|(k, _)| k == CROP_ANGLE);
                let apply = |d: &mut DevelopSettings, info: &lightcraft_pipeline::SourceInfo, dims: (f64, f64)| {
                    let before = d.crop.geometry;
                    if vals.iter().any(|(k, _)| k == "wb.temp" || k == "wb.tint") {
                        lightcraft_pipeline::local::normalize_wb(d, info);
                        if d.wb.mode == WbMode::AsShot {
                            d.wb.temp = info.as_shot_temp;
                            d.wb.tint = info.as_shot_tint;
                        }
                    }
                    for (k, v) in &vals {
                        controls::set(d, k, *v);
                        if k == "wb.temp" || k == "wb.tint" {
                            d.wb.mode = WbMode::Custom;
                        }
                    }
                    refit_crop_angle(angle, before, d, dims);
                };
                if let Some(ids) = p.ids {
                    let ops = ids
                        .iter()
                        .filter_map(|id| s.develop_of(*id).map(|d| (*id, d)))
                        .filter_map(|(id, d)| {
                            let mut d = (*d).clone();
                            let info = s.source_info(id);
                            apply(&mut d, &info, crop_dims(s, id));
                            s.develop_op(id, d, &label)
                        })
                        .collect();
                    s.commit(&label, Op::Batch { ops })?;
                    return ok();
                }
                let id = active(s, "develop.set")?;
                let info = s.source_info(id);
                let dims = crop_dims(s, id);
                edit(s, "develop.set", &label, |d| {
                    apply(d, &info, dims);
                    Ok(())
                })
            }
        ),
        cmd!("develop.adjust", "Nudge Develop Value", [], None, "{control, delta}", Nudge, has_active, |s, p| {
            let (c, delta) = (p.control, p.delta);
            let spec = controls::find(&c).ok_or_else(|| bad("develop.adjust", "unknown control"))?;
            let id = active(s, "develop.adjust")?;
            let (info, dims) = (s.source_info(id), crop_dims(s, id));
            edit(s, "develop.adjust", spec.label, |d| {
                let before = d.crop.geometry;
                if c == "wb.temp" || c == "wb.tint" {
                    lightcraft_pipeline::local::normalize_wb(d, &info);
                    if d.wb.mode == WbMode::AsShot {
                        d.wb.temp = info.as_shot_temp;
                        d.wb.tint = info.as_shot_tint;
                    }
                    d.wb.mode = WbMode::Custom;
                }
                let v = controls::get(d, &c).unwrap_or(spec.default);
                controls::set(d, &c, v + delta);
                refit_crop_angle(c == CROP_ANGLE, before, d, dims);
                Ok(())
            })
        }),
        cmd!("develop.merge", "Apply Settings JSON", [], None, "{settings: partial DevelopSettings JSON, label?}", Merge, has_active, |s, p| {
            let partial = p.settings;
            let label = p.label.unwrap_or_else(|| "Edit".into());
            edit(s, "develop.merge", &label, |d| {
                super::params::check_merge(d, &partial)?;
                *d = d.merged(&partial).map_err(|e| bad("develop.merge", e.to_string()))?;
                Ok(())
            })
        }),
        cmd!("develop.reset", "Reset Edits", ["Photo"], Some("Cmd+Shift+R"), "{ids?}", Targets, has_selection, |s, p| {
            let ids = p.targets(s);
            let ops = ids
                .iter()
                .filter_map(|id| s.develop_of(*id).map(|d| (*id, d)))
                .filter_map(|(id, _d)| {
                    // back to the photo's import defaults when a default preset gave it its look
                    let look = s.catalog.photo(id).and_then(|p| p.import_look.clone());
                    let fresh = match look {
                        Some(l) => (*l).clone(),
                        None => {
                            // the photo's built-in defaults (raws: capture sharpening and colour noise
                            // reduction, embedded lens corrections), as at import, with the white balance
                            // of its current as-shot reference
                            let info = s.source_info(id);
                            let defaults = s.catalog.photo(id).map(|p| p.camera_defaults()).unwrap_or_default();
                            let mut fresh = DevelopSettings {
                                wb: lightcraft_develop::WhiteBalance {
                                    mode: WbMode::AsShot,
                                    temp: info.as_shot_temp,
                                    tint: info.as_shot_tint,
                                    ..Default::default()
                                },
                                ..defaults
                            };
                            lightcraft_pipeline::local::normalize_wb(&mut fresh, &info);
                            fresh
                        }
                    };
                    // a fresh start renders with the current process (a section or slider reset doesn't change it)
                    s.develop_op(id, DevelopSettings { process: ProcessVersion::LATEST, ..fresh }, "Reset")
                })
                .collect();
            s.commit("Reset", Op::Batch { ops })?;
            ok()
        }),
        CommandSpec {
            explicit_targets: true,
            ..cmd!(
                "develop.updateProcess",
                "Update to Current Process",
                ["Photo"],
                None,
                "{ids?}: moves the target photos (`ids`, else the selection) that are on an older rendering process to the current one, as one undo step (their look can change; the sliders keep their values; see docs/process-versions.md). Enabled while a selected photo is on an older process; named photos are checked themselves (each must exist, one must be on an older process) → {changed, process}",
                Targets,
                has_outdated_process,
                |s, p| {
                    const C: &str = "develop.updateProcess";
                    const LABEL: &str = "Update to Current Process";
                    let targets = match named_photos(s, &p, C)? {
                        Some(ids) if !ids.iter().any(|id| outdated(s, *id)) => return Err(crate::EngineError::Disabled(C.into(), already_current())),
                        Some(ids) => ids,
                        None => p.targets(s),
                    };
                    let ops: Vec<Op> = targets
                        .into_iter()
                        .filter_map(|id| s.develop_of(id).map(|d| (id, d)))
                        .filter(|(_, d)| d.process.is_outdated())
                        .filter_map(|(id, d)| s.develop_op(id, DevelopSettings { process: ProcessVersion::LATEST, ..(*d).clone() }, LABEL))
                        .collect();
                    let n = ops.len();
                    if n > 0 {
                        s.commit(LABEL, Op::Batch { ops })?;
                    }
                    Ok(json!({"changed": n, "process": ProcessVersion::LATEST}))
                }
            )
        },
        cmd!(
            "develop.resetSection",
            "Reset Section",
            [],
            None,
            "{section: light|curve|color|mixer|grading|effects|vignette|grain|detail|optics|geometry}",
            ResetSection,
            has_active,
            |s, p| {
                let sec = p.section;
                edit(s, "develop.resetSection", &format!("Reset {}", sec.label()), |d| {
                    d.reset_section(sec);
                    Ok(())
                })
            }
        ),
        cmd!("develop.resetControl", "Reset Control", [], None, "{control}", Control, has_active, |s, p| {
            let c = p.control;
            let spec = controls::find(&c).ok_or_else(|| bad("develop.resetControl", "unknown control"))?;
            let dims = crop_dims(s, active(s, "develop.resetControl")?);
            edit(s, "develop.resetControl", spec.label, |d| {
                let before = d.crop.geometry;
                controls::set(d, &c, spec.default);
                refit_crop_angle(c == CROP_ANGLE, before, d, dims);
                Ok(())
            })
        }),
        cmd!("develop.sectionEnabled", "Toggle Section", [], None, "{section: string, enabled: bool}", SectionEnabled, has_active, |s, p| {
            let sec = p.section;
            let on = p.enabled.unwrap_or(true);
            edit(s, "develop.sectionEnabled", "Toggle Section", |d| {
                d.set_section_enabled(&sec, on);
                Ok(())
            })
        }),
        cmd!(
            "develop.autoBwMix",
            "Auto B&W Mix",
            [],
            None,
            "{} — sets the black & white mix from the photo's colours (and switches to B&W)",
            NoParams,
            has_active,
            |s, _| {
                let id = active(s, "develop.autoBwMix")?;
                let src = s.source_now(id, SourceLevel::Thumb).map_err(|e| bad("develop.autoBwMix", e))?;
                let info = s.source_info(id);
                let d = s.develop_of(id).unwrap_or_default();
                let m = lightcraft_pipeline::auto::auto_bw_mix(&src, &info, &d);
                edit(s, "develop.autoBwMix", "Auto B&W Mix", |d| {
                    d.treatment = Treatment::Bw;
                    let b = &mut d.bw_mix;
                    [b.red, b.orange, b.yellow, b.green, b.aqua, b.blue, b.purple, b.magenta] = m;
                    Ok(())
                })?;
                Ok(json!({"mix": m}))
            }
        ),
        cmd!("develop.auto", "Auto Settings", ["Photo"], Some("Shift+A"), "{}", NoParams, has_active, |s, _| {
            let id = active(s, "develop.auto")?;
            let src = s.source_now(id, SourceLevel::Thumb).map_err(|e| bad("develop.auto", e))?;
            let info = s.source_info(id);
            let d = s.develop_of(id).unwrap_or_default();
            let a = lightcraft_pipeline::auto::auto_tone(&src, &info, &d);
            edit(s, "develop.auto", "Auto", |d| {
                d.light.exposure = a.exposure;
                d.light.contrast = a.contrast;
                d.light.highlights = a.highlights;
                d.light.shadows = a.shadows;
                d.light.whites = a.whites;
                d.light.blacks = a.blacks;
                d.color.vibrance = a.vibrance;
                d.color.saturation = a.saturation;
                Ok(())
            })?;
            Ok(serde_json::to_value(a).unwrap_or_default())
        }),
        cmd!(
            "develop.wb",
            "White Balance",
            [],
            None,
            "{mode: asShot|auto|daylight|cloudy|shade|tungsten|fluorescent|flash|custom, temp?, tint?}",
            Wb,
            has_active,
            |s, p| {
                let mode = p.mode.unwrap_or(WbMode::Custom);
                let id = active(s, "develop.wb")?;
                let info = s.source_info(id);
                let (mut t, mut tint) = match mode {
                    WbMode::AsShot => (info.as_shot_temp, info.as_shot_tint),
                    WbMode::Auto => {
                        let src = s.source_now(id, SourceLevel::Thumb).map_err(|e| bad("develop.wb", e))?;
                        lightcraft_pipeline::auto::auto_wb(&src, &info)
                    }
                    m => m
                        .preset()
                        .map(|(t, ti)| {
                            // presets are Kelvin on raws with that scale, relative to daylight (6500 K = as shot) elsewhere
                            if info.raw && !info.relative_wb { (t, ti) } else { (6500.0 * t / 5500.0, ti) }
                        })
                        .unwrap_or((info.as_shot_temp, info.as_shot_tint)),
                };
                t = p.temp.unwrap_or(t);
                tint = p.tint.unwrap_or(tint);
                // The resolved temperature/tint is stored, so rendering never depends on the mode; the
                // mode is kept for the UI's WB dropdown.
                edit(s, "develop.wb", "White Balance", |d| {
                    lightcraft_pipeline::local::normalize_wb(d, &info);
                    d.wb.mode = mode;
                    d.wb.temp = t.clamp(2000.0, 50000.0);
                    d.wb.tint = tint.clamp(-150.0, 150.0);
                    Ok(())
                })?;
                Ok(json!({"temp": t, "tint": tint}))
            }
        ),
        cmd!(
            "develop.wbPick",
            "White Balance Selector",
            [],
            Some("W"),
            "{x, y} normalized image coords of a neutral point",
            Xy,
            has_active,
            |s, p| {
                let (x, y) = (p.x, p.y);
                let id = active(s, "develop.wbPick")?;
                let src = s.source_now(id, SourceLevel::Thumb).map_err(|e| bad("develop.wbPick", e))?;
                let info = s.source_info(id);
                let d = s.develop_of(id).unwrap_or_default();
                // Sample a small neighbourhood in the oriented source.
                let o = src.oriented(d.orientation);
                let (cx, cy) = ((x.clamp(0.0, 1.0) * o.width as f64) as isize, (y.clamp(0.0, 1.0) * o.height as f64) as isize);
                let mut acc = [0.0f32; 3];
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let c = o.get_clamped(cx + dx, cy + dy);
                        for i in 0..3 {
                            acc[i] += c[i] / 25.0;
                        }
                    }
                }
                let patch = lightcraft_raster::Rgb32f::filled(4, 4, acc);
                let (t, tint) = lightcraft_pipeline::auto::auto_wb(&patch, &info);
                edit(s, "develop.wbPick", "White Balance", |d| {
                    lightcraft_pipeline::local::normalize_wb(d, &info);
                    d.wb.mode = WbMode::Custom;
                    d.wb.temp = t;
                    d.wb.tint = tint;
                    Ok(())
                })?;
                Ok(json!({"temp": t, "tint": tint}))
            }
        ),
        cmd!(
            "develop.treatment",
            "Black & White",
            ["Photo"],
            Some("V"),
            "{bw?: bool} (toggles when omitted)",
            TreatmentParams,
            has_active,
            |s, p| {
                edit(s, "develop.treatment", "Treatment", |d| {
                    let bw = p.bw.unwrap_or(d.treatment != Treatment::Bw);
                    d.treatment = if bw { Treatment::Bw } else { Treatment::Color };
                    Ok(())
                })
            }
        ),
        cmd!("develop.profile", "Set Profile", [], None, "{id: profile id (see profiles.list), amount?: 0..200}", Profile, has_active, |s, p| {
            let id = p.id;
            if !crate::presets::PROFILES.iter().any(|x| x.id == id) && !s.prefs.lut_profiles.iter().any(|x| x.id == id) {
                return Err(bad("develop.profile", format!("unknown profile `{id}`")));
            }
            edit(s, "develop.profile", "Profile", |d| {
                d.profile.id = id.clone();
                d.profile.amount = p.amount.unwrap_or(d.profile.amount).clamp(0.0, 200.0);
                Ok(())
            })?;
            s.note_profile_used(&id);
            ok()
        }),
        cmd!("profile.favorite", "Favorite Profile", [], None, "{id, favorite?: bool} (toggles when omitted)", Favorite, always, |s, p| {
            let id = p.id;
            if crate::presets::profile(&id).is_none() && !s.prefs.lut_profiles.iter().any(|x| x.id == id) {
                return Err(bad("profile.favorite", format!("unknown profile `{id}`")));
            }
            let on = s.profile_favorites.contains(&id);
            let want = p.favorite.unwrap_or(!on);
            if want && !on {
                s.profile_favorites.push(id);
            } else if !want {
                s.profile_favorites.retain(|f| *f != id);
            }
            Ok(json!({"favorite": want}))
        }),
        cmd!(
            "develop.curve",
            "Set Point Curve",
            [],
            None,
            "{channel: master|red|green|blue, points: [[x,y],...] in 0..1}",
            Curve,
            has_active,
            |s, p| {
                let ch = p.channel.unwrap_or_else(|| "master".into());
                let mut pts: Vec<Point> = p.points.iter().map(|[x, y]| Point::new(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))).collect();
                // stored in the order the curve runs, as the editor draws and drags them
                pts.sort_by(|a, b| a.x.total_cmp(&b.x));
                edit(s, "develop.curve", "Tone Curve", |d| {
                    match ch.as_str() {
                        "master" | "rgb" => d.curve.master = pts,
                        "red" => d.curve.red = pts,
                        "green" => d.curve.green = pts,
                        "blue" => d.curve.blue = pts,
                        other => return Err(bad("develop.curve", format!("unknown channel `{other}`"))),
                    }
                    Ok(())
                })
            }
        ),
        // ---- crop & rotate
        cmd!(
            "crop.set",
            "Set Crop",
            [],
            None,
            "{rect?: [x0,y0,x1,y1] normalized in the straightened frame, angle?: degrees}",
            CropSet,
            has_active,
            |s, p| {
                let id = active(s, "crop.set")?;
                let (w, h) = crop_dims(s, id);
                edit(s, "crop.set", "Crop", |d| {
                    if let Some([x0, y0, x1, y1]) = p.rect {
                        d.crop.geometry.rect = Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
                    }
                    let angle = p.angle.map(|a| a.clamp(-45.0, 45.0));
                    d.crop.geometry = match (p.rect, angle) {
                        // a new angle alone turns the crop as Straighten does (issue #742)
                        (None, Some(a)) => d.crop.geometry.with_angle(a, w, h),
                        (_, a) => CropGeometry { angle: a.unwrap_or(d.crop.geometry.angle), ..d.crop.geometry }.constrained(w, h),
                    };
                    Ok(())
                })
            }
        ),
        cmd!(
            "crop.drag",
            "Drag Crop Handle",
            [],
            None,
            "{handle: topLeft|topRight|bottomRight|bottomLeft|top|right|bottom|left|move | 0..8, from: [x,y], to: [x,y] (normalized, straightened frame), start?: [x0,y0,x1,y1] (default: the current crop), keepRatio?: bool (default false)} — drags a crop handle; stops at the image edge, keeps the opposite edges, honours the aspect lock; keepRatio holds the frame's current proportions for this drag even with the lock off (the UI sends it while Shift is held)",
            CropDrag,
            has_active,
            |s, p| {
                let id = active(s, "crop.drag")?;
                let (w, h) = crop_dims(s, id);
                let handle = match &p.handle {
                    HandleRef::Name(n) => CropHandle::from_name(n),
                    HandleRef::Index(i) => u8::try_from(*i).ok().and_then(CropHandle::from_index),
                }
                .ok_or_else(|| bad("crop.drag", "unknown handle"))?;
                let ([fx, fy], [tx, ty]) = (p.from, p.to);
                let (from, to) = (Point::new(fx, fy), Point::new(tx, ty));
                let given = p.start.map(|[x0, y0, x1, y1]| Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)));
                let keep_ratio = p.keep_ratio;
                edit(s, "crop.drag", "Crop", |d| {
                    let start = CropGeometry { rect: given.unwrap_or(d.crop.geometry.rect), angle: d.crop.geometry.angle };
                    // A locked crop keeps the shape it has: the lock stores a width:height tuple whose
                    // orientation is reinterpreted per image, so the frame itself is the truth.
                    // `keepRatio` (Shift held) asks for the same thing for one drag without touching the lock.
                    let ratio = (d.crop.aspect.is_some() || keep_ratio).then(|| start.rect_px(w, h).aspect()).filter(|a| a.is_finite() && *a > 0.0);
                    d.crop.geometry = drag_crop(start, handle, from, to, w, h, ratio);
                    Ok(())
                })
            }
        ),
        cmd!(
            "crop.straighten",
            "Straighten",
            [],
            None,
            "{angle: degrees -45..45} — keeps the crop if it still fits, else shrinks it about its centre (the largest crop of its aspect stays the largest)",
            Straighten,
            has_active,
            |s, p| {
                let id = active(s, "crop.straighten")?;
                let (w, h) = crop_dims(s, id);
                let angle = p.angle.clamp(-45.0, 45.0);
                edit(s, "crop.straighten", "Straighten", |d| {
                    // keep the current crop if it still fits, else shrink it about its centre until it does
                    d.crop.geometry = d.crop.geometry.with_angle(angle, w, h);
                    Ok(())
                })
            }
        ),
        cmd!(
            "crop.autoStraighten",
            "Auto Straighten",
            [],
            None,
            "{} — level the horizon (or plumb lines) with the crop angle",
            NoParams,
            has_active,
            |s, _| {
                let id = active(s, "crop.autoStraighten")?;
                let src = s.source_now(id, SourceLevel::Preview).map_err(|e| bad("crop.autoStraighten", e))?;
                let info = s.source_info(id);
                let d = s.develop_of(id).unwrap_or_default();
                let Some(level) = lightcraft_pipeline::upright::level_degrees(&src, &info, &d) else {
                    return Ok(json!({"changed": false, "reason": "no dominant horizontal or vertical lines"}));
                };
                // Same convention as the crop angle (verified: a horizon descending 6° to the right
                // comes out level with angle = level ≈ −6°).
                let angle = level.clamp(-45.0, 45.0);
                s.execute("crop.straighten", &json!({"angle": (angle * 100.0).round() / 100.0}))?;
                Ok(json!({"changed": true, "angle": angle}))
            }
        ),
        cmd!(
            "crop.aspect",
            "Crop Aspect",
            [],
            None,
            "{aspect: \"original\"|\"free\"|\"current\" (lock the crop's present shape)|\"toggle\" (lock ↔ free)|\"1x1\"|\"4x5\"|\"8.5x11\"|\"5x7\"|\"2x3\"|\"4x3\"|\"16x9\"|\"16x10\"|[w,h]}",
            Aspect,
            has_active,
            |s, p| {
                let id = active(s, "crop.aspect")?;
                let (w, h) = crop_dims(s, id);
                let locked = s.develop_of(id).is_some_and(|d| d.crop.aspect.is_some());
                let lock_current = match p.aspect.as_ref().and_then(Value::as_str) {
                    Some("current") => true,
                    Some("toggle") => !locked,
                    _ => false,
                };
                if lock_current {
                    // lock to the rectangle as it is: no refit
                    return edit(s, "crop.aspect", "Lock Crop Aspect", |d| {
                        let r = d.crop.geometry.rect_px(w, h);
                        d.crop.aspect = Some(((r.width() * 100.0).round().max(1.0) as u32, (r.height() * 100.0).round().max(1.0) as u32));
                        Ok(())
                    });
                }
                let aspect = match &p.aspect {
                    Some(Value::String(a)) if a == "toggle" => None,
                    Some(Value::String(a)) if a == "free" => None,
                    Some(Value::String(a)) if a == "original" => Some(((w * 100.0) as u32, (h * 100.0) as u32)),
                    Some(Value::String(a)) => {
                        let (x, y) = a.split_once('x').ok_or_else(|| bad("crop.aspect", "use WxH"))?;
                        let (x, y): (f64, f64) =
                            (x.parse().map_err(|_| bad("crop.aspect", "bad number"))?, y.parse().map_err(|_| bad("crop.aspect", "bad number"))?);
                        Some(aspect_tuple("crop.aspect", x, y)?)
                    }
                    Some(v @ Value::Array(_)) => match numbers(v).as_deref() {
                        Some(&[x, y]) => Some(aspect_tuple("crop.aspect", x, y)?),
                        _ => return Err(bad("crop.aspect", "use [w, h] with two numbers")),
                    },
                    _ => return Err(bad("crop.aspect", "missing aspect")),
                };
                edit(s, "crop.aspect", "Crop Aspect", |d| {
                    if aspect.is_none() {
                        d.crop.aspect = None;
                    } else {
                        set_aspect(d, w, h, aspect);
                    }
                    Ok(())
                })
            }
        ),
        cmd!("crop.rotateAspect", "Rotate Crop Aspect", [], Some("X"), "{}", NoParams, has_active, |s, _| {
            let id = active(s, "crop.rotateAspect")?;
            let (w, h) = crop_dims(s, id);
            edit(s, "crop.rotateAspect", "Rotate Crop", |d| {
                let r = d.crop.geometry.rect_px(w, h);
                // a degenerate crop has no aspect to rotate: fall back to the image's own
                let a = Some(r.height() / r.width()).filter(|a| a.is_finite() && *a > 0.0);
                d.crop.aspect = d.crop.aspect.map(|(x, y)| (y, x));
                let angle = d.crop.geometry.angle;
                d.crop.geometry = crop_fit_angle(w, h, angle, a);
                Ok(())
            })
        }),
        cmd!("crop.reset", "Reset Crop", [], Some("Cmd+Alt+R"), "{}", NoParams, has_active, |s, _| {
            edit(s, "crop.reset", "Reset Crop", |d| {
                d.crop = Default::default();
                Ok(())
            })
        }),
        // ---- HDR editing
        cmd!(
            "develop.hdr",
            "HDR",
            [],
            None,
            "{enabled?: bool (default: toggle), maxEv?: number 0..5 (headroom limit, stops above SDR white)} — HDR editing: highlights above SDR white for HDR exports and displays; every SDR render uses the SDR rendition (the hdr.sdr* controls)",
            HdrParams,
            has_active,
            |s, p| {
                let id = active(s, "develop.hdr")?;
                let cur = s.develop_of(id).unwrap_or_default().hdr;
                let enabled = p.enabled.unwrap_or(!cur.enabled);
                let max_ev = p.max_ev;
                let label = if enabled { "HDR On" } else { "HDR Off" };
                edit(s, "develop.hdr", label, |d| {
                    d.hdr.enabled = enabled;
                    if let Some(m) = max_ev {
                        d.hdr.max_ev = m.clamp(0.0, lightcraft_develop::Hdr::MAX_EV_LIMIT);
                    }
                    Ok(())
                })?;
                Ok(json!({"enabled": enabled, "maxEv": s.develop_of(id).map(|d| d.hdr.max_ev)}))
            }
        ),
        // ---- geometry: Upright
        cmd!(
            "geometry.upright",
            "Upright",
            [],
            None,
            "{mode: off|auto|guided|level|vertical|full} — analyses the photo (line segments → vanishing points) and stores the correction; run again to update after lens changes",
            UprightParams,
            has_active,
            |s, p| {
                let mode = p.mode.unwrap_or(Upright::Auto);
                let id = active(s, "geometry.upright")?;
                let d = s.develop_of(id).unwrap_or_default();
                let transform = match mode {
                    Upright::Off | Upright::Guided => None,
                    m => {
                        let src = s.source_now(id, SourceLevel::Preview).map_err(|e| bad("geometry.upright", e))?;
                        let info = s.source_info(id);
                        Some(lightcraft_pipeline::upright::auto_transform(&src, &info, &d, m))
                    }
                };
                edit(s, "geometry.upright", "Upright", |d| {
                    d.geometry.upright = mode;
                    d.geometry.upright_transform = transform;
                    Ok(())
                })?;
                Ok(json!({"mode": mode, "transform": transform}))
            }
        ),
        cmd!(
            "geometry.guides",
            "Guided Upright Guides",
            [],
            None,
            "{guides: [[x0,y0,x1,y1], …] (≤ 4, normalized coords of the lens-corrected image), add?: bool (append one guide, dropping the oldest beyond 4)} — switches Upright to Guided",
            Guides,
            has_active,
            |s, p| {
                let guides: Vec<(Point, Point)> = p.guides.iter().map(|[x0, y0, x1, y1]| (Point::new(*x0, *y0), Point::new(*x1, *y1))).collect();
                let add = p.add;
                edit(s, "geometry.guides", "Guided Upright", |d| {
                    if add {
                        d.geometry.guides.extend(guides);
                    } else {
                        d.geometry.guides = guides;
                    }
                    let n = d.geometry.guides.len();
                    if n > 4 {
                        d.geometry.guides.drain(..n - 4);
                    }
                    d.geometry.upright = Upright::Guided;
                    d.geometry.upright_transform = None;
                    Ok(())
                })
            }
        ),
        // ---- copy / paste / sync
        cmd!(
            "develop.copy",
            "Copy Edit Settings",
            ["Edit"],
            Some("Cmd+C"),
            "{groups?: [settingsGroup]} (default: all but crop, masks, remove)",
            Groups,
            has_active,
            |s, p| {
                let c = "develop.copy";
                let id = active(s, c)?;
                if let Some(g) = p.groups {
                    s.copy_groups = g;
                }
                let d = s.develop_of(id).unwrap_or_default();
                s.clipboard = Some(lightcraft_develop::extract_groups(&d, &s.copy_groups));
                Ok(json!({"groups": s.copy_groups}))
            }
        ),
        cmd!(
            "develop.pastePrevious",
            "Paste Settings from Previous",
            ["Edit"],
            Some("Cmd+Alt+V"),
            "{ids?, groups?: [settingsGroup] (default: the copy groups)} — from the photo that was active before this one",
            TargetGroups,
            has_active,
            |s, p| {
                let c = "develop.pastePrevious";
                let prev =
                    s.previous_active.filter(|id| s.catalog.photo(*id).is_some_and(|ph| !ph.deleted)).ok_or_else(|| bad(c, "no previous photo"))?;
                let groups = p.groups.clone().unwrap_or_else(|| s.copy_groups.clone());
                let partial = lightcraft_develop::extract_groups(&s.develop_of(prev).unwrap_or_default(), &groups);
                let ids: Vec<PhotoId> = p.targets(s).into_iter().filter(|id| *id != prev).collect();
                let ops = ids
                    .iter()
                    .filter_map(|id| s.develop_of(*id).map(|d| (*id, d)))
                    .filter_map(|(id, d)| s.develop_op(id, lightcraft_develop::apply_partial(&d, &partial, 1.0), "Paste from Previous"))
                    .collect::<Vec<_>>();
                let n = ops.len();
                s.commit("Paste from Previous", Op::Batch { ops })?;
                Ok(json!({"changed": n, "from": prev.0}))
            }
        ),
        cmd!(
            "develop.paste",
            "Paste Edit Settings",
            ["Edit"],
            Some("Cmd+V"),
            "{ids?, groups?: [settingsGroup] (paste only these of the copied groups)}",
            TargetGroups,
            has_clipboard,
            |s, p| {
                let mut clip = s.clipboard.clone().unwrap_or_default();
                if let Some(only) = p.groups.clone() {
                    // the clipboard holds whole groups: rebuild it with just the chosen (copied) ones
                    let only: Vec<_> = only.into_iter().filter(|g| s.copy_groups.contains(g)).collect();
                    let full = lightcraft_develop::apply_partial(&DevelopSettings::default(), &clip, 1.0);
                    clip = lightcraft_develop::extract_groups(&full, &only);
                }
                let ids = p.targets(s);
                let ops = ids
                    .iter()
                    .filter_map(|id| s.develop_of(*id).map(|d| (*id, d)))
                    .filter_map(|(id, d)| s.develop_op(id, lightcraft_develop::apply_partial(&d, &clip, 1.0), "Paste Settings"))
                    .collect::<Vec<_>>();
                let n = ops.len();
                s.commit("Paste Settings", Op::Batch { ops })?;
                Ok(json!({"changed": n}))
            }
        ),
        cmd!(
            "develop.sync",
            "Sync Settings",
            [],
            None,
            "{groups?} — copy the active photo's settings to all selected photos",
            Groups,
            has_selection,
            |s, p| {
                let c = "develop.sync";
                let id = active(s, c)?;
                let groups = p.groups.unwrap_or_else(SettingsGroup::default_copy);
                let d = s.develop_of(id).unwrap_or_default();
                let partial = lightcraft_develop::extract_groups(&d, &groups);
                let ops = s
                    .selection
                    .ids
                    .clone()
                    .into_iter()
                    .filter(|x| *x != id)
                    .filter_map(|x| s.develop_of(x).map(|d| (x, d)))
                    .filter_map(|(x, dd)| s.develop_op(x, lightcraft_develop::apply_partial(&dd, &partial, 1.0), "Sync Settings"))
                    .collect::<Vec<_>>();
                let n = ops.len();
                s.commit("Sync Settings", Op::Batch { ops })?;
                Ok(json!({"changed": n}))
            }
        ),
        cmd!(
            "develop.quickAdjust",
            "Quick Develop",
            [],
            None,
            "{control: develop control id, delta, ids?} — add `delta` to the control on every target photo (each from its own value; Quick Develop) in one undo step → {changed}",
            QuickAdjust,
            has_selection,
            |s, p| {
                let c = "develop.quickAdjust";
                let ctl = p.control.clone();
                let spec = controls::find(&ctl).ok_or_else(|| bad(c, format!("unknown control `{ctl}`")))?;
                let delta = p.delta;
                let label = format!("Quick Develop: {}", spec.label);
                let ops: Vec<Op> = p
                    .targets(s)
                    .into_iter()
                    .filter_map(|id| s.develop_of(id).map(|d| (id, d)))
                    .filter_map(|(id, d)| {
                        let mut nd = (*d).clone();
                        if ctl == "wb.temp" || ctl == "wb.tint" {
                            // each photo on its own scale: Kelvin (an old relative value converted first) or relative
                            let info = s.source_info(id);
                            lightcraft_pipeline::local::normalize_wb(&mut nd, &info);
                            if nd.wb.mode == WbMode::AsShot {
                                nd.wb.temp = info.as_shot_temp;
                                nd.wb.tint = info.as_shot_tint;
                            }
                        }
                        let before = nd.crop.geometry;
                        let v = controls::get(&nd, &ctl).unwrap_or(spec.default);
                        controls::set(&mut nd, &ctl, v + delta);
                        refit_crop_angle(ctl == CROP_ANGLE, before, &mut nd, crop_dims(s, id));
                        if ctl == "wb.temp" || ctl == "wb.tint" {
                            nd.wb.mode = WbMode::Custom;
                        }
                        s.develop_op(id, nd, &label)
                    })
                    .collect();
                let n = ops.len();
                if n > 0 {
                    s.commit(&label, Op::Batch { ops })?;
                }
                Ok(json!({"changed": n}))
            }
        ),
        cmd!(
            "develop.matchExposure",
            "Match Total Exposures",
            ["Photo"],
            None,
            "{ids?} — set each selected photo's Exposure so its total exposure (shutter × ISO ÷ aperture², plus the slider) equals the active photo's; photos without exposure data are skipped → {changed, skipped}",
            Targets,
            has_selection,
            |s, p| {
                let c = "develop.matchExposure";
                let reference = active(s, c)?;
                // log2 of the light the photo gathered (shutter s × ISO / f-number²)
                let gathered = |s: &Session, id: PhotoId| -> Option<f64> {
                    let m = &s.catalog.photo(id)?.meta;
                    let t = match m.shutter.split_once('/') {
                        Some((a, b)) => a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
                        None => m.shutter.trim().trim_end_matches('s').trim().parse::<f64>().ok()?,
                    };
                    let (n, iso) = (m.aperture? as f64, m.iso? as f64);
                    (t > 0.0 && n > 0.0 && iso > 0.0).then(|| (t * iso / (n * n)).log2())
                };
                let r_ev = gathered(s, reference).ok_or_else(|| bad(c, "the active photo has no shutter / aperture / ISO"))?;
                let r_slider = s.develop_of(reference).map(|d| d.light.exposure).unwrap_or(0.0);
                let ids: Vec<PhotoId> = p.targets(s).into_iter().filter(|x| *x != reference).collect();
                let (mut ops, mut skipped) = (Vec::new(), Vec::new());
                for id in ids {
                    let (Some(ev), Some(d)) = (gathered(s, id), s.develop_of(id)) else {
                        skipped.push(id.0);
                        continue;
                    };
                    let mut nd = (*d).clone();
                    nd.light.exposure = (r_slider + r_ev - ev).clamp(-5.0, 5.0);
                    ops.extend(s.develop_op(id, nd, "Match Total Exposures"));
                }
                let n = ops.len();
                if n > 0 {
                    s.commit("Match Total Exposures", Op::Batch { ops })?;
                }
                Ok(json!({"changed": n, "skipped": skipped}))
            }
        ),
        cmd!(
            "develop.autoSync",
            "Auto Sync",
            ["Edit"],
            Some("Cmd+Alt+Shift+A"),
            "{on?: bool} (toggles when omitted) — edits to the active photo also change the other selected photos (only the settings that changed; not spot removal / red eye) → {on}",
            On,
            always,
            |s, p| {
                s.auto_sync = p.on.unwrap_or(!s.auto_sync);
                Ok(json!({"on": s.auto_sync}))
            }
        ),
        // ---- presets
        cmd!("preset.apply", "Apply Preset", [], None, "{id: presetId, amount?: 0..200 (percent), ids?}", PresetApply, has_selection, |s, p| {
            let pid = p.id;
            let preset = s.presets.iter().find(|x| x.id == pid).cloned().ok_or_else(|| bad("preset.apply", format!("unknown preset `{pid}`")))?;
            let amount = p.amount.unwrap_or(100.0).clamp(0.0, 200.0) / 100.0;
            // `id` names the preset here: the photos are `ids`, else the selection
            let ids = s.targets_of(p.ids.as_deref(), None);
            let ops = ids
                .iter()
                .filter_map(|id| s.develop_of(*id).map(|d| (*id, d)))
                .filter_map(|(id, d)| s.develop_op(id, preset.apply(&d, amount), &format!("Preset: {}", preset.name)))
                .collect::<Vec<_>>();
            s.commit(&format!("Preset: {}", preset.name), Op::Batch { ops })?;
            ok()
        }),
        cmd!("preset.create", "Create Preset", ["Photo"], None, "{name, group?, groups?: [settingsGroup]}", PresetCreate, has_active, |s, p| {
            let c = "preset.create";
            let id = active(s, c)?;
            let name = p.name;
            let group = p.group.as_deref().unwrap_or("User Presets");
            let groups = p.groups.unwrap_or_else(SettingsGroup::default_copy);
            let d = s.develop_of(id).unwrap_or_default();
            let pid = format!("user.{}.{}", s.presets.len(), name.to_lowercase().replace(' ', "-"));
            s.presets.push(Preset::from_settings(&pid, &name, group, &d, &groups));
            Ok(json!({"id": pid}))
        }),
        cmd!("preset.delete", "Delete Preset", [], None, "{id}", PresetRef, always, |s, p| {
            let pid = p.id;
            let i = s.presets.iter().position(|x| x.id == pid).ok_or_else(|| bad("preset.delete", "unknown preset"))?;
            if s.presets[i].builtin {
                return Err(bad("preset.delete", "built-in presets can't be deleted"));
            }
            s.presets.remove(i);
            ok()
        }),
        cmd!("preset.favorite", "Favorite Preset", [], None, "{id, favorite?: bool}", Favorite, always, |s, p| {
            let pr = s.presets.iter_mut().find(|x| x.id == p.id).ok_or_else(|| bad("preset.favorite", "unknown preset"))?;
            pr.favorite = p.favorite.unwrap_or(!pr.favorite);
            ok()
        }),
        cmd!("preset.rename", "Rename Preset", [], None, "{id, name}", PresetRename, always, |s, p| {
            let c = "preset.rename";
            let name = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "missing name"))?.to_string();
            user_preset(s, &p.id, c)?.name = name;
            ok()
        }),
        cmd!("preset.move", "Move Preset to Group", [], None, "{id, group}", PresetMove, always, |s, p| {
            let c = "preset.move";
            let group = p.group.as_deref().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "missing group"))?.to_string();
            user_preset(s, &p.id, c)?.group = group;
            ok()
        }),
        cmd!(
            "preset.update",
            "Update Preset with Current Settings",
            [],
            None,
            "{id, groups?: [settingsGroup] (default: the groups the preset already sets)} — from the active photo",
            PresetUpdate,
            has_active,
            |s, p| {
                let c = "preset.update";
                let id = active(s, c)?;
                let current = s.develop_of(id).unwrap_or_default().to_json();
                let groups = p.groups;
                let pr = user_preset(s, &p.id, c)?;
                pr.settings = match groups {
                    Some(g) => lightcraft_develop::extract_groups(&serde_json::from_value(current).unwrap_or_default(), &g),
                    // the same keys as before, with the current values
                    None => {
                        let keys: Vec<String> = pr.settings.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
                        Value::Object(keys.into_iter().filter_map(|k| current.get(&k).cloned().map(|v| (k, v))).collect())
                    }
                };
                ok()
            }
        ),
        // ---- versions & history
        cmd!("version.create", "Create Version", ["Photo"], Some("Cmd+Shift+S"), "{name?}", Name, has_active, |s, p| {
            let id = active(s, "version.create")?;
            let ph = s.catalog.photo(id).cloned().ok_or_else(|| bad("version.create", "no photo"))?;
            let mut versions = ph.versions.clone();
            let name = p.name.unwrap_or_else(|| format!("Version {}", versions.len() + 1));
            versions.push(Version { name, created: (s.clock)(), settings: ph.develop.clone(), auto: false });
            s.commit("Create Version", Op::SetVersions { id, versions })?;
            ok()
        }),
        cmd!("version.restore", "Restore Version", [], None, "{index}", Index, has_active, |s, p| {
            let id = active(s, "version.restore")?;
            let i = p.index;
            let v = s.catalog.photo(id).and_then(|ph| ph.versions.get(i).cloned()).ok_or_else(|| bad("version.restore", "no such version"))?;
            s.set_develop(id, (*v.settings).clone(), &format!("Restore {}", v.name))?;
            ok()
        }),
        cmd!("version.delete", "Delete Version", [], None, "{index}", Index, has_active, |s, p| {
            let id = active(s, "version.delete")?;
            let i = p.index;
            let mut versions = s.catalog.photo(id).map(|ph| ph.versions.clone()).unwrap_or_default();
            if i >= versions.len() {
                return Err(bad("version.delete", "no such version"));
            }
            versions.remove(i);
            s.commit("Delete Version", Op::SetVersions { id, versions })?;
            ok()
        }),
        cmd!("version.rename", "Rename Version", [], None, "{index, name}", VersionRename, has_active, |s, p| {
            let c = "version.rename";
            let name = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "missing name"))?.to_string();
            edit_version(s, p.index, c, "Rename Version", |v, _| v.name = name)
        }),
        cmd!("version.update", "Update Version with Current Settings", [], None, "{index}", Index, has_active, |s, p| {
            edit_version(s, p.index, "version.update", "Update Version", |v, current| v.settings = current)
        }),
        cmd!("history.clear", "Clear History", [], None, "{} — keeps the current settings as the only step", NoParams, has_active, |s, _| {
            let id = active(s, "history.clear")?;
            let d = s.develop_of(id).unwrap_or_default();
            let history = vec![lightcraft_catalog::HistoryStep { label: "Cleared History".into(), settings: d }];
            s.commit("Clear History", Op::Batch { ops: vec![Op::SetHistory { id, history }, Op::SetHistoryCursor { id, cursor: None }] })?;
            ok()
        }),
        cmd!(
            "history.restore",
            "Go to History Step",
            [],
            None,
            "{index} — loads that step's settings and moves the History cursor there; later steps stay listed until a new edit",
            Index,
            has_active,
            |s, p| {
                let c = "history.restore";
                let id = active(s, c)?;
                let i = p.index;
                go_to_history_step(s, c, id, i)
            }
        ),
        cmd!(
            "history.previous",
            "Previous History Step",
            [],
            None,
            "{} — steps the History cursor back one step",
            NoParams,
            has_history_before,
            |s, _| {
                let c = "history.previous";
                let id = active(s, c)?;
                let i = s
                    .catalog
                    .photo(id)
                    .and_then(|ph| ph.history_index())
                    .and_then(|i| i.checked_sub(1))
                    .ok_or_else(|| bad(c, "at the first History step"))?;
                go_to_history_step(s, c, id, i)
            }
        ),
        cmd!(
            "history.next",
            "Next History Step",
            [],
            None,
            "{} — steps the History cursor forward one step",
            NoParams,
            has_history_after,
            |s, _| {
                let c = "history.next";
                let id = active(s, c)?;
                let ph = s.catalog.photo(id).ok_or_else(|| bad(c, "no photo"))?;
                let i = ph
                    .history_index()
                    .map(|i| i.saturating_add(1))
                    .filter(|i| *i < ph.history.len())
                    .ok_or_else(|| bad(c, "at the latest History step"))?;
                go_to_history_step(s, c, id, i)
            }
        ),
        // ---- interactions (slider drags, brush strokes)
        cmd!("develop.beginInteraction", "Begin Interaction", [], None, "{label}", Label, has_active, |s, p| {
            s.begin_interaction(p.label.as_deref().unwrap_or("Edit"))?;
            ok()
        }),
        cmd!("develop.endInteraction", "End Interaction", [], None, "{}", NoParams, always, |s, _| {
            s.end_interaction()?;
            ok()
        }),
        cmd!("develop.cancelInteraction", "Cancel Interaction", [], None, "{}", NoParams, always, |s, _| {
            s.cancel_interaction()?;
            ok()
        }),
    ]
}

/// Load History step `i` of photo `id` and put the cursor there, without adding a step: the later
/// steps stay listed (one undo step).
fn go_to_history_step(s: &mut Session, c: &str, id: PhotoId, i: usize) -> Result<Value> {
    let ph = s.catalog.photo(id).ok_or_else(|| bad(c, "no photo"))?;
    let st = ph.history.get(i).cloned().ok_or_else(|| bad(c, "no such step"))?;
    let cursor = (i.saturating_add(1) < ph.history.len()).then_some(i);
    let label = format!("History: {}", st.label);
    let edited = Some((s.clock)());
    s.commit(
        &label,
        Op::Batch { ops: vec![Op::SetDevelop { id, settings: st.settings, label: label.clone(), edited }, Op::SetHistoryCursor { id, cursor }] },
    )?;
    ok()
}

fn has_history_before(s: &Session) -> std::result::Result<(), String> {
    let at = s.active().and_then(|id| s.catalog.photo(id)).and_then(|p| p.history_index());
    if at.is_some_and(|i| i > 0) { Ok(()) } else { Err("at the first History step".into()) }
}

fn has_history_after(s: &Session) -> std::result::Result<(), String> {
    let photo = s.active().and_then(|id| s.catalog.photo(id));
    if photo.is_some_and(|p| p.history_index().is_some_and(|i| i.saturating_add(1) < p.history.len())) {
        Ok(())
    } else {
        Err("at the latest History step".into())
    }
}
