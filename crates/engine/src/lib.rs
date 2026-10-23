//! The LightCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`photo.rate`, `develop.set`,
//! `album.create`, `mask.add`…) and JSON parameters. The egui UI, the CLI, the control channel and
//! the MCP server all go through [`Session::execute`].
//!
//! State: a [`Catalog`] (mutated only by ops, so every change is undoable and journaled), the
//! library view (filter/sort/source), the selection, the develop clipboard, presets, and caches
//! of decoded source proxies. Rendering is done by [`RenderJob`]s that are `Send` so frontends can
//! run them off the UI thread.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::disallowed_methods))]

// Model/cache types remain available without linking the optional inference crate.
#[cfg(not(feature = "denoise"))]
extern crate lightcraft_denoise_core as lightcraft_denoise;

pub mod activity;
#[cfg(not(target_arch = "wasm32"))]
pub mod apple_photos;
pub mod availability;
pub(crate) mod camera_preview;
pub mod camera_profiles;
pub mod cmd;
pub mod config;
pub mod contact_sheet;
pub mod control_auth;
pub mod crs;
pub mod crs_masks;
pub mod demo;
pub mod denoise;
pub mod devices;
pub mod display;
pub mod doc_path;
mod errors;
pub mod export;
pub mod face_download;
pub mod face_tiles;
#[cfg(not(target_arch = "wasm32"))]
mod faces_index;
#[cfg(not(target_arch = "wasm32"))]
mod faces_worker;
pub mod files;
#[cfg(not(target_arch = "wasm32"))]
mod focus_scan;
pub mod guard;
mod history;
pub mod import;
mod import_move;
pub mod lens_profiles;
pub mod library;
mod lightroom_archive;
pub mod lightroom_catalog;
pub mod lightroom_folder;
pub mod lightroom_job;
mod lightroom_sqlite;
pub mod logging;
pub mod media;
pub mod memory;
pub mod merge;
mod model_download;
pub mod open_path;
pub mod originals;
mod pdf;
pub mod preset_import;
pub mod preset_luminar;
pub mod presets;
pub mod print;
pub mod print_color;
pub mod print_pdf;
pub mod profile_refs;
pub mod proof;
pub mod rename;
pub mod segment;
pub mod sidecar;
pub mod slide;
pub mod slide_overlay;
pub mod slide_template;
pub mod slide_title;
pub mod slideshow;
pub mod slideshow_export;
pub mod smart;
pub mod sync;
pub mod text;
mod view;
pub mod walk;

use std::sync::Arc;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use errors::{AiModel, CommandError, ErrorKind, FileProblem};
pub use history::{FolderMove, Interaction, UndoEntry};
use lightcraft_catalog::{Catalog, Filter, Op, PhotoId, Sort};
use lightcraft_develop::DevelopSettings;
pub use media::{RenderJob, SourceLevel};
use serde_json::Value;
pub use view::{Browse, FilterChip, LibrarySource, Selection, SelectionState, filter_chips};
pub use {lightcraft_catalog as catalog, lightcraft_develop as develop, lightcraft_gpu as gpu, lightcraft_pipeline as pipeline};

#[derive(Clone, Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("{0}")]
    Catalog(#[from] lightcraft_catalog::CatalogError),
    /// The command's change is applied (in memory, undoable) but its journal records could not
    /// be written. They stay queued and are written by the next successful save.
    #[error("saved in memory but not written to disk: {0}; LightCraft will retry")]
    NotSaved(String),
    /// Another process (the app, `lightcraft-cli`, another computer) has the library open.
    #[error("{0}")]
    LibraryInUse(String),
    /// The folder (or file) asked for is a Lightroom Classic catalog or part of one, not a
    /// LightCraft library (issue #891): nothing was created or opened. `catalog`: the `.lrcat`
    /// to import with `library.importLightroom`, when one was found.
    #[error("{} is a Lightroom Classic catalog, not a LightCraft library; import it into a LightCraft library with File ▸ Import Lightroom Catalog…{}", path.display(), catalog.as_ref().map_or(String::new(), |c| format!(" ({})", c.display())))]
    LightroomCatalog { path: std::path::PathBuf, catalog: Option<std::path::PathBuf> },
    /// The work was cancelled (by the person, an agent or a newer request): not a failure.
    #[error("{0}")]
    Cancelled(String),
    /// The feature needs `model`, which is not installed (or still downloading): a frontend
    /// offers its download (terms first) on [`ErrorKind::ModelMissing`], not on the message.
    #[error("{msg}")]
    ModelMissing { model: AiModel, msg: String },
    /// A file could not be read, decoded or written: [`ErrorKind::File`].
    #[error("{msg}")]
    File { problem: FileProblem, msg: String },
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

static LIBRARY_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct Session {
    /// The library's preferences (`prefs.json`): saved with [`Session::save_prefs`], read when it opens.
    pub prefs: library::LibraryPrefs,
    library_generation: u64,
    /// Auto Sync: edits to the active photo also change the other selected photos (the settings
    /// that changed, nothing else).
    pub auto_sync: bool,
    pub catalog: Catalog,
    pub source: LibrarySource,
    pub filter: Filter,
    pub sort: Sort,
    pub selection: Selection,
    /// The grid order for the current source, filter and sort, and what keeps it up to date.
    view: view::ViewCache,
    /// Undo and redo, the slider drag in progress and the ops still to log.
    pub history: history::History,
    /// Set by a command whose change must not rewrite the photo's XMP sidecar even with auto-write on
    /// (a catalog-only edit of data the sidecar writer does not emit); consumed when the command ends.
    pub(crate) skip_auto_write: bool,
    /// Photos whose automatic XMP sidecar write failed (their folder offline, read-only or full), written again by
    /// [`Session::retry_unsaved_sidecars`] once their file is reachable.
    pub(crate) unsaved_sidecars: std::collections::BTreeSet<PhotoId>,
    /// AI denoise: the model in use, the photos that have their picture and the work in progress.
    pub(crate) denoise: denoise::State,
    /// Where the host keeps face models (one folder each); `None` where there is no file system (the web).
    pub face_models_dir: Option<std::path::PathBuf>,
    /// Face model downloads started this session (the staged files wait in `<face_models_dir>/.downloads`).
    pub face_downloads: face_download::Downloads,
    /// Reverse geocoding: the parsed place names and their download (`metadata.*PlaceNames`).
    pub place_names: cmd::geocode::PlaceNames,
    /// The user's own list of models to download (`catalog.json` in the models folder), as last read.
    pub(crate) face_catalog: lightcraft_faces::catalog::Catalog,
    /// The loaded recognition model and the face embeddings made with it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) faces: faces_index::FacesState,
    /// Reading focus points for photos imported before they were read (`photo.readFocusPoints`, `focus.pump`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) focus_scan: focus_scan::FocusScan,
    /// Copied develop settings (partial JSON) for Paste.
    pub clipboard: Option<Value>,
    /// The folder on disk the [`LibrarySource::Folder`] view browses.
    pub browse: Option<Browse>,
    /// The folder the [`LibrarySource::LibraryFolder`] view shows: the library's photos imported
    /// from it and from the folders inside it.
    pub library_folder: Option<String>,
    /// Copied metadata (`photo.copyMetadata`): photo.setMeta params.
    pub meta_clipboard: Option<Value>,
    /// The photo that was active before the current one (Paste Settings from Previous).
    pub previous_active: Option<PhotoId>,
    /// Groups last used for Copy (Lightroom remembers them).
    pub copy_groups: Vec<lightcraft_develop::SettingsGroup>,
    /// Built-in then user presets; its generation tells savers and caches when it changed.
    pub presets: presets::PresetList,
    /// Favourite profile ids (persisted with the library, like preset favourites).
    pub profile_favorites: Vec<String>,
    /// Recently applied profile ids, newest first (at most [`presets::RECENT_PROFILES`]).
    pub profile_recent: Vec<String>,
    /// Executed commands (actions / debugging / replay).
    pub journal: Vec<(String, Value)>,
    pub media: media::MediaCache,
    /// Current time provider (ISO 8601); injectable for tests and wasm.
    pub clock: Box<dyn Fn() -> String + Send>,
    depth: u32,
    /// Selected mask (Masking panel), by mask id.
    pub active_mask: Option<u32>,
    /// AI masks (SAM 3): the model and the last photo prepared for it.
    pub segmenter: segment::Segmenter,
    /// Selected spot (Remove panel), by index into the active photo's spots.
    pub active_spot: Option<usize>,
    /// The persistent library this session writes to (`None` = in-memory only).
    pub library: Option<library::Library>,
    /// Identity of current library opening; refreshed only after a successful open.
    pub(crate) library_identity: Arc<()>,
    /// The Print module's current layout, image settings, page, guides and job (`print.*`). Lives
    /// with the session, so the app, CLI, control channel and MCP all see the same print.
    pub print: print::template::PrintSettings,
    /// Look-alike signatures by content key (Find Similar).
    pub signatures: std::collections::HashMap<String, [f32; 64]>,
    /// The target album B adds to (`None` = the Quick Collection).
    pub target_album: Option<lightcraft_catalog::AlbumId>,
    /// Auto import: files seen in the watched folder and their size then (a file is imported once
    /// its size held between two scans).
    pub auto_import_seen: std::collections::HashMap<String, u64>,
    /// Before/After: the "before" settings chosen per photo (this session; default: the photo's
    /// import state). See `cmd/before.rs`.
    pub before: std::collections::HashMap<PhotoId, Arc<DevelopSettings>>,
    /// File probes from the last import review (`library.importPreview`), reused by the import.
    pub import_probes: std::collections::HashMap<String, media::ProbeInfo>,
    /// The last Synchronize Folder scan, which `folder.synchronize` acts on while it is current
    /// (see [`Session::take_folder_changes`]).
    pub folder_changes: Option<sync::FolderChanges>,
    /// Background tasks in flight (imports, exports, preview builds…), for the activity stack and
    /// `activity.list` / `activity.cancel`.
    pub activity: activity::Activity,
    /// The last (or running) slideshow export (`slideshow.export`).
    pub slideshow_export: Option<std::sync::Arc<slideshow_export::ExportRun>>,
    /// The last (or running) Build Previews.
    pub preview_build: Option<std::sync::Arc<cmd::previews::PreviewBuild>>,
    /// Where user slideshow templates are kept (`<config>/slideshow-templates`;
    /// [`Session::with_default_slide_templates`]). `None`: the built-in templates only.
    pub slide_templates_dir: Option<std::path::PathBuf>,
    /// The slideshow settings before the last template was applied ("Previous settings"; this
    /// session only: library preferences have no undo).
    pub slide_templates_previous: Option<slide_template::SlideLook>,
    /// The templates as last listed.
    pub(crate) slide_templates_cache: slide_template::Cache,
    /// The background image the slides last rendered (shared by slide jobs: rendered once).
    pub slide_backdrop: std::sync::Arc<slide::BackdropCache>,
    /// Adds exported files to Apple Photos ([`apple_photos`]): `/usr/bin/osascript`, installed by
    /// [`Session::with_fs`] on macOS; `None` elsewhere and in sessions without a file system. Tests
    /// put a fake here.
    #[cfg(not(target_arch = "wasm32"))]
    pub apple_photos: Option<apple_photos::Runner>,
    /// This session's Apple Photos imports, running or finished (`export.photosImports`).
    #[cfg(not(target_arch = "wasm32"))]
    pub apple_photos_imports: apple_photos::Imports,
    /// Apple Photos imports run on a worker by default (`export.addToPhotos` without `wait`,
    /// `app.export` without `background`): set by the desktop app, which answers control requests
    /// on its UI thread. Off (the default), they wait for Photos, so a one-shot process
    /// (lightcraft-cli, a script) never ends in the middle of one.
    #[cfg(not(target_arch = "wasm32"))]
    pub apple_photos_background: bool,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn library_generation(&self) -> u64 {
        self.library_generation
    }

    pub fn new() -> Session {
        Session {
            prefs: Default::default(),
            library_generation: LIBRARY_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            auto_sync: false,
            catalog: Catalog::new(),
            source: LibrarySource::All,
            filter: Filter::default(),
            sort: Sort::default(),
            selection: Selection::default(),
            view: Default::default(),
            history: Default::default(),
            skip_auto_write: false,
            unsaved_sidecars: Default::default(),
            denoise: Default::default(),
            face_models_dir: None,
            face_downloads: Default::default(),
            place_names: Default::default(),
            face_catalog: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            faces: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            focus_scan: Default::default(),
            clipboard: None,
            meta_clipboard: None,
            browse: None,
            library_folder: None,
            previous_active: None,
            copy_groups: lightcraft_develop::SettingsGroup::default_copy(),
            presets: presets::PresetList::new(presets::builtin()),
            profile_favorites: Vec::new(),
            profile_recent: Vec::new(),
            journal: Vec::new(),
            media: media::MediaCache::default(),
            clock: Box::new(|| "2026-09-30T12:00:00".to_string()),
            depth: 0,
            active_mask: None,
            segmenter: segment::Segmenter::default(),
            active_spot: None,
            library: None,
            library_identity: Arc::new(()),
            print: Default::default(),
            signatures: Default::default(),
            target_album: None,
            auto_import_seen: Default::default(),
            before: Default::default(),
            import_probes: Default::default(),
            folder_changes: None,
            activity: Default::default(),
            preview_build: None,
            slideshow_export: None,
            slide_templates_dir: None,
            slide_templates_previous: None,
            slide_templates_cache: None,
            slide_backdrop: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            apple_photos: None,
            #[cfg(not(target_arch = "wasm32"))]
            apple_photos_imports: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            apple_photos_background: false,
        }
    }

    /// A session with the procedurally generated demo library loaded.
    pub fn with_demo() -> Session {
        let mut s = Session::new();
        demo::load(&mut s);
        s
    }

    /// Run a command by id. THE entry point for every frontend.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        // a command that validates the photos a call names isn't held back by the selection
        if !(spec.explicit_targets && cmd::names_photos(params)) {
            (spec.enabled)(self).map_err(|why| EngineError::Disabled(id.to_string(), why))?;
        }
        let empty = Value::Object(Default::default());
        let params = if params.is_null() { &empty } else { params };
        self.run_command(id, spec.journal.then_some(params), |s| (spec.run)(s, params)).map_err(|e| match e {
            // a shared parameter check ([`Session::targets`]) names the command it ran for
            EngineError::BadParams { cmd, msg } if cmd.is_empty() => EngineError::BadParams { cmd: id.to_string(), msg },
            e => e,
        })
    }

    /// Run `f` as command `id` with what [`Session::execute`] does around every command: the
    /// panic guard, auto versions, XMP sidecar auto-write and the durable save (a failed save is
    /// `NotSaved`). Not journaled. The app's import task commits its batches this way.
    pub fn execute_fn(&mut self, id: &str, f: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Value> {
        self.run_command(id, None, f)
    }

    fn run_command(&mut self, id: &str, journal: Option<&Value>, f: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Value> {
        let log_start = self.history.pending_log.len();
        let was_active = self.active();
        self.depth += 1;
        // last-resort guard: a panic in a command is that command's error, not a crash
        let r = guard::catch(&format!("`{id}`"), || f(self)).unwrap_or_else(|e| Err(EngineError::Other(e)));
        self.depth -= 1;
        if self.depth == 0 && was_active.is_some() && self.active() != was_active {
            self.previous_active = was_active;
            if let Some(left) = was_active {
                self.auto_version(left);
            }
        }
        if r.is_ok()
            && let Some(params) = journal
            && self.depth == 0
        {
            self.journal.push((id.to_string(), params.clone()));
            if self.journal.len() > 10_000 {
                self.journal.drain(..1000);
            }
        }
        if self.depth == 0 {
            let skip = std::mem::take(&mut self.skip_auto_write);
            if r.is_ok() && !skip && self.prefs.xmp.auto_write && self.history.interaction.is_none() && self.history.pending_log.len() > log_start {
                self.auto_write_sidecars_since(log_start);
            }
        }
        if self.depth == 0 && self.library.is_some() {
            // Make the command durable before reporting success. When the command's own records
            // can't be appended, it fails with `NotSaved`: the change stays applied in memory and
            // queued, and the next save (any later command, or the frame loop) retries it. Other
            // persistence trouble (a failed compaction, an older queued change still unwritten by
            // a command that changed nothing) doesn't fail the command; `library.info` reports it.
            let produced = self.history.pending_log.len() > log_start;
            if let Err(e @ EngineError::NotSaved(_)) = self.persist()
                && produced
                && r.is_ok()
            {
                return Err(e);
            }
        }
        r
    }

    pub fn commands(&self) -> Vec<CommandInfo> {
        command_specs().iter().map(|c| c.info(self)).collect()
    }

    /// The box to cut a face's picture from: the detector's, when the scan has looked at the face (every face is then shown
    /// equally close, however loosely or tightly its own region was drawn), else the region's own box.
    pub fn face_view(&self, id: PhotoId, rect: lightcraft_geom::Rect) -> lightcraft_geom::Rect {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(v) = self.faces.index.view(id.0, &rect) {
            return v;
        }
        let _ = id;
        rect
    }

    // ---------------------------------------------------------------- develop edits

    /// The photo being edited (the active photo).
    pub fn active(&self) -> Option<PhotoId> {
        self.selection.active.filter(|id| self.catalog.photo(*id).is_some())
    }

    pub fn develop_of(&self, id: PhotoId) -> Option<Arc<DevelopSettings>> {
        self.catalog.photo(id).map(|p| p.develop.clone())
    }

    /// Change a photo's develop settings. During an interaction the change is previewed without an
    /// undo step; otherwise it is committed with a history entry.
    pub fn set_develop(&mut self, id: PhotoId, settings: DevelopSettings, label: &str) -> Result<()> {
        let now = (self.clock)();
        let settings = Arc::new(settings);
        if let Some(i) = &self.history.interaction
            && i.photo == id
        {
            return self.apply_silent(Op::SetDevelop { id, settings, label: label.into(), edited: Some(now) });
        }
        let mut ops = vec![self.develop_op(id, (*settings).clone(), label).ok_or(lightcraft_catalog::CatalogError::NoPhoto(id))?];
        ops.extend(self.auto_sync_ops(id, &settings, label));
        let op = if ops.len() == 1 { ops.remove(0) } else { Op::Batch { ops } };
        self.commit(label, op)
    }

    /// With Auto Sync on, the ops that carry an edit of the active photo `id` (to `new`) over to the
    /// other selected photos: only the settings that changed; never spot removal or red eye (they
    /// belong to one photo's pixels), nor the rendering process (Sync doesn't carry it either), nor
    /// history / snapshot restores.
    fn auto_sync_ops(&self, id: PhotoId, new: &DevelopSettings, label: &str) -> Vec<Op> {
        if !self.auto_sync
            || self.active() != Some(id)
            || self.selection.ids.len() < 2
            || label.starts_with("History:")
            || label.starts_with("Restore ")
        {
            return Vec::new();
        }
        let Some(old) = self.develop_of(id) else { return Vec::new() };
        let Some(mut delta) = json_delta(&old.to_json(), &new.to_json()) else { return Vec::new() };
        if let Some(o) = delta.as_object_mut() {
            for k in ["spots", "red_eye", "version", "process"] {
                o.remove(k);
            }
            if o.is_empty() {
                return Vec::new();
            }
        }
        self.selection
            .ids
            .iter()
            .filter(|x| **x != id)
            .filter_map(|x| self.develop_of(*x).map(|d| (*x, d)))
            .filter_map(|(x, d)| {
                let synced = lightcraft_develop::apply_partial(&d, &delta, 1.0);
                (synced != *d).then(|| self.develop_op(x, synced, label)).flatten()
            })
            .collect()
    }

    /// The op that sets a photo's develop settings and appends a History entry (for batches).
    pub fn develop_op(&self, id: PhotoId, settings: DevelopSettings, label: &str) -> Option<Op> {
        self.catalog.photo(id)?;
        let settings = Arc::new(settings);
        let step = lightcraft_catalog::HistoryStep { label: label.into(), settings: settings.clone() };
        Some(Op::Batch {
            ops: vec![Op::SetDevelop { id, settings, label: label.into(), edited: Some((self.clock)()) }, Op::PushHistory { id, step }],
        })
    }

    pub fn begin_interaction(&mut self, label: &str) -> Result<()> {
        if self.history.interaction.is_some() {
            self.end_interaction()?;
        }
        let id = self.active().ok_or_else(|| EngineError::Other("no active photo".into()))?;
        let original = self.develop_of(id).unwrap_or_default();
        self.history.interaction = Some(Interaction { label: label.into(), photo: id, original });
        Ok(())
    }

    /// Commit the interaction as one undo step (no-op if nothing changed).
    pub fn end_interaction(&mut self) -> Result<()> {
        let Some(i) = self.history.interaction.take() else { return Ok(()) };
        let Some(cur) = self.develop_of(i.photo) else { return Ok(()) };
        if *cur == *i.original {
            return Ok(());
        }
        // Restore the original silently, then commit the final value as one step.
        self.apply_silent(Op::SetDevelop { id: i.photo, settings: i.original.clone(), label: i.label.clone(), edited: None })?;
        self.set_develop(i.photo, (*cur).clone(), &i.label)
    }

    pub fn cancel_interaction(&mut self) -> Result<()> {
        if let Some(i) = self.history.interaction.take() {
            self.apply_silent(Op::SetDevelop { id: i.photo, settings: i.original, label: i.label, edited: None })?;
        }
        Ok(())
    }

    /// Targets of photo commands: the explicit `ids` (photo ids) or `id` (a photo id) param, else [`Self::selected`].
    /// An `ids` or `id` that is there but isn't photo ids is an error, never the selection or part of the list
    /// (issue #1506): a mistyped target mustn't turn a command meant for one photo into one for every selected photo.
    pub fn targets(&self, p: &Value) -> Result<Vec<PhotoId>> {
        let invalid = |what: &str| EngineError::BadParams { cmd: String::new(), msg: format!("`{what}` must be photo ids (whole numbers)") };
        match p.get("ids") {
            None | Some(Value::Null) => {}
            Some(Value::Array(a)) => return a.iter().map(|v| v.as_u64().map(PhotoId).ok_or_else(|| invalid("ids"))).collect(),
            Some(_) => return Err(invalid("ids")),
        }
        match p.get("id") {
            None | Some(Value::Null) => Ok(self.selected()),
            Some(v) => v.as_u64().map(|id| vec![PhotoId(id)]).ok_or_else(|| invalid("id")),
        }
    }

    /// Targets of a typed photo command: its `ids`, else its `id` (a photo), else [`Self::selected`].
    pub fn targets_of(&self, ids: Option<&[PhotoId]>, id: Option<PhotoId>) -> Vec<PhotoId> {
        match (ids, id) {
            (Some(ids), _) => ids.to_vec(),
            (None, Some(id)) => vec![id],
            (None, None) => self.selected(),
        }
    }

    /// The photos commands act on when a call names none: the selection, else the active photo.
    pub fn selected(&self) -> Vec<PhotoId> {
        if self.selection.ids.is_empty() { self.selection.active.into_iter().collect() } else { self.selection.ids.clone() }
    }
}

/// Automatic versions kept per photo.
pub const AUTO_VERSIONS: usize = 20;

/// The parts of `new` that differ from `old` (objects recurse; anything else is taken whole).
pub fn json_delta(old: &Value, new: &Value) -> Option<Value> {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            let m: serde_json::Map<String, Value> =
                b.iter().filter_map(|(k, nv)| json_delta(a.get(k).unwrap_or(&Value::Null), nv).map(|d| (k.clone(), d))).collect();
            (!m.is_empty()).then_some(Value::Object(m))
        }
        (a, b) if a == b => None,
        (_, b) => Some(b.clone()),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_album_order;
#[cfg(test)]
mod tests_album_remove;
#[cfg(test)]
mod tests_albums_local;
#[cfg(test)]
mod tests_color;
#[cfg(test)]
mod tests_crop_angle;
#[cfg(test)]
mod tests_denoise;
#[cfg(test)]
mod tests_export;
#[cfg(test)]
mod tests_face_models;
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests_face_recognize;
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests_face_tiles;
#[cfg(test)]
mod tests_filesystem_history;
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests_focus_scan;
#[cfg(test)]
mod tests_folders;
#[cfg(test)]
mod tests_forget_local;
#[cfg(test)]
mod tests_history_cursor;
#[cfg(test)]
mod tests_hold_view;
#[cfg(test)]
mod tests_import;
#[cfg(test)]
mod tests_import_move;
#[cfg(test)]
mod tests_keyword_list;
#[cfg(test)]
mod tests_libops;
#[cfg(test)]
mod tests_library;
#[cfg(test)]
mod tests_lightroom_folder;
#[cfg(test)]
mod tests_mask_effects;
#[cfg(test)]
mod tests_merge;
#[cfg(test)]
mod tests_move_to_share;
#[cfg(test)]
mod tests_organize;
#[cfg(test)]
mod tests_persist;
#[cfg(test)]
mod tests_prefs;
#[cfg(test)]
mod tests_presets_scale;
#[cfg(test)]
mod tests_print;
#[cfg(test)]
mod tests_process;
#[cfg(test)]
mod tests_recheck;
#[cfg(test)]
mod tests_segment;
#[cfg(test)]
mod tests_settings_files;
#[cfg(test)]
mod tests_slide;
#[cfg(test)]
mod tests_slideshow_export;
#[cfg(test)]
mod tests_spots;
#[cfg(test)]
mod tests_sync;
#[cfg(test)]
mod tests_view_follow;
#[cfg(test)]
mod tests_xmp;
