//! Library preferences (Settings → Import / Performance): develop defaults applied on import and
//! the thumbnail cache budget. Saved in the library's `prefs.json` (see [`crate::library`]).

use serde_json::{Value, json};

use super::params::{params, present};
use super::{CommandSpec, always, bad, cmd};
use crate::import::{CameraDefault, ImportDefaults};
use crate::{Result, Session};

const ID: &str = "library.preferences";

fn current(s: &Session) -> Value {
    json!({
        "xmp": s.prefs.xmp,
        "import": s.prefs.import_defaults,
        "cacheMb": if s.prefs.cache_mb == 0 { s.cache_bytes() >> 20 } else { u64::from(s.prefs.cache_mb) },
        "forgetLocalDays": s.prefs.forget_local_days,
        "persistent": s.library.is_some(),
    })
}

/// `"default"`, `""` or null = no preset; anything else must name an existing preset.
fn preset_ref(s: &Session, v: &Value) -> Result<Option<String>> {
    match v {
        Value::Null => Ok(None),
        Value::String(x) if x.is_empty() || x == "default" => Ok(None),
        Value::String(x) if s.presets.iter().any(|p| p.id == *x) => Ok(Some(x.clone())),
        Value::String(x) => Err(bad(ID, format!("unknown preset `{x}` (see preset.list)"))),
        other => Err(bad(ID, format!("preset must be a preset id or \"default\", not {other}"))),
    }
}

params! {
    /// `library.preferences`
    struct Prefs {
        import: Option<ImportPrefs>,
        camera: Option<CameraPref>,
        forget_local_days: Option<u64>,
        cache_mb: Option<u64>,
    }
}

/// `library.preferences {import}`: what's left out keeps its value; a preset `null` is none.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportPrefs {
    #[serde(default, deserialize_with = "present")]
    raw_preset: Option<Option<Value>>,
    #[serde(default, deserialize_with = "present")]
    other_preset: Option<Option<Value>>,
    per_camera: Option<bool>,
    show_on_card: Option<bool>,
    copyright: Option<String>,
    creator: Option<String>,
    #[serde(default, deserialize_with = "present")]
    metadata_preset: Option<Option<String>>,
    cameras: Option<Vec<CameraPref>>,
}

/// One camera's import default (`remove` only in `library.preferences {camera}`).
#[derive(Debug, serde::Deserialize)]
struct CameraPref {
    camera: Option<String>,
    preset: Option<Value>,
    remove: Option<bool>,
}

fn prefs(s: &mut Session, p: Prefs) -> Result<Value> {
    let mut d: ImportDefaults = s.prefs.import_defaults.clone();
    let mut changed = false;
    if let Some(i) = p.import {
        if let Some(v) = i.raw_preset {
            d.raw_preset = preset_ref(s, &v.unwrap_or(Value::Null))?;
        }
        if let Some(v) = i.other_preset {
            d.other_preset = preset_ref(s, &v.unwrap_or(Value::Null))?;
        }
        if let Some(v) = i.per_camera {
            d.per_camera = v;
        }
        if let Some(v) = i.show_on_card {
            d.show_on_card = v;
        }
        if let Some(v) = i.copyright {
            d.copyright = v.trim().to_string();
        }
        if let Some(v) = i.creator {
            d.creator = v.trim().to_string();
        }
        match i.metadata_preset {
            Some(Some(n)) if n.is_empty() || n == "none" => d.metadata_preset = None,
            Some(Some(n)) => {
                if !s.prefs.metadata_presets.iter().any(|m| m.name.eq_ignore_ascii_case(&n)) {
                    return Err(bad(ID, format!("unknown metadata preset `{n}` (see metadata.presets)")));
                }
                d.metadata_preset = Some(n);
            }
            Some(None) => d.metadata_preset = None,
            None => {}
        }
        if let Some(a) = i.cameras {
            d.cameras.clear();
            for c in a {
                if c.remove.is_some() {
                    return Err(bad(ID, "unknown parameter `import.cameras[].remove` (camera.remove takes one off)"));
                }
                let camera = c.camera.as_deref().unwrap_or_default().trim().to_string();
                if camera.is_empty() {
                    return Err(bad(ID, "each camera default needs `camera` (make + model)"));
                }
                let preset = preset_ref(s, c.preset.as_ref().unwrap_or(&Value::Null))?;
                d.cameras.push(CameraDefault { camera, preset });
            }
        }
        changed = true;
    }
    // set or clear one camera's default: {camera: {camera: "Make Model", preset: id|null}}
    if let Some(c) = p.camera {
        let camera = c.camera.as_deref().unwrap_or_default().trim().to_string();
        if camera.is_empty() {
            return Err(bad(ID, "`camera.camera` (make + model) is required"));
        }
        let preset = preset_ref(s, c.preset.as_ref().unwrap_or(&Value::Null))?;
        d.cameras.retain(|x| !x.camera.eq_ignore_ascii_case(&camera));
        if c.remove != Some(true) {
            d.cameras.push(CameraDefault { camera, preset });
        }
        changed = true;
    }
    s.prefs.import_defaults = d;
    if let Some(days) = p.forget_local_days {
        s.prefs.forget_local_days = days.min(36_500) as u32;
        changed = true;
    }
    if let Some(mb) = p.cache_mb {
        s.set_cache_mb(mb.clamp(0, 1 << 20) as u32)?;
    } else if changed {
        s.save_prefs()?;
    }
    Ok(current(s))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "library.preferences",
        "Library Preferences",
        [],
        None,
        "{import?: {rawPreset?: presetId|\"default\", otherPreset?: presetId|\"default\", perCamera?: bool, cameras?: [{camera: \"Make Model\", preset: presetId|null}], copyright?: text, creator?: text (given to imported photos without one), showOnCard?: bool (open the import review when a camera or card is connected)}, camera?: {camera, preset?, remove?: bool}, cacheMb?: n (0 = default), forgetLocalDays?: n (forget untouched Local photos of folders not browsed for n days; 0 = never)} — develop defaults applied on import (raws / other images / per camera) and the thumbnail cache size, saved with the library → {xmp, import, cacheMb, forgetLocalDays, persistent}",
        Prefs,
        always,
        prefs
    )]
}
