//! Library commands: view source, filter/sort, selection, ratings/flags/labels, rotate, delete,
//! metadata, albums, import.

use lightcraft_catalog::{Album, AlbumId, ColorLabel, CopyrightStatus, Filter, Flag, GroupBy, Op, PhotoId, Sort, SortKey};
use serde_json::{Value, json};

use super::params::{NoParams, Patch, params};
use super::{CommandSpec, always, bad, cmd, has_active, has_library_selection, has_selection, ok};
use crate::{LibrarySource, Result, Selection, Session};

fn select_after_deletion(s: &mut Session, before: &[PhotoId]) {
    let at = before.iter().position(|id| Some(*id) == s.selection.active);
    let remaining: std::collections::HashSet<_> = s.visible_cloned().into_iter().collect();
    s.selection.ids.retain(|id| remaining.contains(id));
    s.selection.active = s.selection.active.filter(|id| remaining.contains(id)).or_else(|| s.selection.ids.first().copied());
    if s.selection.active.is_none()
        && let Some(at) = at
    {
        // Keep culling near the deleted photo: next visible survivor, or the previous one at the end.
        let neighbour = before.iter().skip(at.saturating_add(1)).chain(before.iter().take(at).rev()).find(|id| remaining.contains(id));
        s.selection = neighbour.map(|id| Selection::single(*id)).unwrap_or_default();
    }
}

/// The auto-import folder's visible files with their sizes (the file-system half of
/// `library.autoImportScan`; the app lists on a worker thread and passes `listing`).
pub fn list_auto_import_folder(folder: &str) -> std::result::Result<Vec<(String, u64)>, String> {
    let rd = std::fs::read_dir(folder).map_err(|e| format!("{folder}: {e}"))?;
    Ok(rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let m = std::fs::metadata(e.path()).ok()?;
            m.is_file().then(|| (e.path().to_string_lossy().to_string(), m.len()))
        })
        .collect())
}

params! {
    /// `library.import`
    pub struct Import {
        #[serde(default)]
        paths: Vec<String>,
        mode: Option<String>,
        destination: Option<String>,
        organize: Option<String>,
        rename: Option<String>,
        rename_start: Option<usize>,
        metadata_preset: Option<String>,
        #[serde(default)]
        dng: bool,
        #[serde(default)]
        local: bool,
        album: Option<AlbumId>,
        album_name: Option<String>,
        preset: Option<String>,
        #[serde(default)]
        keywords: Vec<String>,
        on_deleted: Option<String>,
    }
}

/// `library.import`'s params, checked (no file-system calls).
pub struct ImportRequest {
    pub paths: Vec<String>,
    pub opts: crate::import::ImportOptions,
    pub album: Option<u64>,
    pub album_name: Option<String>,
}

/// Parse and check `library.import`'s params (the app's import task runs the import itself, on a
/// worker thread, with the same options).
pub fn import_params(s: &Session, p: &Value) -> Result<ImportRequest> {
    import_request(s, super::params::parse_as("library.import", p)?)
}

fn import_request(s: &Session, p: Import) -> Result<ImportRequest> {
    let mode = match p.mode.as_deref().unwrap_or("add") {
        "add" => crate::import::ImportMode::Add,
        "copy" => crate::import::ImportMode::Copy,
        "move" => crate::import::ImportMode::Move,
        other => return Err(bad("library.import", format!("unknown mode `{other}` (add|copy|move)"))),
    };
    let preset = match p.preset.as_deref().filter(|x| !x.is_empty()) {
        Some(id) => Some(s.presets.iter().find(|x| x.id == id).cloned().ok_or_else(|| bad("library.import", format!("unknown preset `{id}`")))?),
        None => None,
    };
    if let Some(a) = p.album
        && s.catalog.album(a).is_none_or(|al| al.folder || al.is_smart())
    {
        return Err(bad("library.import", "album must be a regular album"));
    }
    let organize = match p.organize.as_deref() {
        Some(o) => crate::import::Organize::parse(o).ok_or_else(|| {
            bad("library.import", format!("unknown organize `{o}` (date|month|flat, or a folder template like {{date:%Y}}/{{date:%Y%m%d}})"))
        })?,
        None => Default::default(),
    };
    if let crate::import::Organize::Template(t) = &organize
        && let Some(e) = crate::rename::folder_template_error(t)
    {
        return Err(bad("library.import", e));
    }
    let metadata_preset = p.metadata_preset.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    if let Some(n) = &metadata_preset
        && !s.prefs.metadata_presets.iter().any(|m| m.name.eq_ignore_ascii_case(n))
    {
        return Err(bad("library.import", format!("unknown metadata preset `{n}`")));
    }
    let on_deleted = match p.on_deleted.as_deref() {
        Some(v) => {
            crate::import::OnDeleted::parse(v).ok_or_else(|| bad("library.import", format!("unknown onDeleted `{v}` (restore|skip|fresh)")))?
        }
        None => Default::default(),
    };
    let opts = crate::import::ImportOptions {
        on_deleted,
        mode,
        preset,
        keywords: p.keywords,
        destination: p.destination,
        organize,
        rename: p.rename,
        rename_start: p.rename_start.unwrap_or(1),
        metadata_preset,
        convert_dng: p.dng,
        local: p.local,
    };
    let album_name = p.album_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    Ok(ImportRequest { paths: p.paths, opts, album: p.album.map(|a| a.0), album_name })
}

/// After a batch of an import was committed: the album (a new one named `album_name` when there
/// is no `album` and something was imported) gets the photos. Returns the report as JSON (with
/// `album`).
pub fn import_batch_done(s: &mut Session, report: crate::import::ImportReport, mut album: Option<u64>, album_name: Option<&str>) -> Result<Value> {
    let mut report = serde_json::to_value(report).unwrap_or_default();
    // photos an import restored from Recently Deleted take part like new ones (album, selection)
    let imported: Vec<u64> =
        ["imported", "restored"].iter().flat_map(|k| report[*k].as_array().into_iter().flatten().filter_map(Value::as_u64)).collect();
    if album.is_none()
        && let Some(name) = album_name
        && !imported.is_empty()
    {
        let r = s.execute("album.create", &json!({"name": name}))?;
        album = r["id"].as_u64();
    }
    if let Some(a) = album
        && !imported.is_empty()
    {
        // (album.addPhotos wants a selection: the new photos are about to be it)
        s.selection = Selection::single(PhotoId(imported[0]));
        s.execute("album.addPhotos", &json!({"id": a, "ids": imported}))?;
        report["album"] = json!(a);
    }
    Ok(report)
}

/// The targets an album can hold and how many were browsed (Local) photos, which album views
/// never show (issue #684): those are skipped, and reported, instead of becoming invisible members.
fn album_targets(s: &Session, targets: Vec<PhotoId>) -> (Vec<PhotoId>, usize) {
    let before = targets.len();
    let kept: Vec<PhotoId> = targets.into_iter().filter(|id| s.catalog.photo(*id).is_none_or(|p| !p.local)).collect();
    let skipped = before - kept.len();
    (kept, skipped)
}

/// Album targets, or the error for a selection that was nothing but Local photos.
fn album_targets_or_err(s: &Session, command: &str, targets: Vec<PhotoId>) -> Result<(Vec<PhotoId>, usize)> {
    let (kept, skipped) = album_targets(s, targets);
    if kept.is_empty() && skipped > 0 {
        return Err(bad(command, super::LOCAL_ONLY));
    }
    Ok((kept, skipped))
}

/// Move to Recently Deleted and drop the photos from stacks so leftover members do not keep
/// showing `i/n` (issue #1493).
fn mark_deleted(s: &mut Session, ids: &[PhotoId], label: &str) -> Result<usize> {
    let mut ops: Vec<Op> = ids.iter().map(|id| Op::SetDeleted { id: *id, deleted: true }).collect();
    let n = ops.len();
    if n == 0 {
        return Ok(0);
    }
    ops.extend(s.catalog.remove_from_stacks_ops(ids));
    s.commit(label, Op::Batch { ops })?;
    Ok(n)
}

/// Apply one op per target as a single undo step.
fn for_targets(s: &mut Session, targets: Vec<PhotoId>, label: &str, f: impl Fn(PhotoId) -> Option<Op>) -> Result<Value> {
    let ops: Vec<Op> = targets.iter().filter_map(|id| f(*id)).collect();
    let n = ops.len();
    if n > 0 {
        s.commit(label, Op::Batch { ops })?;
    }
    Ok(json!({"changed": n}))
}

/// Adjust each target's own rating; unchanged bounds do not create an undo step.
fn change_rating(s: &mut Session, p: TargetsAdvance, delta: i8, label: &str) -> Result<Value> {
    let mut ops = Vec::new();
    for id in p.targets(s) {
        let old = s.catalog.photo(id).ok_or(lightcraft_catalog::CatalogError::NoPhoto(id))?.rating;
        let rating = (i16::from(old) + i16::from(delta)).clamp(0, 5) as u8;
        if rating != old {
            ops.push(Op::SetRating { id, rating });
        }
    }
    let n = ops.len();
    if n > 0 {
        s.commit(label, Op::Batch { ops })?;
    }
    advance_if(s, p.advance);
    Ok(json!({"changed": n}))
}

/// Move each target's own flag by `step`: cycling wraps unflagged -> pick -> reject -> unflagged,
/// raising/lowering clamps on reject < unflagged < pick. Unchanged photos make no history step.
fn change_flag(s: &mut Session, p: TargetsAdvance, label: &str, step: impl Fn(Flag) -> Flag) -> Result<Value> {
    let mut ops = Vec::new();
    for id in p.targets(s) {
        let old = s.catalog.photo(id).ok_or(lightcraft_catalog::CatalogError::NoPhoto(id))?.flag;
        let flag = step(old);
        if flag != old {
            ops.push(Op::SetFlag { id, flag });
        }
    }
    let n = ops.len();
    if n > 0 {
        s.commit(label, Op::Batch { ops })?;
    }
    advance_if(s, p.advance);
    Ok(json!({"changed": n}))
}

fn advance_if(s: &mut Session, advance: bool) {
    if advance {
        let _ = step(s, 1);
    }
}

fn step(s: &mut Session, d: isize) -> Result<Value> {
    let vis = s.visible_cloned();
    if vis.is_empty() {
        return ok();
    }
    let cur = s.selection.active.and_then(|a| vis.iter().position(|x| *x == a));
    let next = match cur {
        Some(i) => (i as isize + d).clamp(0, vis.len() as isize - 1) as usize,
        None => 0,
    };
    s.end_interaction()?;
    s.selection = Selection::single(vis[next]);
    s.active_mask = None;
    s.active_spot = None;
    Ok(json!({"active": vis[next].0}))
}

params! {
    /// `library.backup`
    struct Backup {
        dir: Option<String>,
    }
}

params! {
    /// `library.up` / `library.down`
    struct Rows {
        rows: Option<u64>,
    }
}

/// `library.up` / `library.down`: how many photos to move (at least 1, capped so hostile input
/// can't overflow).
fn row_step(p: &Rows) -> isize {
    p.rows.unwrap_or(1).clamp(1, 1 << 20) as isize
}

params! {
    /// Photo commands that take only their targets.
    struct Targets with targets {}
}

params! {
    /// Rating and flag commands: their targets, and whether to move on to the next photo.
    struct TargetsAdvance with targets {
        #[serde(default)]
        advance: bool,
    }
}

fn rotate(s: &mut Session, p: Targets, cw: bool) -> Result<Value> {
    let targets = p.targets(s);
    let mut ops = Vec::new();
    for id in targets {
        if let Some(d) = s.develop_of(id) {
            let mut d = (*d).clone();
            d.orientation = d.orientation.rotated(cw);
            // the crop lives in the oriented frame, so it turns with the image (else it keeps the old width and height and cuts the photo)
            let turn = lightcraft_geom::Orientation::Normal.rotated(cw);
            d.crop.geometry.rect = turn.map_norm_rect(d.crop.geometry.rect);
            d.crop.aspect = d.crop.aspect.map(|(w, h)| (h, w));
            ops.extend(s.develop_op(id, d, "Rotate"));
        }
    }
    let n = ops.len();
    s.commit(if cw { "Rotate Right" } else { "Rotate Left" }, Op::Batch { ops })?;
    Ok(json!({"changed": n}))
}

fn flip(s: &mut Session, p: Targets, horizontal: bool) -> Result<Value> {
    let targets = p.targets(s);
    let mut ops = Vec::new();
    for id in targets {
        if let Some(d) = s.develop_of(id) {
            let mut d = (*d).clone();
            if horizontal {
                d.crop.flip_h = !d.crop.flip_h;
            } else {
                d.crop.flip_v = !d.crop.flip_v;
            }
            ops.extend(s.develop_op(id, d, "Flip"));
        }
    }
    let n = ops.len();
    s.commit("Flip", Op::Batch { ops })?;
    Ok(json!({"changed": n}))
}

params! {
    /// `library.selectBy`
    struct SelectBy {
        flag: Option<String>,
        rating: Option<u64>,
        rating_op: Option<String>,
        label: Option<String>,
        #[serde(default)]
        add: bool,
    }
}

/// `library.selectBy`: select the photos in view that match every given criterion.
fn select_by(s: &mut Session, p: SelectBy) -> Result<Value> {
    const C: &str = "library.selectBy";
    let flag = match p.flag.as_deref() {
        Some(f) => Some(Flag::parse(f).ok_or_else(|| bad(C, "flag is pick, reject or none"))?),
        None => None,
    };
    let rating = p.rating.map(|r| r.min(5) as u8);
    let op = p.rating_op.as_deref().unwrap_or("gte");
    let label: Option<Option<ColorLabel>> = match p.label.as_deref() {
        Some("none") => Some(None),
        Some(l) => Some(Some(ColorLabel::parse(l).ok_or_else(|| bad(C, format!("unknown label `{l}`")))?)),
        None => None,
    };
    if flag.is_none() && rating.is_none() && label.is_none() {
        return Err(bad(C, "give flag, rating and/or label"));
    }
    let matches = |ph: &lightcraft_catalog::Photo| {
        flag.is_none_or(|f| ph.flag == f)
            && rating.is_none_or(|r| match op {
                "eq" => ph.rating == r,
                "lte" => ph.rating <= r,
                _ => ph.rating >= r,
            })
            && label.is_none_or(|l| ph.label == l)
    };
    let mut ids: Vec<PhotoId> = if p.add { s.selection.ids.clone() } else { Vec::new() };
    for id in s.visible_cloned() {
        if !ids.contains(&id) && s.catalog.photo(id).is_some_and(|ph| matches(ph)) {
            ids.push(id);
        }
    }
    let active = s.selection.active.filter(|a| ids.contains(a)).or(ids.first().copied());
    let n = ids.len();
    s.selection = Selection { ids, active };
    Ok(json!({"selected": n}))
}

/// Seeds stay exactly representable as a JSON double, so web and agent clients read back what they wrote.
const MAX_SEED: u64 = 1 << 53;

/// `seed` in `0..2^53`, or `default` when absent or null.
fn seed_param(seed: Option<u64>, cmd: &str, default: u64) -> Result<u64> {
    match seed {
        None => Ok(default),
        Some(n) if n < MAX_SEED => Ok(n),
        Some(_) => Err(bad(cmd, "seed must be an integer from 0 to 2^53 - 1")),
    }
}

/// The seed after `prev` at time `now`: a mix of `prev` and the clock text, so repeated reshuffles
/// within one clock tick still differ (barring a 2^-53 collision). Kept below 2^53 so JSON
/// clients that read numbers as doubles (web, agents) get the same seed back.
fn next_seed(prev: u64, now: &str) -> u64 {
    let t = now.bytes().fold(0u64, |h, b| lightcraft_catalog::mix64(h ^ u64::from(b)));
    (lightcraft_catalog::mix64(prev.wrapping_add(1)) ^ t) & (MAX_SEED - 1)
}

params! {
    /// `library.source`
    struct Source {
        kind: Option<String>,
        id: Option<AlbumId>,
        path: Option<String>,
    }
}

params! {
    /// `library.removeFolder`
    struct RemoveFolder {
        path: Option<String>,
        #[serde(default)]
        disk: bool,
    }
}

params! {
    /// `folder.label`
    struct FolderLabel {
        path: Option<String>,
        label: Option<String>,
    }
}

params! {
    /// `library.sort`
    struct SortParams {
        key: Option<SortKey>,
        ascending: Option<bool>,
        group: Option<String>,
        seed: Option<u64>,
    }
}

params! {
    /// `library.holdView`
    struct HoldView {
        on: Option<bool>,
    }
}

params! {
    /// `library.shuffle`
    struct Shuffle {
        seed: Option<u64>,
    }
}

params! {
    /// `library.groups`
    struct Groups {
        by: Option<String>,
    }
}

params! {
    /// `library.select`
    struct Select {
        #[serde(default)]
        ids: Vec<PhotoId>,
        active: Option<PhotoId>,
        mode: Option<String>,
    }
}

params! {
    /// `photo.pasteMetadata`
    struct PasteMetadata {
        ids: Option<Vec<PhotoId>>,
        fields: Option<Vec<String>>,
    }
}

params! {
    /// `photo.rate`
    struct Rate with targets {
        #[serde(deserialize_with = "super::params::whole")]
        rating: i64,
        #[serde(default)]
        advance: bool,
    }
}

params! {
    /// `photo.flag`
    struct SetFlag with targets {
        flag: Option<String>,
        #[serde(default)]
        advance: bool,
    }
}

params! {
    /// `photo.label`
    struct SetLabel with targets {
        label: Option<String>,
    }
}

params! {
    /// `photo.removeRegion`
    struct RemoveRegion {
        id: Option<PhotoId>,
        index: usize,
    }
}

/// A face / pet region's box, normalized.
#[derive(Debug, serde::Deserialize)]
struct RegionRect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

params! {
    /// `photo.setRegion`
    struct SetRegion {
        id: Option<PhotoId>,
        index: usize,
        rect: RegionRect,
    }
}

params! {
    /// `photo.setMeta`
    struct SetMeta with targets {
        title: Option<String>,
        caption: Option<String>,
        alt_text: Option<String>,
        extended_description: Option<String>,
        copyright: Option<String>,
        copyright_status: Option<String>,
        usage_terms: Option<String>,
        copyright_url: Option<String>,
        creator: Option<String>,
        location: Option<String>,
        city: Option<String>,
        state: Option<String>,
        country: Option<String>,
        #[serde(default, deserialize_with = "super::params::present")]
        gps: Option<Option<Value>>,
        keywords: Option<Vec<String>>,
        #[serde(default)]
        add_keywords: Vec<String>,
        #[serde(default)]
        remove_keywords: Vec<String>,
    }
}

params! {
    /// `album.create`
    struct CreateAlbum {
        name: Option<String>,
        parent: Option<AlbumId>,
        #[serde(default)]
        folder: bool,
        #[serde(default)]
        add_selected: bool,
    }
}

params! {
    /// `album.createSmart`: `rules` is a partial `Filter`, checked when merged.
    struct CreateSmart {
        name: Option<String>,
        rules: Option<Value>,
        parent: Option<AlbumId>,
    }
}

params! {
    /// `album.setRules`
    struct SetRules {
        id: AlbumId,
        rules: Option<Value>,
        #[serde(default)]
        replace: bool,
        #[serde(default)]
        from_view: bool,
        name: Option<String>,
    }
}

params! {
    /// `album.rename`
    struct RenameAlbum {
        id: AlbumId,
        name: String,
    }
}

params! {
    /// `album.delete`
    struct AlbumRef {
        id: AlbumId,
    }
}

params! {
    /// `album.move`
    struct MoveAlbum {
        id: AlbumId,
        parent: Option<AlbumId>,
    }
}

params! {
    /// `album.reorder`: `parent` left out keeps the album's folder, `null` is the top level.
    struct ReorderAlbum {
        id: AlbumId,
        #[serde(default, deserialize_with = "super::params::present")]
        parent: Option<Option<AlbumId>>,
        before: Option<AlbumId>,
    }
}

params! {
    /// `album.sort`
    struct SortAlbums {
        parent: Option<AlbumId>,
    }
}

params! {
    /// `album.addPhotos`
    struct AddPhotos {
        id: AlbumId,
        ids: Option<Vec<PhotoId>>,
    }
}

params! {
    /// `album.toggleTarget`
    struct PhotoIds {
        ids: Option<Vec<PhotoId>>,
    }
}

params! {
    /// `album.setTarget`
    struct SetTarget {
        id: Option<AlbumId>,
    }
}

params! {
    /// `library.autoImport`: a key left out keeps its setting, `null` turns it off.
    struct AutoImport {
        #[serde(default, deserialize_with = "super::params::present")]
        folder: Option<Option<String>>,
        copy: Option<bool>,
        #[serde(default, deserialize_with = "super::params::present")]
        album: Option<Option<String>>,
    }
}

params! {
    /// `library.autoImportScan`
    struct AutoImportScan {
        listing: Option<Vec<(String, u64)>>,
        start: Option<bool>,
    }
}

params! {
    /// `album.removePhotos`
    struct RemovePhotos {
        id: Option<AlbumId>,
        ids: Option<Vec<PhotoId>>,
    }
}

params! {
    /// `album.setCover`
    struct SetCover {
        id: AlbumId,
        photo: Option<PhotoId>,
    }
}

params! {
    /// `library.inspectLightroom`. The desktop app reads and imports a catalog in the background: there `status`
    /// (or no `path`) reports the task, `cancel` stops it and `wait` waits for it. Here each call runs to the end
    /// before it returns, so nothing is ever in flight.
    struct LightroomPath {
        path: Option<String>,
        #[serde(default)]
        status: bool,
        #[serde(default)]
        wait: bool,
        #[serde(default)]
        cancel: bool,
    }
}

params! {
    /// `library.importLightroom` (`status`, `wait`, `cancel`: as for [`LightroomPath`])
    struct ImportLightroom {
        path: Option<String>,
        #[serde(default)]
        update_existing: bool,
        #[serde(default)]
        status: bool,
        #[serde(default)]
        wait: bool,
        #[serde(default)]
        cancel: bool,
    }
}

/// The path to read, or `None` for a status request (`status`, or no `path`): no task runs in the engine, so a
/// status has nothing to report and a cancel nothing to stop.
fn lightroom_target(path: Option<&str>, status: bool) -> Option<&std::path::Path> {
    path.filter(|_| !status).map(std::path::Path::new)
}

params! {
    /// `library.importPreview`
    struct Paths {
        #[serde(default)]
        paths: Vec<String>,
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // ---- view source / filter / sort
        cmd!(
            "library.source",
            "Show Source",
            [],
            None,
            "{kind: all|recentlyAdded|album|recentlyDeleted|picks|missing|libraryFolder, id?: albumId, path?: a path from library.folders (for libraryFolder)}",
            Source,
            always,
            |s, p| {
                let kind = p.kind.as_deref().unwrap_or("all");
                s.source = match kind {
                    "all" => LibrarySource::All,
                    "recentlyAdded" => LibrarySource::RecentlyAdded,
                    "recentlyDeleted" => LibrarySource::RecentlyDeleted,
                    "picks" => LibrarySource::Picks,
                    "missing" => LibrarySource::Missing,
                    "album" => {
                        let a = p.id.ok_or_else(|| bad("library.source", "missing album `id`"))?;
                        if s.catalog.album(a).is_none_or(|a| a.folder) {
                            return Err(bad("library.source", "no such album"));
                        }
                        LibrarySource::Album(a)
                    }
                    "libraryFolder" => {
                        let path = p.path.as_deref().filter(|d| !d.trim().is_empty() && !lightcraft_catalog::query::folder_key(d).is_empty());
                        let path = path.ok_or_else(|| bad("library.source", "libraryFolder needs the `path` of a folder from library.folders"))?;
                        if lightcraft_catalog::folders::is_startup_disk(path) {
                            return Err(bad("library.source", "the startup disk's path covers every disk: choose a folder in it"));
                        }
                        let f = Filter { library_folder: Some(path.to_string()), ..Default::default() };
                        let ids = s.catalog.query(&f, &Sort::default());
                        if ids.is_empty() {
                            return Err(bad("library.source", format!("{path}: no photo in the library was imported from it")));
                        }
                        if covers_other_disks(s, path, &ids) {
                            return Err(bad("library.source", format!("{path} holds other disks as well: choose a disk or a folder on one")));
                        }
                        s.library_folder = Some(path.to_string());
                        // one folder at a time: a folder filter left over would be ignored
                        s.filter.library_folder = None;
                        LibrarySource::LibraryFolder
                    }
                    other => return Err(bad("library.source", format!("unknown source `{other}`"))),
                };
                s.keep_active_in_view();
                Ok(json!({"count": s.visible().len()}))
            }
        ),
        cmd!(
            "library.filter",
            "Filter",
            [],
            None,
            "partial Filter: {text?, rating?, ratingOp?: atLeast|exactly|atMost, flag?: pick|reject|none|null, label?, kind?, merged?: hdr|panorama|hdrPanorama|any, edited?, date?, libraryFolder?: a path from library.folders, keyword?, person?, camera?}",
            Patch,
            always,
            |s, p| {
                let p = p.value();
                let mut v = serde_json::to_value(&s.filter).unwrap_or_default();
                lightcraft_develop::presets::deep_merge(&mut v, &p);
                let mut f: Filter = super::params::from_value(&v)?;
                // a blank folder is no folder: no hidden "filters active" state
                f.library_folder = f.library_folder.filter(|d| !d.trim().is_empty());
                if f.library_folder.is_some() && s.source == LibrarySource::LibraryFolder {
                    return Err(bad("library.filter", "a folder is already shown (library.source): show another source first"));
                }
                // checked like a smart album's rules when this call sets them: an unknown field or a value
                // that can't match is an error, not an empty grid. Rules that stopped checking since
                // (their album was deleted) don't block other filter changes.
                if let Some(rules) = f.rule_set.as_mut() {
                    rules.upgrade();
                }
                let sets_rules = p.get("ruleSet").is_some_and(|r| !r.is_null());
                if let Some(problems) = f.rule_set.as_ref().filter(|_| sets_rules).map(|rs| rs.check(&s.catalog)).filter(|p| !p.is_empty()) {
                    return Err(bad("library.filter", problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")));
                }
                s.filter = f;
                Ok(json!({"count": s.visible().len()}))
            }
        ),
        cmd!(
            query "library.folders",
            "Library Folders",
            [],
            None,
            "{} → [{name, path, count, own, volume, selectable, label?, children}] the disks and the folders the library's photos were imported from, with photo counts (subfolders included in `count`); show one with library.source {kind: libraryFolder, path} (rows with selectable false only open)",
            NoParams,
            always,
            |s, _| Ok(serde_json::to_value(s.catalog.folder_tree()).unwrap_or_default())
        ),
        cmd!(
            "library.removeFolder",
            "Remove Folder from Library",
            [],
            None,
            "{path, disk?: bool} — move the library photos imported from this folder (and the folders inside it) to Recently Deleted; one undo step, no file is touched. A whole disk or share (`/Volumes/nas`, `C:\\`, `\\\\srv\\share`) goes only with `disk: true`, the startup disk never → {removed}",
            RemoveFolder,
            always,
            |s, p| {
                const C: &str = "library.removeFolder";
                let path = p.path.as_deref().filter(|d| !d.trim().is_empty()).ok_or_else(|| bad(C, "missing `path`"))?;
                if lightcraft_catalog::query::folder_key(path).is_empty() {
                    return Err(bad(C, format!("{path}: not a folder")));
                }
                if lightcraft_catalog::folders::is_startup_disk(path) {
                    return Err(bad(C, "choose a folder, not the whole startup disk"));
                }
                if lightcraft_catalog::folders::is_disk_root(path) && !p.disk {
                    return Err(bad(C, format!("{path} is a whole disk or share: pass `disk: true` to remove everything on it")));
                }
                let f = Filter { library_folder: Some(path.to_string()), ..Default::default() };
                let ids = s.catalog.query(&f, &Sort::default());
                if ids.is_empty() {
                    return Err(bad(C, format!("{path}: no photo in the library was imported from it")));
                }
                if covers_other_disks(s, path, &ids) && !p.disk {
                    return Err(bad(C, format!("{path} holds whole disks: pass `disk: true` to remove everything on them")));
                }
                mark_deleted(s, &ids, "Remove Folder from Library")?;
                if s.filter.library_folder.as_deref().is_some_and(|c| lightcraft_catalog::query::folder_within(c, path)) {
                    s.filter.library_folder = None;
                }
                // the folder being shown is gone: back to everything, as for a deleted album
                if s.library_folder.as_deref().is_some_and(|c| lightcraft_catalog::query::folder_within(c, path)) {
                    s.library_folder = None;
                    if s.source == LibrarySource::LibraryFolder {
                        s.source = LibrarySource::All;
                    }
                }
                let vis = s.visible_cloned();
                s.selection = vis.first().map(|f| Selection::single(*f)).unwrap_or_default();
                Ok(json!({"removed": ids.len()}))
            }
        ),
        cmd!(
            "folder.label",
            "Set Folder Color Label",
            [],
            None,
            "{path, label: red|yellow|green|blue|purple|none} — give a folder of the library (one its photos were imported from, see library.folders) a colour label, or take it off (also once its photos are gone); one undo step. The label follows the folder when it is renamed or moved with folder.rename / folder.move → {path, label}",
            FolderLabel,
            always,
            |s, p| {
                const C: &str = "folder.label";
                let path = p.path.as_deref().filter(|d| !d.trim().is_empty()).ok_or_else(|| bad(C, "missing `path`"))?;
                let label = match p.label.as_deref().ok_or_else(|| bad(C, "missing `label` (red|yellow|green|blue|purple|none)"))? {
                    "none" => None,
                    x => Some(ColorLabel::parse(x).ok_or_else(|| bad(C, format!("unknown label {x:?}")))?),
                };
                // taking a label off always works, also once the folder's photos are gone
                let f = Filter { library_folder: Some(path.to_string()), ..Default::default() };
                if label.is_some() && s.catalog.query(&f, &Sort::default()).is_empty() {
                    return Err(bad(C, format!("{path}: no photo in the library was imported from it")));
                }
                if label.is_none() && s.catalog.folder_record(path).is_none() {
                    return Ok(json!({"path": path, "label": label}));
                }
                let op = s.catalog.folder_label_op(path, label);
                s.commit(if label.is_some() { "Set Folder Color Label" } else { "Remove Folder Color Label" }, op)?;
                Ok(json!({"path": path, "label": label}))
            }
        ),
        cmd!("library.clearFilter", "Clear Filters", ["View"], None, "{}", NoParams, always, |s, _| {
            s.filter = Default::default();
            Ok(json!({"count": s.visible().len()}))
        }),
        cmd!(
            "library.sort",
            "Sort",
            ["View", "Sort"],
            None,
            "{key?: captureDate|importDate|editDate|fileName|rating|fileSize|random, ascending?: bool, group?: auto|none|day|month|year, seed?: u64 (the shuffle `random` gives)}",
            SortParams,
            always,
            |s, p| {
                let key = p.key.unwrap_or(s.sort.key);
                let group = match p.group.as_deref() {
                    Some(g) => GroupBy::parse(g).ok_or_else(|| bad("library.sort", "group must be auto|none|day|month|year"))?,
                    None => s.sort.group,
                };
                let mut seed = seed_param(p.seed, "library.sort", s.sort.seed)?;
                let explicit = p.seed.is_some();
                if key == SortKey::Random && s.sort.key != SortKey::Random && !explicit {
                    // switching to Random is a fresh shuffle, not whatever seed was left behind
                    seed = next_seed(seed, &(s.clock)());
                }
                s.sort = Sort { key, ascending: p.ascending.unwrap_or(s.sort.ascending), group, seed };
                ok()
            }
        ),
        cmd!(
            "library.refresh",
            "Refresh View",
            ["View"],
            None,
            "{} — show what matches now: a view that holds its photos (`library.holdView`) lets go of the ones that stopped matching a smart album's rules, Picks or the filter bar since it was opened, sorted or last refreshed",
            NoParams,
            always,
            |s, _| {
                s.refresh_view();
                ok()
            }
        ),
        cmd!(
            "library.holdView",
            "Hold View Until Refreshed",
            ["View"],
            None,
            "{on?: bool (toggles when omitted)} → {hold} — when on, an edit that makes a photo stop matching the view (a keyword given in a smart album of photos without keywords, a flag taken off in Picks, a rating under the filter bar's) keeps it in the view until `library.refresh`, Reshuffle, or another source, filter or sort; photos that start matching still appear at once, and deleting a photo or taking it out of the album or folder shown still takes it out of the view. Changing a smart album's rules is another view too. Off (the default): the view follows every edit. Kept with the library's preferences; asking for the state it already has changes nothing",
            HoldView,
            always,
            |s, p| {
                let on = p.on.unwrap_or(!s.prefs.hold_view);
                if on != s.prefs.hold_view {
                    let was = s.prefs.hold_view;
                    s.prefs.hold_view = on;
                    if let Err(e) = s.save_prefs() {
                        // not kept: not changed either, so asking again does what was asked
                        s.prefs.hold_view = was;
                        return Err(e);
                    }
                    s.refresh_view();
                }
                Ok(json!({"hold": s.prefs.hold_view}))
            }
        ),
        cmd!(
            "library.shuffle",
            "Reshuffle",
            ["View", "Sort"],
            None,
            "{seed?: 0..2^53-1} — sort at random; without `seed` a new shuffle each time",
            Shuffle,
            always,
            |s, p| {
                let seed = match seed_param(p.seed, "library.shuffle", s.sort.seed)? {
                    given if p.seed.is_some() => given,
                    // derived from the previous seed and the session clock: reproducible under a test
                    // clock, and no RNG needed
                    prev => next_seed(prev, &(s.clock)()),
                };
                s.sort = Sort { key: SortKey::Random, seed, ..s.sort };
                ok()
            }
        ),
        cmd!(
            query "library.groups",
            "Date Groups",
            [],
            None,
            "{by?: day|month|year (default: the sort's grouping; auto = day)} → [{key, label, start, count}] date headers of the grid",
            Groups,
            always,
            |s, p| {
                let by = match p.by.as_deref() {
                    Some(g) => GroupBy::parse(g).ok_or_else(|| bad("library.groups", "by must be auto|none|day|month|year"))?,
                    None => s.sort.group,
                };
                let vis = s.visible_cloned();
                Ok(serde_json::to_value(s.catalog.date_runs(&vis, s.sort.key, by)).unwrap_or_default())
            }
        ),
        // ---- selection
        cmd!(
            "library.select",
            "Select Photos",
            [],
            None,
            "{ids: [id], active?: id, mode?: replace|add|toggle|range} — every id must be a photo of the library",
            Select,
            always,
            |s, p| {
                let ids = p.ids;
                let active = p.active;
                let mode = p.mode.as_deref().unwrap_or("replace");
                // An id that is not in the library is an error, and the selection stays as it was:
                // selecting it would otherwise report success and leave the session with no usable
                // active photo (#182).
                if let Some(id) = ids.iter().chain(active.as_ref()).find(|id| s.catalog.photo(**id).is_none()) {
                    return Err(bad("library.select", format!("no such photo {}", id.0)));
                }
                s.end_interaction()?;
                match mode {
                    "replace" => {
                        s.selection = Selection { ids: ids.clone(), active: active.or(ids.first().copied()) };
                    }
                    "add" => {
                        for id in ids {
                            if !s.selection.contains(id) {
                                s.selection.ids.push(id);
                            }
                            s.selection.active = Some(id);
                        }
                    }
                    "toggle" => ids.into_iter().for_each(|id| s.selection.toggle(id)),
                    "range" => {
                        let vis = s.visible_cloned();
                        if let Some(id) = ids.first() {
                            s.selection.extend_to(*id, &vis);
                        }
                    }
                    other => return Err(bad("library.select", format!("unknown mode `{other}`"))),
                }
                s.active_mask = None;
                s.active_spot = None;
                Ok(json!({"selected": s.selection.ids.len()}))
            }
        ),
        cmd!("library.selectAll", "Select All", ["Edit"], Some("Cmd+A"), "{}", NoParams, always, |s, _| {
            let vis = s.visible_cloned();
            s.selection = Selection { active: s.selection.active.filter(|a| vis.contains(a)).or(vis.first().copied()), ids: vis };
            Ok(json!({"selected": s.selection.ids.len()}))
        }),
        cmd!(query "library.devices", "Cameras and Cards", [], None, "{} → [{name, path (its DCIM folder), root}] — mounted volumes with a DCIM folder", NoParams, always, |_, _| {
            Ok(serde_json::to_value(crate::devices::devices_now()).unwrap_or_default())
        }),
        cmd!(query "photo.copyMetadata", "Copy Metadata", ["Photo"], None, "{} — title, caption, copyright (notice, status, usage terms, info URL), creator, location and keywords of the active photo", NoParams, has_active, |s, _| {
            let id = s.active().ok_or_else(|| bad("photo.copyMetadata", "no active photo"))?;
            let m = &s.catalog.photo(id).ok_or_else(|| bad("photo.copyMetadata", "no photo"))?.meta;
            let v = json!({"title": m.title, "caption": m.caption, "altText": m.alt_text, "extendedDescription": m.extended_description,
                "copyright": m.copyright, "copyrightStatus": m.copyright_status.id(), "usageTerms": m.usage_terms, "copyrightUrl": m.copyright_url,
                "creator": m.creator, "location": m.location, "city": m.city, "state": m.state, "country": m.country,
                "keywords": m.keywords});
            s.meta_clipboard = Some(v.clone());
            Ok(v)
        }),
        cmd!(
            "photo.pasteMetadata",
            "Paste Metadata",
            ["Photo"],
            None,
            "{ids?, fields?: [title|caption|copyright|copyrightStatus|usageTerms|copyrightUrl|creator|location|keywords] (default: all copied)}",
            PasteMetadata,
            has_selection,
            |s, p| {
                let c = "photo.pasteMetadata";
                let clip = s.meta_clipboard.clone().ok_or_else(|| bad(c, "nothing copied (photo.copyMetadata)"))?;
                let fields = p.fields;
                let mut params = serde_json::Map::new();
                for (k, v) in clip.as_object().into_iter().flatten() {
                    if fields.as_ref().is_none_or(|f| f.iter().any(|x| x == k)) {
                        params.insert(k.clone(), v.clone());
                    }
                }
                if let Some(ids) = p.ids {
                    params.insert("ids".into(), json!(ids));
                }
                s.execute("photo.setMeta", &Value::Object(params))
            }
        ),
        cmd!(
            "library.selectBy",
            "Select by Flag, Rating or Label",
            [],
            None,
            "{flag?: pick|reject|none, rating?: 0..5, ratingOp?: gte|eq|lte (default gte), label?: red|…|none, add?: bool (extend the selection)} — among the photos in view",
            SelectBy,
            always,
            select_by
        ),
        cmd!("library.selectNone", "Deselect All", ["Edit"], Some("Cmd+Shift+A"), "{}", NoParams, always, |s, _| {
            let a = s.selection.active;
            s.selection = Selection { ids: a.into_iter().collect(), active: a };
            ok()
        }),
        cmd!("library.next", "Next Photo", [], Some("Right"), "{}", NoParams, always, |s, _| step(s, 1)),
        cmd!("library.previous", "Previous Photo", [], Some("Left"), "{}", NoParams, always, |s, _| step(s, -1)),
        cmd!(
            "library.down",
            "Photo Below",
            [],
            Some("Down"),
            "{rows?: n (photos to move forward; default 1, the grid passes the distance to the cell below)}",
            Rows,
            always,
            |s, p| { step(s, row_step(&p)) }
        ),
        cmd!(
            "library.up",
            "Photo Above",
            [],
            Some("Up"),
            "{rows?: n (photos to move back; default 1, the grid passes the distance to the cell above)}",
            Rows,
            always,
            |s, p| { step(s, -row_step(&p)) }
        ),
        // ---- rating / flags / labels
        cmd!("photo.rate", "Set Rating", ["Photo", "Set Rating"], None, "{rating: 0..5, ids?, advance?: bool}", Rate, has_selection, |s, p| {
            let r = u8::try_from(p.rating).ok().filter(|r| *r <= 5).ok_or_else(|| bad("photo.rate", "rating must be 0..5"))?;
            let v = for_targets(s, p.targets(s), "Set Rating", |id| Some(Op::SetRating { id, rating: r }))?;
            advance_if(s, p.advance);
            Ok(v)
        }),
        cmd!("photo.decreaseRating", "Decrease Rating", [], None, "{ids?, advance?: bool}", TargetsAdvance, has_selection, |s, p| {
            change_rating(s, p, -1, "Decrease Rating")
        }),
        cmd!("photo.increaseRating", "Increase Rating", [], None, "{ids?, advance?: bool}", TargetsAdvance, has_selection, |s, p| {
            change_rating(s, p, 1, "Increase Rating")
        }),
        cmd!(
            "photo.flag",
            "Set Flag",
            ["Photo", "Set Flag"],
            None,
            "{flag: pick|reject|none, ids?, advance?: bool}",
            SetFlag,
            has_selection,
            |s, p| {
                let f = p.flag.as_deref().and_then(Flag::parse).ok_or_else(|| bad("photo.flag", "flag must be pick|reject|none"))?;
                let v = for_targets(s, p.targets(s), "Set Flag", |id| Some(Op::SetFlag { id, flag: f }))?;
                advance_if(s, p.advance);
                Ok(v)
            }
        ),
        // Classic: ` cycles the flag; Cmd+Up / Cmd+Down raise / lower it (Ctrl on Windows and Linux)
        cmd!(
            "photo.cycleFlag",
            "Cycle Flag",
            ["Photo", "Set Flag"],
            Some("`"),
            "{ids?, advance?: bool} — each photo's own flag: unflagged -> pick -> reject -> unflagged",
            TargetsAdvance,
            has_selection,
            |s, p| {
                change_flag(s, p, "Cycle Flag", |f| match f {
                    Flag::None => Flag::Pick,
                    Flag::Pick => Flag::Reject,
                    Flag::Reject => Flag::None,
                })
            }
        ),
        cmd!(
            "photo.increaseFlag",
            "Increase Flag Status",
            ["Photo", "Set Flag"],
            Some("Cmd+Up"),
            "{ids?, advance?: bool} — reject -> unflagged -> pick (stops at pick)",
            TargetsAdvance,
            has_selection,
            |s, p| { change_flag(s, p, "Increase Flag Status", |f| if f == Flag::Reject { Flag::None } else { Flag::Pick }) }
        ),
        cmd!(
            "photo.decreaseFlag",
            "Decrease Flag Status",
            ["Photo", "Set Flag"],
            Some("Cmd+Down"),
            "{ids?, advance?: bool} — pick -> unflagged -> reject (stops at reject)",
            TargetsAdvance,
            has_selection,
            |s, p| { change_flag(s, p, "Decrease Flag Status", |f| if f == Flag::Pick { Flag::None } else { Flag::Reject }) }
        ),
        cmd!("photo.pick", "Flag as Pick", ["Photo", "Set Flag"], Some("P"), "{ids?}", Targets, has_selection, |s, p| {
            for_targets(s, p.targets(s), "Flag as Pick", |id| Some(Op::SetFlag { id, flag: Flag::Pick }))
        }),
        cmd!("photo.reject", "Flag as Reject", ["Photo", "Set Flag"], Some("X"), "{ids?}", Targets, has_selection, |s, p| {
            for_targets(s, p.targets(s), "Flag as Reject", |id| Some(Op::SetFlag { id, flag: Flag::Reject }))
        }),
        cmd!("photo.unflag", "Unflag", ["Photo", "Set Flag"], Some("U"), "{ids?}", Targets, has_selection, |s, p| {
            for_targets(s, p.targets(s), "Unflag", |id| Some(Op::SetFlag { id, flag: Flag::None }))
        }),
        cmd!(
            "photo.label",
            "Set Color Label",
            ["Photo", "Set Color Label"],
            None,
            "{label: red|yellow|green|blue|purple|none, ids?}",
            SetLabel,
            has_selection,
            |s, p| {
                let l = match p.label.as_deref() {
                    None | Some("none") => None,
                    Some(x) => Some(ColorLabel::parse(x).ok_or_else(|| bad("photo.label", "unknown label"))?),
                };
                for_targets(s, p.targets(s), "Set Color Label", |id| Some(Op::SetLabel { id, label: l }))
            }
        ),
        // ---- orientation
        cmd!("photo.rotateLeft", "Rotate Left", ["Photo"], Some("Cmd+["), "{ids?}", Targets, has_selection, |s, p| rotate(s, p, false)),
        cmd!("photo.rotateRight", "Rotate Right", ["Photo"], Some("Cmd+]"), "{ids?}", Targets, has_selection, |s, p| rotate(s, p, true)),
        cmd!("photo.flipHorizontal", "Flip Horizontal", ["Photo"], None, "{ids?}", Targets, has_selection, |s, p| flip(s, p, true)),
        cmd!("photo.flipVertical", "Flip Vertical", ["Photo"], None, "{ids?}", Targets, has_selection, |s, p| flip(s, p, false)),
        // ---- face / pet regions
        cmd!(
            "photo.removeRegion",
            "Remove Face Box",
            [],
            None,
            "{id?, index} — remove one face / pet region (by its position in the photo's regions) from the photo in the catalog; undoable. The XMP sidecar is never rewritten for this, even with auto-write on, so reading the metadata from the file brings the region back",
            RemoveRegion,
            always,
            |s, p| {
                let id = p.id.or_else(|| s.active()).ok_or_else(|| bad("photo.removeRegion", "no photo"))?;
                let index = p.index;
                let mut meta = s.catalog.photo(id).ok_or_else(|| bad("photo.removeRegion", "no such photo"))?.meta.clone();
                if index >= meta.regions.len() {
                    return Err(bad("photo.removeRegion", "no such region"));
                }
                let gone = meta.regions.remove(index);
                s.commit("Remove Face Box", Op::SetMeta { id, meta: Box::new(meta) })?;
                s.skip_auto_write = true;
                Ok(json!({"removed": gone.name}))
            }
        ),
        cmd!(
            "photo.setRegion",
            "Resize Face Box",
            [],
            None,
            "{id?, index, rect: {x0, y0, x1, y1}} — set one face / pet region's box (normalized, in the photo's upright frame; clamped to the photo, at least 0.5 % each way) in the catalog; undoable. Like photo.removeRegion it never rewrites the XMP sidecar",
            SetRegion,
            always,
            |s, p| {
                const C: &str = "photo.setRegion";
                // a drag is one undo step however many frames it took: the caller commits once, on release
                let id = p.id.or_else(|| s.active()).ok_or_else(|| bad(C, "no photo"))?;
                let index = p.index;
                let RegionRect { x0: a, y0: b, x1: c, y1: d } = p.rect;
                let (x0, x1) = (a.min(c).clamp(0.0, 1.0), a.max(c).clamp(0.0, 1.0));
                let (y0, y1) = (b.min(d).clamp(0.0, 1.0), b.max(d).clamp(0.0, 1.0));
                if x1 - x0 < 0.005 || y1 - y0 < 0.005 {
                    return Err(bad(C, "the box would be too small"));
                }
                let mut meta = s.catalog.photo(id).ok_or_else(|| bad(C, "no such photo"))?.meta.clone();
                let region = meta.regions.get_mut(index).ok_or_else(|| bad(C, "no such region"))?;
                region.rect = lightcraft_geom::Rect { x0, y0, x1, y1 };
                s.commit("Resize Face Box", Op::SetMeta { id, meta: Box::new(meta) })?;
                s.skip_auto_write = true;
                Ok(json!({"rect": {"x0": x0, "y0": y0, "x1": x1, "y1": y1}}))
            }
        ),
        // ---- delete / restore
        cmd!("photo.delete", "Delete Photo", ["Photo"], Some("Delete"), "{ids?} — moves to Recently Deleted", Targets, has_selection, |s, p| {
            let before = s.visible_cloned();
            let targets = p.targets(s);
            let n = mark_deleted(s, &targets, "Delete")?;
            select_after_deletion(s, &before);
            Ok(json!({"changed": n}))
        }),
        cmd!("photo.restore", "Restore", ["Photo"], None, "{ids?}", Targets, has_selection, |s, p| for_targets(s, p.targets(s), "Restore", |id| {
            Some(Op::SetDeleted { id, deleted: false })
        })),
        cmd!(
            "library.emptyRecentlyDeleted",
            "Empty Recently Deleted",
            ["Photo"],
            None,
            "{} → {deleted} — removes every photo in Recently Deleted from the library (files on disk stay)",
            NoParams,
            always,
            |s, _| {
                let ids: Vec<PhotoId> = s.catalog.photos().filter(|p| p.deleted).map(|p| p.id).collect();
                if ids.is_empty() {
                    return Ok(json!({"deleted": 0}));
                }
                let op = s.catalog.delete_photos_permanently_ops(&ids);
                s.commit("Empty Recently Deleted", op)?;
                s.selection = Selection::default();
                Ok(json!({"deleted": ids.len()}))
            }
        ),
        cmd!("photo.deletePermanently", "Delete Permanently", ["Photo"], None, "{ids?}", Targets, has_selection, |s, p| {
            let before = s.visible_cloned();
            let t = p.targets(s);
            let op = s.catalog.delete_photos_permanently_ops(&t);
            s.commit("Delete Permanently", op)?;
            select_after_deletion(s, &before);
            Ok(json!({"deleted": t.len()}))
        }),
        // ---- metadata
        cmd!(
            "photo.setMeta",
            "Edit Info",
            [],
            None,
            "{ids?, title?, caption?, altText?, extendedDescription?, copyright?, copyrightStatus?: unknown|copyrighted|publicDomain, usageTerms?, copyrightUrl?, creator?, location?, city?, state?, country?, gps?: \"lat, lon\" | [lat, lon] | null, keywords?: [..], addKeywords?: [..], removeKeywords?: [..]}",
            SetMeta,
            has_selection,
            |s, p| {
                let targets = p.targets(s);
                // GPS: "lat, lon" (decimal or 51°30'26"N 0°7'39"W), [lat, lon], or null / "" to clear
                let gps: Option<Option<(f64, f64)>> = match &p.gps {
                    None => None,
                    Some(None) => Some(None),
                    Some(Some(Value::String(t))) if t.trim().is_empty() => Some(None),
                    Some(Some(Value::String(t))) => Some(Some(
                        parse_gps(t).ok_or_else(|| bad("photo.setMeta", format!("can't read `{t}` as coordinates (e.g. 51.5072, -0.1276)")))?,
                    )),
                    Some(Some(Value::Array(a))) if a.len() == 2 => match (a[0].as_f64(), a[1].as_f64()) {
                        (Some(la), Some(lo)) if la.abs() <= 90.0 && lo.abs() <= 180.0 => Some(Some((la, lo))),
                        _ => return Err(bad("photo.setMeta", "gps is [latitude, longitude] in degrees")),
                    },
                    Some(_) => return Err(bad("photo.setMeta", "gps is \"lat, lon\", [lat, lon] or null")),
                };
                let status = match p.copyright_status.as_deref() {
                    Some(t) => Some(
                        CopyrightStatus::parse(t)
                            .ok_or_else(|| bad("photo.setMeta", format!("copyrightStatus `{t}`: unknown, copyrighted or publicDomain")))?,
                    ),
                    None => None,
                };
                let mut ops = Vec::new();
                for id in targets {
                    let Some(ph) = s.catalog.photo(id) else { continue };
                    let mut m = ph.meta.clone();
                    if let Some(g) = gps {
                        m.gps = g;
                    }
                    if let Some(st) = status {
                        m.copyright_status = st;
                    }
                    for (v, field) in [
                        (&p.title, &mut m.title),
                        (&p.caption, &mut m.caption),
                        (&p.copyright, &mut m.copyright),
                        (&p.usage_terms, &mut m.usage_terms),
                        (&p.copyright_url, &mut m.copyright_url),
                        (&p.creator, &mut m.creator),
                        (&p.location, &mut m.location),
                        (&p.city, &mut m.city),
                        (&p.state, &mut m.state),
                        (&p.country, &mut m.country),
                        (&p.alt_text, &mut m.alt_text),
                        (&p.extended_description, &mut m.extended_description),
                    ] {
                        if let Some(v) = v {
                            v.clone_into(field);
                        }
                    }
                    // keywords are stored cleaned (`a | b ` is `a|b`), each once whatever its case
                    use lightcraft_catalog::keywords::{clean, same};
                    let add = |m: &mut lightcraft_catalog::Meta, k: &str| {
                        let k = clean(k);
                        if !k.is_empty() && !m.keywords.iter().any(|x| same(x, &k)) {
                            m.keywords.push(k);
                        }
                    };
                    if let Some(k) = &p.keywords {
                        m.keywords.clear();
                        for k in k {
                            add(&mut m, k);
                        }
                    }
                    for k in &p.add_keywords {
                        add(&mut m, k);
                    }
                    for k in &p.remove_keywords {
                        let k = clean(k);
                        m.keywords.retain(|x| !same(&clean(x), &k));
                    }
                    ops.push(Op::SetMeta { id, meta: Box::new(m) });
                }
                let n = ops.len();
                s.commit("Edit Info", Op::Batch { ops })?;
                // keywords just added become the Recent Keywords set
                let added: Vec<String> = p.add_keywords.iter().chain(p.keywords.iter().flatten()).cloned().collect();
                if n > 0 && !added.is_empty() {
                    crate::cmd::keywords::note_recent(s, &added);
                }
                Ok(json!({"changed": n}))
            }
        ),
        // ---- albums
        cmd!(
            "album.create",
            "New Album",
            ["File"],
            None,
            "{name, parent?: folderId, folder?: bool, addSelected?: bool} → {id, skipped} (skipped: Local photos of the selection, which albums can't hold)",
            CreateAlbum,
            always,
            |s, p| {
                let name = p.name.as_deref().unwrap_or("Untitled Album").trim().to_string();
                if name.is_empty() {
                    return Err(bad("album.create", "empty name"));
                }
                let folder = p.folder;
                let parent = p.parent;
                let (photos, skipped) = if !folder && p.add_selected { album_targets(s, s.selected()) } else { (vec![], 0) };
                let id = s.catalog.alloc_album_id();
                let cover = photos.first().copied();
                s.commit(
                    if folder { "New Folder" } else { "New Album" },
                    Op::AddAlbum { album: Album { id, name, parent, folder, photos, cover, smart: None, quick: false, order: None } },
                )?;
                Ok(json!({"id": id.0, "skipped": skipped}))
            }
        ),
        cmd!(
            "album.createSmart",
            "New Smart Album…",
            [],
            None,
            "{name, rules?: partial Filter (rating, ratingOp, flag, label, kind, edited, keyword, camera, lens, dateFrom, dateTo, date, text, album, ruleSet: {match: all|any|none, rules: [{field, op, value} | {group: ruleSet}]} — see album.ruleFields), parent?: folderId} — without `rules`, saves the current view (source + filter)",
            CreateSmart,
            always,
            |s, p| {
                let name = p.name.as_deref().unwrap_or("Smart Album").trim().to_string();
                if name.is_empty() {
                    return Err(bad("album.createSmart", "empty name"));
                }
                let rules = match &p.rules {
                    Some(r) => merge_rules(&lightcraft_catalog::Filter::default(), r, "album.createSmart", &s.catalog, None)?,
                    None => view_rules(s),
                };
                let parent = p.parent;
                let id = s.catalog.alloc_album_id();
                let album = Album { parent, smart: Some(Box::new(rules)), ..Album::new(id, name) };
                s.commit("New Smart Album", Op::AddAlbum { album })?;
                Ok(json!({"id": id.0, "count": s.catalog.album_count(id)}))
            }
        ),
        cmd!(query "album.ruleFields", "Smart Album Rule Fields", [], None, "{} → [{field, label, kind (text|keywords|number|date|choice|bool|album: an album id), group (field-menu submenu, null at the top level), ops: [{op, label}], choices? (ids), choiceLabels? ({id: label})}] for ruleSet rules, in menu order", NoParams, always, |_, _| {
            use lightcraft_catalog::rules::{FIELDS, Kind, field_group, ops_for};
            Ok(json!(FIELDS
                .iter()
                .map(|(id, label, kind)| {
                    let k = match kind {
                        Kind::Text => "text",
                        Kind::Keywords => "keywords",
                        Kind::Number => "number",
                        Kind::Album => "album",
                        Kind::Date => "date",
                        Kind::Choice(_) => "choice",
                        Kind::Bool => "bool",
                    };
                    let mut v = json!({"field": id, "label": label, "kind": k, "group": field_group(id), "ops": ops_for(*kind).iter().map(|(o, l)| json!({"op": o, "label": l})).collect::<Vec<_>>()});
                    if let Kind::Choice(c) = kind {
                        v["choices"] = json!(c.iter().map(|c| c.0).collect::<Vec<_>>());
                        v["choiceLabels"] = c.iter().map(|(id, label)| (id.to_string(), json!(label))).collect::<serde_json::Map<_, _>>().into();
                    }
                    v
                })
                .collect::<Vec<_>>()))
        }),
        cmd!(
            "album.setRules",
            "Edit Smart Album",
            [],
            None,
            "{id, rules?: partial Filter merged onto the current rules (null clears a field), replace?: bool, fromView?: bool (use the current view), name?: also rename it (blank keeps the name) — one undo step, both or neither}",
            SetRules,
            always,
            |s, p| {
                let id = p.id;
                let cur = s.catalog.album(id).and_then(|a| a.smart.as_deref().cloned()).ok_or_else(|| bad("album.setRules", "not a smart album"))?;
                let rules = if p.from_view {
                    // the view is saved as it is, but never as a loop (showing A, which tests this
                    // album, and saving that view here would make it test itself)
                    let view = view_rules(s);
                    let loops: Vec<String> = view
                        .rule_set
                        .as_ref()
                        .map(|r| r.check_for(&s.catalog, Some(id)))
                        .unwrap_or_default()
                        .into_iter()
                        .chain(s.catalog.album_filter_problem(&view, Some(id)))
                        .filter(|p| p.issue == lightcraft_catalog::rules::Issue::AlbumLoop)
                        .map(|p| p.to_string())
                        .collect();
                    if !loops.is_empty() {
                        return Err(bad("album.setRules", loops.join("; ")));
                    }
                    view
                } else {
                    let replace = p.replace;
                    let folder = cur.library_folder.clone();
                    let base = if replace { Default::default() } else { cur };
                    let mut r = merge_rules(&base, p.rules.as_ref().unwrap_or(&Value::Null), "album.setRules", &s.catalog, Some(id))?;
                    // the rules dialog has no folder field: replacing its rules keeps the folder
                    // unless the call says (`libraryFolder: null`) to drop it
                    if replace && p.rules.as_ref().and_then(|r| r.get("libraryFolder")).is_none() {
                        r.library_folder = folder;
                    }
                    r
                };
                let set = Op::SetAlbumRules { id, rules: Box::new(rules) };
                // a new name goes with the rules: one step, applied (and undone) together
                let rename = p.name.as_deref().map(str::trim).filter(|n| !n.is_empty() && s.catalog.album(id).is_some_and(|a| a.name != *n));
                match rename {
                    Some(name) => s.commit("Edit Smart Album", Op::Batch { ops: vec![Op::RenameAlbum { id, name: name.to_string() }, set] })?,
                    None => s.commit("Edit Smart Album", set)?,
                };
                Ok(json!({"count": s.catalog.album_count(id)}))
            }
        ),
        cmd!("album.rename", "Rename Album", [], None, "{id, name}", RenameAlbum, always, |s, p| {
            s.commit("Rename Album", Op::RenameAlbum { id: p.id, name: p.name })?;
            ok()
        }),
        cmd!(
            "album.delete",
            "Delete Album",
            [],
            None,
            "{id} — deletes the album or folder and its descendants, keeping photos in the library; one undo step",
            AlbumRef,
            always,
            |s, p| {
                let id = p.id;
                let folder = s.catalog.album(id).ok_or_else(|| bad("album.delete", "no such album"))?.folder;
                let mut children = std::collections::BTreeMap::<AlbumId, Vec<AlbumId>>::new();
                for album in s.catalog.albums() {
                    if let Some(parent) = album.parent {
                        children.entry(parent).or_default().push(album.id);
                    }
                }
                // Iterative traversal: even a malformed cycle is rejected before any change.
                // Reverse parent-first order removes children first; the batch inverse restores
                // parents first, including memberships, smart rules, covers and manual ordering.
                let mut pending = vec![id];
                let mut order = Vec::new();
                let mut removed = std::collections::BTreeSet::new();
                while let Some(next) = pending.pop() {
                    if !removed.insert(next) {
                        return Err(bad("album.delete", "cycle in album folders"));
                    }
                    order.push(next);
                    if let Some(nested) = children.remove(&next) {
                        pending.extend(nested);
                    }
                }
                let ops = order.into_iter().rev().map(|id| Op::RemoveAlbum { id }).collect();
                s.commit(if folder { "Delete Folder" } else { "Delete Album" }, Op::Batch { ops })?;
                if matches!(s.source, LibrarySource::Album(current) if removed.contains(&current)) {
                    s.source = LibrarySource::All;
                }
                ok()
            }
        ),
        cmd!("album.move", "Move Album", [], None, "{id, parent?: folderId|null}", MoveAlbum, always, |s, p| {
            s.commit("Move Album", Op::MoveAlbum { id: p.id, parent: p.parent })?;
            ok()
        }),
        cmd!(
            "album.reorder",
            "Reorder Album",
            [],
            None,
            "{id, parent?: folderId|null, before?: albumId|null} — places the album (or folder) in `parent` (default: where it is; null: the top level) before the sibling `before` of the same kind, or last of its kind; the folder is then ordered by hand. One undo step. → {changed}: how many edits it took (0: already there)",
            ReorderAlbum,
            always,
            |s, p| {
                let id = p.id;
                let me = s.catalog.album(id).ok_or_else(|| bad("album.reorder", "no such album"))?;
                let parent = p.parent.unwrap_or(me.parent);
                let before = p.before;
                if before == Some(id) {
                    // before itself: where it is
                    return Ok(json!({"changed": 0}));
                }
                let mut order: Vec<AlbumId> = s.catalog.album_children(parent).into_iter().filter(|a| a.id != id).map(|a| a.id).collect();
                let at = match before {
                    Some(b) => {
                        let at = order.iter().position(|x| *x == b).ok_or_else(|| bad("album.reorder", "`before` is not inside that folder"))?;
                        if s.catalog.album(b).is_none_or(|x| x.folder != me.folder) {
                            return Err(bad("album.reorder", "folders are placed among folders and albums among albums"));
                        }
                        at
                    }
                    None if me.folder => order.iter().take_while(|x| s.catalog.album(**x).is_some_and(|a| a.folder)).count(),
                    None => order.len(),
                };
                order.insert(at.min(order.len()), id);
                let moved = me.parent != parent;
                let mut ops = Vec::new();
                if moved {
                    ops.push(Op::MoveAlbum { id, parent });
                }
                for (i, aid) in order.iter().enumerate() {
                    let want = u32::try_from(i).ok();
                    // a moved album starts without a place in its new folder
                    let have = if moved && *aid == id { None } else { s.catalog.album(*aid).and_then(|a| a.order) };
                    if have != want {
                        ops.push(Op::SetAlbumOrder { id: *aid, order: want });
                    }
                }
                if ops.is_empty() {
                    return Ok(json!({"changed": 0}));
                }
                let changed = ops.len();
                s.commit("Reorder Album", Op::Batch { ops })?;
                Ok(json!({"changed": changed}))
            }
        ),
        cmd!(
            "album.sort",
            "Sort Albums by Name",
            [],
            None,
            "{parent?: folderId|null} — drops the hand order of the albums inside the folder (default: the top level), so they are listed by name → {changed}: how many albums lost their place",
            SortAlbums,
            always,
            |s, p| {
                let parent = p.parent;
                let ops: Vec<Op> = s
                    .catalog
                    .album_children(parent)
                    .into_iter()
                    .filter(|a| a.order.is_some())
                    .map(|a| Op::SetAlbumOrder { id: a.id, order: None })
                    .collect();
                if ops.is_empty() {
                    return Ok(json!({"changed": 0}));
                }
                let changed = ops.len();
                s.commit("Sort Albums by Name", Op::Batch { ops })?;
                Ok(json!({"changed": changed}))
            }
        ),
        cmd!(
            "album.addPhotos",
            "Add to Album",
            ["Photo"],
            None,
            "{id: albumId, ids?: [photoIds]} (default: selection) → {added, skipped}; Local photos are skipped (albums hold library photos), only Local ones is an error",
            AddPhotos,
            has_library_selection,
            |s, p| {
                let id = p.id;
                let targets = p.ids.unwrap_or_else(|| s.selected());
                let al = s.catalog.album(id).ok_or_else(|| bad("album.addPhotos", "no such album"))?;
                if al.is_smart() || al.folder {
                    return Err(bad("album.addPhotos", "smart albums and folders can't hold photos"));
                }
                let (targets, skipped) = album_targets_or_err(s, "album.addPhotos", targets)?;
                let mut photos = al.photos.clone();
                let before = photos.len();
                // looked up, not searched for: adding thousands to a big album stays linear
                let mut held: std::collections::HashSet<PhotoId> = photos.iter().copied().collect();
                for t in targets {
                    if held.insert(t) {
                        photos.push(t);
                    }
                }
                let added = photos.len() - before;
                let cover = al.cover.or(photos.first().copied());
                s.commit("Add to Album", Op::Batch { ops: vec![Op::SetAlbumPhotos { id, photos }, Op::SetAlbumCover { id, cover }] })?;
                Ok(json!({"added": added, "skipped": skipped}))
            }
        ),
        cmd!(
            "album.toggleTarget",
            "Add to Target Album",
            [],
            None,
            "{ids?} — B: adds the photos to the target album (the Quick Collection unless one is set), or removes them when they're all in it → {album, added, skipped}; Local photos are skipped",
            PhotoIds,
            has_library_selection,
            |s, p| {
                let targets = album_targets_or_err(s, "album.toggleTarget", p.ids.unwrap_or_else(|| s.selected()))?;
                let (targets, skipped) = targets;
                let target = s.target_album.filter(|a| s.catalog.album(*a).is_some_and(|al| !al.is_smart() && !al.folder));
                let id = match target.or_else(|| s.catalog.quick_collection()) {
                    Some(id) => id,
                    None => {
                        let id = s.catalog.alloc_album_id();
                        let album = Album { quick: true, ..Album::new(id, "Quick Collection") };
                        s.commit("New Quick Collection", Op::AddAlbum { album })?;
                        id
                    }
                };
                let al = s.catalog.album(id).ok_or_else(|| bad("album.toggleTarget", "no target album"))?;
                // looked up, not searched for: thousands toggled in a big album stay linear
                let mut held: std::collections::HashSet<PhotoId> = al.photos.iter().copied().collect();
                let all_in = !targets.is_empty() && targets.iter().all(|t| held.contains(t));
                let photos: Vec<PhotoId> = if all_in {
                    let targets: std::collections::HashSet<PhotoId> = targets.iter().copied().collect();
                    al.photos.iter().copied().filter(|x| !targets.contains(x)).collect()
                } else {
                    al.photos.iter().copied().chain(targets.iter().copied().filter(|t| held.insert(*t))).collect()
                };
                let name = al.name.clone();
                let cover = al.cover.filter(|c| photos.contains(c)).or(photos.first().copied());
                s.commit(
                    if all_in { "Remove from Target Album" } else { "Add to Target Album" },
                    Op::Batch { ops: vec![Op::SetAlbumPhotos { id, photos }, Op::SetAlbumCover { id, cover }] },
                )?;
                Ok(json!({"album": id.0, "name": name, "added": !all_in, "count": s.catalog.album_count(id), "skipped": skipped}))
            }
        ),
        cmd!("album.setTarget", "Set as Target Album", [], None, "{id: albumId | null} (null = the Quick Collection)", SetTarget, always, |s, p| {
            s.target_album = match p.id {
                Some(id) => {
                    let al = s.catalog.album(id).ok_or_else(|| bad("album.setTarget", "no such album"))?;
                    if al.is_smart() || al.folder {
                        return Err(bad("album.setTarget", "smart albums and folders can't hold photos"));
                    }
                    Some(id)
                }
                None => None,
            };
            Ok(json!({"target": s.target_album.map(|a| a.0)}))
        }),
        cmd!("album.clearQuick", "Clear Quick Collection", [], None, "{}", NoParams, always, |s, _| {
            let Some(id) = s.catalog.quick_collection() else { return ok() };
            s.commit(
                "Clear Quick Collection",
                Op::Batch { ops: vec![Op::SetAlbumPhotos { id, photos: vec![] }, Op::SetAlbumCover { id, cover: None }] },
            )?;
            ok()
        }),
        cmd!(
            "library.autoImport",
            "Auto Import Settings",
            [],
            None,
            "{folder?: path | null (off), copy?: bool (copy into the library's Originals, else add in place), album?: name | null} — a watched folder whose new photos are added as they arrive (library.autoImportScan; the app scans every few seconds) → the settings",
            AutoImport,
            always,
            |s, p| {
                if let Some(f) = p.folder {
                    s.prefs.import_defaults.auto_folder = match f.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
                        Some(f) if std::path::Path::new(f).is_dir() || cfg!(target_arch = "wasm32") => Some(f.to_string()),
                        Some(f) => return Err(bad("library.autoImport", format!("`{f}` is not a folder"))),
                        None => None,
                    };
                }
                if let Some(c) = p.copy {
                    s.prefs.import_defaults.auto_copy = c;
                }
                if let Some(a) = p.album {
                    s.prefs.import_defaults.auto_album = a.as_deref().map(str::trim).filter(|a| !a.is_empty()).map(str::to_string);
                }
                s.save_prefs()?;
                let d = &s.prefs.import_defaults;
                Ok(json!({"folder": d.auto_folder, "copy": d.auto_copy, "album": d.auto_album}))
            }
        ),
        cmd!(
            "library.autoImportScan",
            "Auto Import Now",
            [],
            None,
            "{listing?: [[path, size]] (the folder as listed by the caller, e.g. on a worker thread), start?: bool (false: return the `library.import` params as `import` instead of importing)} — add the watched folder's new photos (files the library doesn't have yet; partial / still-copying files wait for the next scan) → {imported, folder, warnings?: [{path, code, profile?, mode?, fields?}], import?}",
            AutoImportScan,
            always,
            |s, p| {
                let Some(folder) = s.prefs.import_defaults.auto_folder.clone() else { return Ok(json!({"imported": [], "folder": null})) };
                // only files that stopped growing: a file still being written is left for later
                let known: std::collections::HashSet<String> = s
                    .catalog
                    .photos()
                    .filter_map(|p| match &p.source {
                        lightcraft_catalog::Source::File { path } => Some(path.clone()),
                        _ => None,
                    })
                    .collect();
                // the folder's files and sizes: listed here, or already listed on a worker thread (the app)
                let listing: Vec<(String, u64)> = match p.listing {
                    Some(l) => l,
                    None => list_auto_import_folder(&folder).map_err(|e| bad("library.autoImportScan", e))?,
                };
                let mut fresh = Vec::new();
                for (ps, size) in listing {
                    if known.contains(&ps) {
                        continue;
                    }
                    // each file is tried once (a non-photo isn't retried every scan)
                    if s.auto_import_seen.get(&ps) == Some(&u64::MAX) {
                        continue;
                    }
                    let seen = s.auto_import_seen.insert(ps.clone(), size);
                    if size > 0 && seen == Some(size) {
                        fresh.push(ps);
                    }
                }
                if fresh.is_empty() {
                    return Ok(json!({"imported": [], "folder": folder}));
                }
                let mode = if s.prefs.import_defaults.auto_copy { "copy" } else { "add" };
                // a photo deleted from the library doesn't come back on its own because its file
                // is still in the watched folder (a copy's source is not a known path)
                let mut params = json!({"paths": fresh, "mode": mode, "onDeleted": "skip"});
                if let Some(a) = s.prefs.import_defaults.auto_album.clone() {
                    match s.catalog.albums().find(|al| al.name.eq_ignore_ascii_case(&a) && !al.folder && !al.is_smart()) {
                        Some(al) => params["album"] = json!(al.id.0),
                        None => params["albumName"] = json!(a),
                    }
                }
                for f in &fresh {
                    s.auto_import_seen.insert(f.clone(), u64::MAX);
                }
                if p.start == Some(false) {
                    // the caller imports them (the app: on a worker thread)
                    return Ok(json!({"imported": [], "folder": folder, "import": params}));
                }
                let sel = s.selection.clone();
                let r = s.execute("library.import", &params)?;
                // arriving photos don't take over the selection
                s.selection = sel;
                let mut result = json!({"imported": r["imported"], "folder": folder});
                if let Some(warnings) = r.get("warnings") {
                    result["warnings"] = warnings.clone();
                }
                Ok(result)
            }
        ),
        cmd!(
            "album.removePhotos",
            "Remove from Album",
            ["Photo"],
            None,
            "{id?: albumId (default: the album on screen), ids?} — takes the photos out of the album only: they stay in the library and in their folders (issue #1805; photo.delete removes them from the library) → {removed, album: name}",
            RemovePhotos,
            has_selection,
            |s, p| {
                let id = match p.id {
                    Some(id) => id,
                    None => match s.source {
                        LibrarySource::Album(a) => a,
                        _ => return Err(bad("album.removePhotos", "no album: pass `id` or show an album")),
                    },
                };
                let targets = p.ids.unwrap_or_else(|| s.selected());
                let al = s.catalog.album(id).ok_or_else(|| bad("album.removePhotos", "no such album"))?;
                if al.is_smart() {
                    return Err(bad("album.removePhotos", "smart albums update automatically: change their rules"));
                }
                if al.folder {
                    return Err(bad("album.removePhotos", "album folders don't hold photos"));
                }
                let name = al.name.clone();
                let targets: std::collections::HashSet<PhotoId> = targets.into_iter().collect();
                let photos: Vec<PhotoId> = al.photos.iter().copied().filter(|x| !targets.contains(x)).collect();
                let removed = al.photos.len().saturating_sub(photos.len());
                if removed > 0 {
                    s.commit("Remove from Album", Op::SetAlbumPhotos { id, photos })?;
                }
                Ok(json!({"removed": removed, "album": name}))
            }
        ),
        cmd!("album.setCover", "Set as Album Cover", [], None, "{id: albumId, photo?: photoId}", SetCover, has_active, |s, p| {
            let photo = p.photo.or(s.active());
            s.commit("Set Album Cover", Op::SetAlbumCover { id: p.id, cover: photo })?;
            ok()
        }),
        // ---- import
        cmd!(query "library.inspectLightroom", "Inspect Lightroom Catalog", [], None,
        "{path: .lrcat, status?, wait?, cancel?} → {photos, collections, missing, warnings}; read-only, no Lightroom required. The desktop app reads in the background: `status` (or no `path`) reports the task, `cancel` stops it, `wait` waits for it",
        LightroomPath, always, |_, p| {
            let _ = (p.wait, p.cancel);
            let Some(path) = lightroom_target(p.path.as_deref(), p.status) else { return Ok(Value::Null) };
            let data = crate::lightroom_catalog::read(path).map_err(|e| bad("library.inspectLightroom",e))?;
            Ok(json!({"photos":data.photos.len(),"collections":data.collections.iter().filter(|r|r.get("systemOnly").and_then(Value::as_f64).unwrap_or(0.0)==0.0).count(),"missing":data.photos.iter().filter(|p| !p.original_is_file()).map(|p|&p.path).collect::<Vec<_>>(),"warnings":data.warnings}))
        }),
        cmd!(
            "library.importLightroom",
            "Import Lightroom Catalog",
            [],
            None,
            "{path: .lrcat, updateExisting?: false, status?, wait?, cancel?}; direct import of paths, ratings, flags, metadata, keywords, collections, virtual copies & mapped edits. Archives source settings/history first. Existing edited photos are preserved unless updateExisting=true; source catalog is read-only.",
            ImportLightroom,
            always,
            |s, p| {
                let _ = (p.wait, p.cancel);
                let Some(path) = lightroom_target(p.path.as_deref(), p.status) else { return Ok(Value::Null) };
                crate::lightroom_catalog::import(s, path, p.update_existing)
            }
        ),
        cmd!(
            query "library.importPreview",
            "Review Import",
            [],
            None,
            "{paths: [file or folder (recursive)]} → {candidates: [{path, name, format, kind, width, height, fileSize, captured, duplicate?: path|content, existing?, error?, previewOnly?: why a raw can only be shown from its embedded preview}], duplicates, scanned} — nothing is added",
            Paths,
            always,
            |s, p| {
                let paths = p.paths;
                if paths.is_empty() {
                    return Err(bad("library.importPreview", "no paths"));
                }
                let c = crate::import::scan(s, &paths);
                let dups = c.iter().filter(|c| c.duplicate.is_some()).count();
                Ok(json!({"scanned": c.len(), "duplicates": dups, "candidates": c}))
            }
        ),
        cmd!(
            "library.import",
            "Import Photos",
            ["File"],
            Some("Cmd+Shift+I"),
            "{paths: [file or folder (recursive)], mode?: add|copy|move (add = reference the files in place; copy = into the library's Originals/YYYY/YYYY-MM-DD/; move = as copy, then each original and its XMP sidecars are removed from the source — only after the copy is verified (a hard link on the same volume, else copied, synced and compared byte for byte) and its catalog record is saved; failed, duplicate and unchecked files keep their sources; a taken name gets -1, -2…; undo removes the photos from the library but leaves the files at the destination), destination?: folder for copies / moves, organize?: date (YYYY/YYYY-MM-DD) | month (YYYY/YYYY-MM) | flat | a folder template, e.g. `{date:%Y}/{date:%Y%m%d}` → 2026/20260114 (the template's `/` make the folders, each level expanded with the rename tokens and made a safe folder name: never outside the destination; must be relative, no `..`; a level with missing metadata is `unknown`) — dated by capture time, else the import time, rename?: file-name template for copies, original extension added (tokens: {name} {num} {seq} {seq:N} {date} {date:%Y%m%d} {folder} {camera} {lens} {iso} {rating} {title} {creator} {ext}; photo.renameTokens explains each; blank = keep names), renameStart?: 1, metadataPreset?: name, dng?: bool (copy raws as DNG; copy only), local?: bool (browsing: the photos stay out of the library, like library.browse; not with move), album?: albumId, albumName?: new album, preset?: presetId, keywords?: [..], onDeleted?: restore|skip|fresh (a file that is in Recently Deleted: restore (default) = bring the photo back with its edits — relinked to the file when its own is gone, skip = leave it there and say so, fresh = delete the trashed record and import the file as new)} → {imported, duplicates: [{path, existing, reason, existingDeleted?}], restored?: [photoId], failed, warnings?: [{path, code: cameraProfile|uprightGeometry|unmappedXmp, profile?, mode?: off|auto|level|vertical|full|guided|unknown, fields?}], moved?: [{from, to, sidecars?}], kept?: [{path, reason}] (move: sources left in place and why), album?}",
            Import,
            always,
            |s, p| {
                let req = import_request(s, p)?;
                if req.paths.is_empty() {
                    return Err(bad("library.import", "no paths"));
                }
                let undo0 = s.history.undo.len();
                let report = crate::import::import_with(s, &req.paths, &req.opts)?;
                let report = import_batch_done(s, report, req.album, req.album_name.as_deref())?;
                // one undo step for the whole import
                let ids_of = |k: &str| -> Vec<u64> { report[k].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default() };
                let (imported, restored) = (ids_of("imported"), ids_of("restored"));
                let n = s.history.undo.len().saturating_sub(undo0);
                let (verb, count) = if imported.is_empty() && !restored.is_empty() { ("Restore", restored.len()) } else { ("Add", imported.len()) };
                s.merge_undo(n, &format!("{verb} {count} Photo{}", if count == 1 { "" } else { "s" }));
                let imported = if imported.is_empty() { restored } else { imported };
                if let Some(f) = imported.first() {
                    s.selection = Selection::single(PhotoId(*f));
                }
                Ok(report)
            }
        ),
        // ---- persistence
        cmd!(query "library.info", "Library Info", [], None, "{}", NoParams, always, |s, _| {
            let photos = s.catalog.len();
            let albums = s.catalog.albums().count();
            let (rendered_n, rendered_bytes) = s.media.rendered.mem_usage();
            let (sources_n, sources_bytes) = s.media.source_usage();
            let disk = s.media.rendered.disk().map(|d| {
                use std::sync::atomic::Ordering::Relaxed;
                json!({
                    "path": d.dir().display().to_string(),
                    "bytes": d.size(),
                    "hits": d.hits.load(Relaxed),
                    "misses": d.misses.load(Relaxed),
                    "writes": d.writes.load(Relaxed),
                })
            });
            let cache = json!({
                "renderedInMemory": rendered_n, "renderedBytes": rendered_bytes,
                "sourcesInMemory": sources_n, "sourceBytes": sources_bytes,
                "disk": disk,
            });
            let Some(lib) = &s.library else {
                return Ok(json!({"persistent": false, "photos": photos, "albums": albums, "cache": cache}));
            };
            let j = lib.journal();
            Ok(json!({
                "persistent": true,
                "path": lib.dir.display().to_string(),
                "photos": photos,
                "albums": albums,
                "seq": j.seq(),
                "snapshotSeq": j.snapshot_seq(),
                "logRecords": j.log_records(),
                "logBytes": j.log_bytes(),
                "lastError": lib.last_error,
                // settings files that were unreadable or damaged at open (kept, defaults used)
                "settingsWarnings": lib.settings_warnings,
                // changes applied in memory whose write failed (retried by every save)
                "unsavedOps": s.unsaved().map_or(0, |u| u.0),
                "unsavedError": s.unsaved().map(|u| u.1),
                // untouched Local records forgotten when the library opened
                "forgotLocal": lib.forgot_local.as_ref().map(|p| json!({"forgotten": p.evict.len(), "local": p.local, "keptRecent": p.kept_recent, "keptTouched": p.kept_touched, "keptInUse": p.kept_in_use})),
                "persistence": j.stats(),
                "cache": cache,
                "load": {
                    "created": lib.report.created,
                    "replayed": lib.report.replayed,
                    "tornBytes": lib.report.torn_bytes,
                    "damaged": lib.report.damaged,
                    "restored": lib.report.restored,
                    "checksumMismatch": lib.report.checksum_mismatch,
                },
            }))
        }),
        cmd!(
            "library.backup",
            "Back Up Library",
            ["File"],
            None,
            "{dir?: folder the backup folder is made in (default: the library's Backups folder)} → {path, photos}: a new folder `LightCraft Library <date time>` holding the catalog (unsaved changes included), presets, preferences and view; it opens as a library (File ▸ Open Library), which is how it is restored. Originals are not copied.",
            Backup,
            has_library,
            |s, p| {
                let path = s.backup_library(p.dir.as_deref().map(str::trim).filter(|d| !d.is_empty()).map(std::path::PathBuf::from))?;
                Ok(json!({"path": path.display().to_string(), "photos": s.catalog.len()}))
            }
        ),
        cmd!("library.compact", "Optimize Library", ["File"], None, "{}", NoParams, has_library, |s, _| {
            s.compact_library()?;
            ok()
        }),
        cmd!("library.clearPreviews", "Clear Preview Cache", ["File"], None, "{}", NoParams, always, |s, _| {
            s.media.rendered.clear();
            s.media.clear_sources();
            ok()
        }),
    ]
}

/// `base` with a partial Filter (JSON) merged on top.
/// `patch` merged onto `base`, its rules brought up to date (`RuleSet::upgrade`), unchecked (see
/// [`merge_rules`]).
fn merge_filter(base: &lightcraft_catalog::Filter, patch: &Value, c: &str) -> Result<lightcraft_catalog::Filter> {
    let mut v = serde_json::to_value(base).unwrap_or_default();
    lightcraft_develop::presets::deep_merge(&mut v, patch);
    let mut f: lightcraft_catalog::Filter = super::params::from_value(&v).map_err(|e| match e {
        crate::EngineError::BadParams { msg, .. } => bad(c, msg),
        e => e,
    })?;
    if let Some(rules) = f.rule_set.as_mut() {
        rules.upgrade();
    }
    Ok(f)
}

/// `patch` merged onto `base`, refused when the rule set it sets has problems (`RuleSet::check_for`;
/// `owner` is the smart album the rules are for, so testing an album that leads back to it is a
/// problem too).
fn merge_rules(
    base: &lightcraft_catalog::Filter,
    patch: &Value,
    c: &str,
    cat: &lightcraft_catalog::Catalog,
    owner: Option<AlbumId>,
) -> Result<lightcraft_catalog::Filter> {
    let f = merge_filter(base, patch, c)?;
    // only rules this change sets are checked: one that stopped checking since (its album was
    // deleted) doesn't block editing the album's other settings
    let sets_rules = patch.get("ruleSet").is_some_and(|r| !r.is_null());
    let mut problems = f.rule_set.as_ref().filter(|_| sets_rules).map(|r| r.check_for(cat, owner)).unwrap_or_default();
    // and a loop through the album field the change sets
    if patch.get("album").is_some_and(|a| !a.is_null()) {
        problems.extend(cat.album_filter_problem(&f, owner));
    }
    if !problems.is_empty() {
        return Err(bad(c, problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")));
    }
    Ok(f)
}

/// Whether `path` lies above a disk: some of its `ids` photos are on a disk (not the startup
/// disk) that the folder does not lie in. Such a folder (`/Volumes`, `/mnt`…) is not a folder of
/// one disk but a way to reach several.
pub(crate) fn covers_other_disks(s: &Session, path: &str, ids: &[PhotoId]) -> bool {
    ids.iter()
        .filter_map(|id| s.catalog.photo(*id))
        .filter_map(|p| lightcraft_catalog::folders::volume_of(p))
        .any(|v| v != "/" && !lightcraft_catalog::query::folder_within(path, &v))
}

/// The current view (source + filter) as smart-album rules. Viewing a smart album starts from
/// its rules with the filter bar's settings on top.
fn view_rules(s: &Session) -> lightcraft_catalog::Filter {
    use lightcraft_catalog::Filter;
    let smart = match s.source {
        LibrarySource::Album(a) => s.catalog.album(a).and_then(|a| a.smart.as_deref().cloned()),
        _ => None,
    };
    match smart {
        Some(base) => {
            // overlay the fields the filter bar changed
            let cur = serde_json::to_value(&s.filter).unwrap_or_default();
            let def = serde_json::to_value(Filter::default()).unwrap_or_default();
            let mut patch = serde_json::Map::new();
            if let (Some(c), Some(d)) = (cur.as_object(), def.as_object()) {
                for (k, v) in c {
                    if d.get(k) != Some(v) {
                        patch.insert(k.clone(), v.clone());
                    }
                }
            }
            // the view as it is: a rule that no longer checks (its album was deleted) just matches
            // nothing there, and mustn't cost the filter bar's settings
            merge_filter(&base, &Value::Object(patch), "").unwrap_or(base)
        }
        None => {
            let mut f = s.source.to_filter(&s.filter, &s.catalog);
            f.deleted = false;
            if s.source == LibrarySource::LibraryFolder {
                f.library_folder = s.library_folder.clone();
            }
            f
        }
    }
}

impl Session {
    /// The current view (source + filter) as smart-album rules (see `album.createSmart`).
    pub fn view_rules(&self) -> lightcraft_catalog::Filter {
        view_rules(self)
    }
}

fn has_library(s: &Session) -> std::result::Result<(), String> {
    if s.library.is_some() { Ok(()) } else { Err("no library is open (in-memory session)".into()) }
}

/// Coordinates typed by a person: "51.5072, -0.1276", "51.5072 N 0.1276 W",
/// "51°30'26\"N 0°7'39\"W" (degrees, minutes, seconds; N/S/E/W or signs).
pub(crate) fn parse_gps(t: &str) -> Option<(f64, f64)> {
    let t = t.trim();
    // split into the two coordinates: at a comma, else after the first N/S hemisphere letter
    let (a, b) = match t.split_once(',') {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => {
            let i = t.find(['N', 'S', 'n', 's']).map(|i| i + 1).or_else(|| t.find(char::is_whitespace))?;
            (t[..i].trim().to_string(), t[i..].trim().to_string())
        }
    };
    let one = |s: &str, pos: char, neg: char| -> Option<f64> {
        let up = s.to_ascii_uppercase();
        let sign = if up.contains(neg) || up.trim_start().starts_with('-') { -1.0 } else { 1.0 };
        let nums: Vec<f64> =
            up.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|x| !x.is_empty()).map(|x| x.parse::<f64>().ok()).collect::<Option<_>>()?;
        let v = match nums.as_slice() {
            [d] => *d,
            [d, m] => d + m / 60.0,
            [d, m, s] => d + m / 60.0 + s / 3600.0,
            _ => return None,
        };
        let _ = pos;
        Some(sign * v)
    };
    let (la, lo) = (one(&a, 'N', 'S')?, one(&b, 'E', 'W')?);
    (la.abs() <= 90.0 && lo.abs() <= 180.0).then_some((la, lo))
}

#[cfg(test)]
mod gps_tests {
    #[test]
    fn coordinates_people_type() {
        let p = super::parse_gps;
        assert_eq!(p("51.5072, -0.1276"), Some((51.5072, -0.1276)));
        let (la, lo) = p("51°30'26\"N 0°7'39\"W").unwrap();
        assert!((la - 51.50722).abs() < 1e-4 && (lo + 0.1275).abs() < 1e-4, "{la} {lo}");
        let (la, lo) = p("33.8688 S, 151.2093 E").unwrap();
        assert!((la + 33.8688).abs() < 1e-9 && (lo - 151.2093).abs() < 1e-9);
        assert_eq!(p("95, 10"), None);
        assert_eq!(p("hello"), None);
    }
}

#[cfg(test)]
#[path = "library/tests_relative_ratings.rs"]
mod relative_rating_tests;

#[cfg(test)]
#[path = "library/tests_flag_cycle.rs"]
mod flag_cycle_tests;
