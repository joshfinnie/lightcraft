//! Library-wide keyword commands: list (tree with counts), suggestions, rename, delete, merge;
//! keyword sets (nine keywords a keystroke away: ⌥1–⌥9) and Recent Keywords.

use lightcraft_catalog::keywords::{KeywordInfo, clean, closest, is_under, reparent};
use serde_json::{Value, json};

use super::params::{NoParams, Strings, params, present};
use super::{CommandSpec, always, bad, cmd};
use crate::{Result, Session};

/// Commit a keyword batch; returns `{changed: photos changed, listed: keyword list changes}`. The
/// keyword filter follows a renamed keyword and is cleared when its keyword is deleted.
fn commit_keywords(s: &mut Session, label: &str, op: lightcraft_catalog::Op, follow: impl Fn(&str) -> Option<String>) -> Result<Value> {
    use lightcraft_catalog::Op;
    let (photos, listed) = match &op {
        Op::Batch { ops } => {
            (ops.iter().filter(|o| matches!(o, Op::SetMeta { .. })).count(), ops.iter().filter(|o| matches!(o, Op::SetKeyword { .. })).count())
        }
        Op::SetMeta { .. } => (1, 0),
        _ => (0, 1),
    };
    if photos + listed > 0 {
        s.commit(label, op)?;
    }
    if let Some(k) = s.filter.keyword.clone() {
        s.filter.keyword = follow(&k);
    }
    Ok(json!({"changed": photos, "listed": listed}))
}

/// A keyword's attributes as `keyword.create` / `keyword.edit` take them.
struct Attrs<'a> {
    synonyms: &'a Option<Strings>,
    include_on_export: Option<bool>,
    export_containing: Option<bool>,
    export_synonyms: Option<bool>,
    person: Option<bool>,
}

/// A keyword's attributes from command params over `base`: those not given keep their value.
fn info_from(p: Attrs, base: KeywordInfo) -> KeywordInfo {
    KeywordInfo {
        synonyms: match p.synonyms {
            Some(s) => s.to_vec().into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            None => base.synonyms,
        },
        include_on_export: p.include_on_export.unwrap_or(base.include_on_export),
        export_containing: p.export_containing.unwrap_or(base.export_containing),
        export_synonyms: p.export_synonyms.unwrap_or(base.export_synonyms),
        person: p.person.unwrap_or(base.person),
        new_keywords_inside: base.new_keywords_inside,
    }
}

/// The keyword a command names (`keyword`), as the library writes it.
fn named_keyword(s: &Session, k: Option<&str>, command: &str) -> Result<String> {
    let k = k.ok_or_else(|| bad(command, "missing `keyword`"))?;
    s.catalog.keyword_path(k).ok_or_else(|| bad(command, format!("no keyword “{}”", clean(k))))
}

/// A catalog refusal as a command error; a taken name says how to merge.
fn refused(command: &str, e: lightcraft_catalog::CatalogError, how_to_merge: &str) -> crate::EngineError {
    match e {
        lightcraft_catalog::CatalogError::KeywordExists(k) => bad(command, format!("there is a keyword “{k}” already{how_to_merge}")),
        e => bad(command, e.to_string()),
    }
}

/// A named set of up to nine keywords.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeywordSet {
    pub name: String,
    pub keywords: Vec<String>,
}

/// Two set names (or recent keywords) are the same whatever the case of any of their letters, as
/// keywords are.
fn same_set(a: &str, b: &str) -> bool {
    lightcraft_catalog::keywords::same(a.trim(), b.trim())
}

/// The set name meaning "the nine most recently added keywords".
pub const RECENT: &str = "Recent Keywords";

/// Remember `added` as the most recent keywords (newest first, nine kept).
pub fn note_recent(s: &mut Session, added: &[String]) {
    for k in added.iter().rev() {
        let k = clean(k);
        if k.is_empty() {
            continue;
        }
        s.recent_keywords.retain(|x| !same_set(x, &k));
        s.recent_keywords.insert(0, k);
    }
    s.recent_keywords.truncate(9);
    let _ = s.save_prefs();
}

/// The largest keyword list file read: a file is input (a list of 100,000 keywords is ~2 MB).
const MAX_LIST_BYTES: u64 = 16 << 20;

/// The characters Capture One's keyword importer refuses in a list (`|` can't be in a name here).
const CAPTURE_ONE_REFUSES: [char; 4] = [';', ',', '<', '>'];

/// A keyword list file's text: at most `MAX_LIST_BYTES`, UTF-8 (with or without a byte-order mark).
fn read_list(path: &str) -> Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| bad("keyword.import", format!("{path}: {e}")))?;
    let mut bytes = Vec::new();
    file.take(MAX_LIST_BYTES + 1).read_to_end(&mut bytes).map_err(|e| bad("keyword.import", format!("{path}: {e}")))?;
    if bytes.len() as u64 > MAX_LIST_BYTES {
        return Err(bad("keyword.import", format!("{path}: larger than {} MB, not a keyword list", MAX_LIST_BYTES >> 20)));
    }
    // Notepad and some exporters save UTF-16: say how to get the file in, not just that it isn't
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(bad("keyword.import", format!("{path}: UTF-16 text — save it as UTF-8 and import it again")));
    }
    String::from_utf8(bytes).map_err(|_| bad("keyword.import", format!("{path}: not UTF-8 text")))
}

/// A set's slots as typed: each cleaned in its place (an empty one is an empty slot, so the others
/// keep their ⌥ keys), each keyword once whatever its case (a second one leaves its slot empty),
/// nine at most, no empty ones at the end.
pub fn slots(typed: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in typed.iter().take(9) {
        let k = clean(k);
        let twice = !k.is_empty() && out.iter().any(|x| lightcraft_catalog::keywords::same(x, &k));
        out.push(if twice { String::new() } else { k });
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// The nine keywords ⌥1–⌥9 apply: the current set's, or the recent ones.
pub fn current_keywords(s: &Session) -> Vec<String> {
    let set = s.keyword_set.as_deref().and_then(|n| s.keyword_sets.iter().find(|x| same_set(&x.name, n)));
    let mut v = set.map_or_else(|| s.recent_keywords.clone(), |x| x.keywords.clone());
    v.truncate(9);
    v
}

/// `{sets: [{name, keywords}], current, keywords}` (Recent Keywords first).
pub fn keyword_sets_json(s: &Session) -> Value {
    let mut sets = vec![json!({"name": RECENT, "keywords": s.recent_keywords})];
    sets.extend(s.keyword_sets.iter().map(|x| json!({"name": x.name, "keywords": x.keywords})));
    json!({"sets": sets, "current": s.keyword_set.clone().unwrap_or_else(|| RECENT.into()), "keywords": current_keywords(s)})
}

params! {
    /// `keyword.useSet`, `keyword.deleteSet`
    struct SetName {
        name: Option<String>,
    }
}

params! {
    /// `keyword.saveSet`
    struct SaveSet {
        name: Option<String>,
        keywords: Option<Strings>,
        replace: Option<String>,
        #[serde(default)]
        new: bool,
    }
}

params! {
    /// `keyword.toggleFromSet`
    struct ToggleFromSet with targets {
        index: u64,
    }
}

params! {
    /// `keyword.suggest`
    struct Suggest with targets {
        prefix: Option<String>,
        limit: Option<usize>,
    }
}

params! {
    /// Commands on one keyword.
    struct KeywordRef {
        keyword: Option<String>,
    }
}

params! {
    /// `keyword.create`: `parent` left out is the default parent, `null` the top level.
    struct Create with targets {
        name: Option<String>,
        #[serde(default, deserialize_with = "present")]
        parent: Option<Option<String>>,
        synonyms: Option<Strings>,
        include_on_export: Option<bool>,
        export_containing: Option<bool>,
        export_synonyms: Option<bool>,
        person: Option<bool>,
        #[serde(default)]
        add_to_selected: bool,
    }
}

params! {
    /// `keyword.edit`
    struct Edit {
        keyword: Option<String>,
        name: Option<String>,
        synonyms: Option<Strings>,
        include_on_export: Option<bool>,
        export_containing: Option<bool>,
        export_synonyms: Option<bool>,
        person: Option<bool>,
    }
}

params! {
    /// `keyword.move`: `parent` is required, `null` for the top level.
    struct Move {
        keyword: Option<String>,
        #[serde(default, deserialize_with = "present")]
        parent: Option<Option<String>>,
        #[serde(default)]
        merge: bool,
    }
}

params! {
    /// `keyword.export`
    struct ExportList {
        path: Option<String>,
    }
}

params! {
    /// `keyword.import`
    struct ImportList {
        path: Option<String>,
        text: Option<String>,
    }
}

params! {
    /// `keyword.rename`
    struct Rename {
        from: String,
        to: String,
    }
}

params! {
    /// `keyword.merge`
    struct Merge {
        from: Option<Strings>,
        into: String,
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "keyword.sets", "Keyword Sets", [], None, "{} → {sets: [{name, keywords}], current, keywords: the nine ⌥1–⌥9 apply}", NoParams, always, |s, _| Ok(keyword_sets_json(s))),
        cmd!("keyword.useSet", "Use Keyword Set", [], None, "{name} (\"Recent Keywords\" = the recently added ones)", SetName, always, |s, p| {
            let name = p.name.as_deref().map(str::trim).unwrap_or(RECENT);
            s.keyword_set = if same_set(name, RECENT) || name.is_empty() {
                None
            } else {
                Some(
                    s.keyword_sets
                        .iter()
                        .find(|x| same_set(&x.name, name))
                        .ok_or_else(|| bad("keyword.useSet", format!("no keyword set `{name}`")))?
                        .name
                        .clone(),
                )
            };
            s.save_prefs()?;
            Ok(keyword_sets_json(s))
        }),
        cmd!(
            "keyword.saveSet",
            "Save Keyword Set",
            [],
            None,
            "{name, keywords?: [up to 9, \"\" = an empty slot] (default: the renamed set's, else the current nine), replace?: the set it renames, new?: refuse a name another set has} — replaces a set of that name (or `replace`, in its place) and makes it current",
            SaveSet,
            always,
            |s, p| {
                let name = p
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|n| !n.is_empty() && !same_set(n, RECENT))
                    .ok_or_else(|| bad("keyword.saveSet", "missing or reserved `name`"))?
                    .to_string();
                let same_name = |x: &KeywordSet, n: &str| same_set(&x.name, n);
                // the set it renames, if any (an empty `replace` is none)
                let renames = match p.replace.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
                    Some(old) => Some(
                        s.keyword_sets
                            .iter()
                            .position(|x| same_name(x, old))
                            .ok_or_else(|| bad("keyword.saveSet", format!("no keyword set `{old}`")))?,
                    ),
                    None => None,
                };
                // keywords not given: the renamed set's own, else the current nine
                let keywords = if let Some(k) = &p.keywords {
                    slots(&k.to_vec())
                } else {
                    match renames.and_then(|at| s.keyword_sets.get(at)) {
                        Some(x) => x.keywords.clone(),
                        None => current_keywords(s),
                    }
                };
                let set = KeywordSet { name: name.clone(), keywords };
                match renames {
                    // rename: in its place, onto a name no other set has
                    Some(at) => {
                        if s.keyword_sets.iter().enumerate().any(|(i, x)| i != at && same_name(x, &name)) {
                            return Err(bad("keyword.saveSet", format!("there is a keyword set “{name}” already")));
                        }
                        if let Some(x) = s.keyword_sets.get_mut(at) {
                            *x = set;
                        }
                    }
                    // a new set (`new`) never takes another set's name
                    None if p.new && s.keyword_sets.iter().any(|x| same_name(x, &name)) => {
                        return Err(bad("keyword.saveSet", format!("there is a keyword set “{name}” already")));
                    }
                    None => match s.keyword_sets.iter_mut().find(|x| same_name(x, &name)) {
                        Some(x) => *x = set,
                        None => s.keyword_sets.push(set),
                    },
                }
                s.keyword_set = Some(name);
                s.save_prefs()?;
                Ok(keyword_sets_json(s))
            }
        ),
        cmd!("keyword.deleteSet", "Delete Keyword Set", [], None, "{name}", SetName, always, |s, p| {
            let name = p.name.as_deref().ok_or_else(|| bad("keyword.deleteSet", "missing `name`"))?;
            let before = s.keyword_sets.len();
            s.keyword_sets.retain(|x| !same_set(&x.name, name));
            if s.keyword_sets.len() == before {
                return Err(bad("keyword.deleteSet", format!("no keyword set `{name}`")));
            }
            if s.keyword_set.as_deref().is_some_and(|c| same_set(c, name)) {
                s.keyword_set = None;
            }
            s.save_prefs()?;
            Ok(keyword_sets_json(s))
        }),
        cmd!(
            "keyword.toggleFromSet",
            "Toggle Keyword from Set",
            [],
            None,
            "{index: 1..9, ids?} — the set's keyword N: added to the target photos, or removed when they all have it",
            ToggleFromSet,
            super::has_selection,
            |s, p| {
                let i = Some(p.index).filter(|i| (1..=9).contains(i)).ok_or_else(|| bad("keyword.toggleFromSet", "`index` must be 1..9"))?;
                // (an empty slot does nothing)
                let Some(k) = current_keywords(s).get(i as usize - 1).cloned().filter(|k| !k.is_empty()) else {
                    return Ok(json!({"changed": 0}));
                };
                let ids = p.targets(s);
                let all = !ids.is_empty()
                    && ids.iter().all(|id| {
                        s.catalog
                            .photo(*id)
                            .is_some_and(|ph| ph.meta.keywords.iter().any(|x| lightcraft_catalog::keywords::same(&clean(x), &clean(&k))))
                    });
                let ids: Vec<u64> = ids.iter().map(|i| i.0).collect();
                let key = if all { "removeKeywords" } else { "addKeywords" };
                // applying from Recent Keywords mustn't reshuffle the numbers under the keys
                let recent = s.recent_keywords.clone();
                let mut r = s.execute("photo.setMeta", &json!({"ids": ids, key: [k.clone()]}))?;
                if s.keyword_set.is_none() {
                    s.recent_keywords = recent;
                }
                r["keyword"] = json!(k);
                r["added"] = json!(!all);
                Ok(r)
            }
        ),
        cmd!(query "keyword.list", "Keywords", [], None, "{} → [{name, path, count, children}] keyword tree (`a|b|c` keywords are hierarchical)", NoParams, always, |s, _| {
            Ok(serde_json::to_value(s.catalog.keyword_tree()).unwrap_or_default())
        }),
        cmd!(
            query "keyword.suggest",
            "Keyword Suggestions",
            [],
            None,
            "{prefix?: typed text, ids?, limit?: 12} → keywords to suggest for the photos (co-occurring / most used, or matching the prefix)",
            Suggest,
            always,
            |s, p| {
                let mut current: Vec<String> = Vec::new();
                for id in p.targets(s) {
                    if let Some(ph) = s.catalog.photo(id) {
                        current.extend(ph.meta.keywords.iter().cloned());
                    }
                }
                let n = p.limit.unwrap_or(12);
                Ok(json!(s.catalog.keyword_suggestions(&current, p.prefix.as_deref().unwrap_or(""), n)))
            }
        ),
        cmd!(
            query "keyword.info",
            "Keyword Info",
            [],
            None,
            "{keyword} → {path, listed, count, synonyms, includeOnExport, exportContaining, exportSynonyms, person}",
            KeywordRef,
            always,
            |s, p| {
                let path = named_keyword(s, p.keyword.as_deref(), "keyword.info")?;
                let info = s.catalog.keyword_info(&path).cloned();
                let count = s.catalog.photos().filter(|ph| ph.in_library() && ph.meta.keywords.iter().any(|k| is_under(k, &path))).count();
                let mut v = serde_json::to_value(info.clone().unwrap_or_default()).unwrap_or_default();
                if let Some(o) = v.as_object_mut() {
                    o.insert("path".into(), json!(path));
                    o.insert("listed".into(), json!(info.is_some()));
                    o.insert("count".into(), json!(count));
                    o.entry("synonyms").or_insert(json!([]));
                }
                Ok(v)
            }
        ),
        cmd!(
            "keyword.create",
            "Create Keyword",
            [],
            None,
            "{name, parent?: keyword | null (default: the default parent; null = the top level), synonyms?, includeOnExport?, exportContaining?, exportSynonyms?, person?, addToSelected?: bool, ids?} → {keyword}",
            Create,
            always,
            |s, p| {
                let name = p.name.as_deref().map(clean).filter(|n| !n.is_empty()).ok_or_else(|| bad("keyword.create", "missing `name`"))?;
                let parent = match &p.parent {
                    Some(Some(k)) => Some(s.catalog.keyword_path(k).unwrap_or_else(|| clean(k))),
                    Some(None) => None,
                    None => s.catalog.default_keyword_parent(),
                };
                let path = clean(&match parent {
                    Some(parent) => format!("{parent}|{name}"),
                    None => name,
                });
                let photos = if p.add_to_selected { p.targets(s) } else { Vec::new() };
                let attrs = Attrs {
                    synonyms: &p.synonyms,
                    include_on_export: p.include_on_export,
                    export_containing: p.export_containing,
                    export_synonyms: p.export_synonyms,
                    person: p.person,
                };
                let op = s
                    .catalog
                    .create_keyword_ops(&path, info_from(attrs, KeywordInfo::default()), &photos)
                    .map_err(|e| refused("keyword.create", e, ""))?;
                s.commit("Create Keyword", op)?;
                if !photos.is_empty() {
                    note_recent(s, std::slice::from_ref(&path));
                }
                Ok(json!({"keyword": path}))
            }
        ),
        cmd!(
            "keyword.edit",
            "Edit Keyword",
            [],
            None,
            "{keyword, name?: its new name (one level), synonyms?, includeOnExport?, exportContaining?, exportSynonyms?, person?} — what isn't given keeps its value",
            Edit,
            always,
            |s, p| {
                let from = named_keyword(s, p.keyword.as_deref(), "keyword.edit")?;
                let leaf = from.rsplit('|').next().unwrap_or(&from).to_string();
                let name = p.name.clone().unwrap_or(leaf);
                let attrs = Attrs {
                    synonyms: &p.synonyms,
                    include_on_export: p.include_on_export,
                    export_containing: p.export_containing,
                    export_synonyms: p.export_synonyms,
                    person: p.person,
                };
                let info = info_from(attrs, s.catalog.keyword_info(&from).cloned().unwrap_or_default());
                let op =
                    s.catalog.edit_keyword_ops(&from, &name, info).map_err(|e| refused("keyword.edit", e, " (to merge them, use keyword.merge)"))?;
                let to = match from.rsplit_once('|') {
                    Some((parent, _)) => format!("{parent}|{}", name.trim()),
                    None => name.trim().to_string(),
                };
                commit_keywords(s, "Edit Keyword", op, |k| Some(if is_under(k, &from) { reparent(k, &from, &to) } else { k.to_string() }))
            }
        ),
        cmd!(
            "keyword.move",
            "Move Keyword",
            [],
            None,
            "{keyword, parent: keyword | null (the top level), merge?: bool} — with the keywords below it; onto a name that is taken only with `merge: true`",
            Move,
            always,
            |s, p| {
                let from = named_keyword(s, p.keyword.as_deref(), "keyword.move")?;
                let parent = p.parent.ok_or_else(|| bad("keyword.move", "`parent` is a keyword, or null for the top level"))?;
                // where it lands, as the catalog spells it: a parent that doesn't exist yet is made
                let leaf = from.rsplit('|').next().unwrap_or(&from).to_string();
                let to = match parent.as_deref().map(|k| s.catalog.keyword_path(k).unwrap_or_else(|| clean(k))).filter(|k| !k.is_empty()) {
                    Some(parent) => format!("{parent}|{leaf}"),
                    None => leaf,
                };
                let op = s
                    .catalog
                    .move_keyword_ops(&from, parent.as_deref(), p.merge)
                    .map_err(|e| refused("keyword.move", e, ": moving would merge the two (merge: true)"))?;
                commit_keywords(s, "Move Keyword", op, |k| Some(if is_under(k, &from) { reparent(k, &from, &to) } else { k.to_string() }))
            }
        ),
        cmd!(
            "keyword.purgeUnused",
            "Purge Unused Keywords",
            [],
            None,
            "{} — takes off the keyword list the keywords no photo has → {purged}",
            NoParams,
            always,
            |s, _| {
                let op = s.catalog.purge_unused_keywords_ops();
                let r = commit_keywords(s, "Purge Unused Keywords", op, |k| Some(k.to_string()))?;
                Ok(json!({"purged": r["listed"]}))
            }
        ),
        cmd!(
            "keyword.setDefaultParent",
            "Put New Keywords Inside",
            [],
            None,
            "{keyword: keyword | null} — the parent keyword.create puts new keywords inside (null: the top level)",
            KeywordRef,
            always,
            |s, p| {
                let keyword = match p.keyword.as_deref() {
                    None => None,
                    k => Some(named_keyword(s, k, "keyword.setDefaultParent")?),
                };
                let op = s.catalog.set_default_parent_ops(keyword.as_deref()).map_err(|e| bad("keyword.setDefaultParent", e.to_string()))?;
                commit_keywords(s, "Put New Keywords Inside", op, |k| Some(k.to_string()))?;
                Ok(json!({"keyword": s.catalog.default_keyword_parent()}))
            }
        ),
        cmd!(
            "keyword.export",
            "Export Keywords",
            [],
            None,
            "{path?} — the keyword list as a file (Lightroom Classic's format, which Capture One, Photo Supreme and Bridge read) → {keywords, captureOneRefuses: keywords with ; , < > that Capture One's importer refuses, unwritable: keywords left out because the format can't hold them (a name in brackets or braces, a line break, deeper than 64 levels; a synonym as \"path {synonym}\"), text when there is no path}",
            ExportList,
            always,
            |s, p| {
                let file = s.catalog.keyword_list_text();
                // Capture One's importer refuses ; , < > in a keyword's name
                let refuses: Vec<&str> = file
                    .written
                    .iter()
                    .filter_map(|p| p.rsplit(lightcraft_catalog::keywords::SEP).next())
                    .filter(|n| n.contains(CAPTURE_ONE_REFUSES))
                    .collect();
                let mut r = json!({"keywords": file.written.len(), "captureOneRefuses": refuses, "unwritable": file.unwritable});
                match p.path.as_deref().map(str::trim).filter(|x| !x.is_empty()) {
                    Some(path) => {
                        std::fs::write(path, &file.text).map_err(|e| crate::EngineError::Other(format!("{path}: {e}")))?;
                        r["path"] = json!(path);
                    }
                    None => r["text"] = json!(file.text),
                }
                Ok(r)
            }
        ),
        cmd!(
            "keyword.import",
            "Import Keywords",
            [],
            None,
            "{path | text} — a keyword list (Lightroom Classic's format, also Capture One's and Photo Supreme's Formatted Vocabulary File): adds the keywords the library doesn't have, gives those it has the list's synonyms; one undo step → {added, updated}",
            ImportList,
            always,
            |s, p| {
                let text = match (p.path.as_deref().map(str::trim).filter(|x| !x.is_empty()), p.text.as_deref()) {
                    (Some(path), _) => read_list(path)?,
                    (None, Some(text)) if text.len() as u64 > MAX_LIST_BYTES => {
                        return Err(bad("keyword.import", format!("larger than {} MB, not a keyword list", MAX_LIST_BYTES >> 20)));
                    }
                    (None, Some(text)) => text.to_string(),
                    (None, None) => return Err(bad("keyword.import", "give `path` or `text`")),
                };
                let entries = lightcraft_catalog::keywords::parse_keyword_list(&text).map_err(|e| bad("keyword.import", e.to_string()))?;
                let (op, added, updated) = s.catalog.import_keywords_ops(&entries);
                commit_keywords(s, "Import Keywords", op, |k| Some(k.to_string()))?;
                Ok(json!({"added": added, "updated": updated}))
            }
        ),
        cmd!(
            "keyword.rename",
            "Rename Keyword",
            [],
            None,
            "{from, to} — on every photo, children included (`a` → `b` renames `a|x` to `b|x`); renaming onto an existing keyword merges them",
            Rename,
            always,
            |s, p| {
                let (from, to) = (p.from, p.to);
                let op = s.catalog.rename_keyword_ops(&from, &to).map_err(|e| bad("keyword.rename", e.to_string()))?;
                let (f, t) = (clean(&from), clean(&to));
                commit_keywords(s, "Rename Keyword", op, |k| Some(if is_under(k, &f) { reparent(k, &f, &t) } else { k.to_string() }))
            }
        ),
        cmd!(
            "keyword.delete",
            "Delete Keyword",
            [],
            None,
            "{keyword} — removes it (and the keywords below it) from every photo",
            KeywordRef,
            always,
            |s, p| {
                let k = p.keyword.ok_or_else(|| bad("keyword.delete", "missing `keyword`"))?;
                let op = s.catalog.delete_keyword_ops(&k).map_err(|e| bad("keyword.delete", e.to_string()))?;
                let c = clean(&k);
                commit_keywords(s, "Delete Keyword", op, |f| (!is_under(f, &c)).then(|| f.to_string()))
            }
        ),
        cmd!(
            "keyword.merge",
            "Merge Keywords",
            [],
            None,
            "{from: [keyword], into: keyword} — replaces each `from` keyword (children included) with `into` on every photo",
            Merge,
            always,
            |s, p| {
                let from = p.from.map(|f| f.to_vec()).unwrap_or_default();
                let into = p.into;
                let op = s.catalog.merge_keywords_ops(&from, &into).map_err(|e| bad("keyword.merge", e.to_string()))?;
                let (from, into) = (from.iter().map(|f| clean(f)).collect::<Vec<_>>(), clean(&into));
                commit_keywords(s, "Merge Keywords", op, |k| {
                    Some(match closest(k, &from) {
                        Some(f) => reparent(k, f, &into),
                        None => k.to_string(),
                    })
                })
            }
        ),
    ]
}
