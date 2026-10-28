//! Library view state: what the grid shows and what is selected.

use std::sync::Arc;

use lightcraft_catalog::{AlbumId, Catalog, Filter, PhotoId, Sort};
use serde::{Deserialize, Serialize};

use crate::{Session, cmd};

/// The "My Photos" source in the left panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum LibrarySource {
    #[default]
    All,
    /// Imported in the last import session (we treat "recent" as the latest import date).
    RecentlyAdded,
    Album(AlbumId),
    RecentlyDeleted,
    /// Photos with picks.
    Picks,
    /// A folder on disk ([`crate::Session::browse`]): its files, added to the library or not.
    Folder,
    /// Photos whose original file can't be found (`library.missing`).
    Missing,
    /// A folder the library's photos were imported from ([`crate::Session::library_folder`]),
    /// and the folders inside it (`library.folders`).
    LibraryFolder,
}

/// The folder a [`LibrarySource::Folder`] view shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Browse {
    pub path: String,
    pub subfolders: bool,
}

/// How far back Recently Added reaches from the latest import.
pub const RECENT_DAYS: i64 = 30;

impl LibrarySource {
    pub fn to_filter(&self, f: &Filter, cat: &Catalog) -> Filter {
        let mut f = f.clone();
        match self {
            LibrarySource::All => {}
            // everything imported in the 30 days up to the latest import (by import, newest first)
            LibrarySource::RecentlyAdded => {
                let latest = cat.photos().filter(|p| p.in_library()).map(|p| p.imported.clone()).max().unwrap_or_default();
                let from = lightcraft_catalog::stacks::iso_seconds(&latest)
                    .map(|s| lightcraft_catalog::dates::civil(s - RECENT_DAYS * 86_400))
                    .unwrap_or_else(|| latest.get(..10).unwrap_or("").to_string());
                f.imported_from = Some(from);
            }
            LibrarySource::Album(a) => f.album = Some(*a),
            LibrarySource::RecentlyDeleted => f.deleted = true,
            LibrarySource::Picks => f.flag = Some(lightcraft_catalog::Flag::Pick),
            // the folder itself is filled in by the session (it holds the path)
            LibrarySource::Folder | LibrarySource::Missing | LibrarySource::LibraryFolder => {}
        }
        f
    }

    pub fn label(&self, cat: &Catalog) -> String {
        match self {
            LibrarySource::All => "All Photos".into(),
            LibrarySource::RecentlyAdded => "Recently Added".into(),
            LibrarySource::Album(a) => cat.album(*a).map(|a| a.name.clone()).unwrap_or_else(|| "Album".into()),
            LibrarySource::RecentlyDeleted => "Recently Deleted".into(),
            LibrarySource::Picks => "Picks".into(),
            LibrarySource::Folder => "Folder".into(),
            LibrarySource::LibraryFolder => "Folder".into(),
            LibrarySource::Missing => "Missing Photos".into(),
        }
    }
}

/// Selected photos (ordered) and the active (most-selected) one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub ids: Vec<PhotoId>,
    pub active: Option<PhotoId>,
}

/// How a photo relates to the selection: what every view draws (grid cell, filmstrip cell…).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionState {
    NotSelected,
    /// Selected, but not the photo the loupe and panels act on.
    Selected,
    /// The most-selected photo.
    Active,
}

impl Selection {
    /// What views draw for `id`: active, selected or neither (one answer for grid, filmstrip, …).
    pub fn state_of(&self, id: PhotoId) -> SelectionState {
        if self.active == Some(id) {
            SelectionState::Active
        } else if self.contains(id) {
            SelectionState::Selected
        } else {
            SelectionState::NotSelected
        }
    }
    pub fn single(id: PhotoId) -> Selection {
        Selection { ids: vec![id], active: Some(id) }
    }
    pub fn contains(&self, id: PhotoId) -> bool {
        self.ids.contains(&id)
    }
    pub fn toggle(&mut self, id: PhotoId) {
        if let Some(i) = self.ids.iter().position(|x| *x == id) {
            self.ids.remove(i);
            if self.active == Some(id) {
                self.active = self.ids.last().copied();
            }
        } else {
            self.ids.push(id);
            self.active = Some(id);
        }
    }
    /// Shift-click: select the range between the active photo and `id` in `order`.
    pub fn extend_to(&mut self, id: PhotoId, order: &[PhotoId]) {
        let anchor = self.active.unwrap_or(id);
        let (Some(a), Some(b)) = (order.iter().position(|x| *x == anchor), order.iter().position(|x| *x == id)) else {
            *self = Selection::single(id);
            return;
        };
        let (lo, hi) = (a.min(b), a.max(b));
        for x in &order[lo..=hi] {
            if !self.ids.contains(x) {
                self.ids.push(*x);
            }
        }
        self.active = Some(id);
    }
}

/// One active filter, shown as a removable chip above the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterChip {
    pub label: String,
    /// `library.filter` patch that clears just this filter.
    pub clear: serde_json::Value,
}

/// The chips for every constraint in the filter bar / search / sidebar (not the source itself).
pub fn filter_chips(f: &Filter, cat: &Catalog) -> Vec<FilterChip> {
    use lightcraft_catalog::{MediaKind, RatingOp};
    use serde_json::{Value::Null, json};
    let mut v = Vec::new();
    let mut add = |label: String, clear: serde_json::Value| v.push(FilterChip { label, clear });
    if !f.text.trim().is_empty() {
        add(format!("Search: “{}”", f.text.trim()), json!({"text": ""}));
    }
    if f.rating > 0 {
        let op = match f.rating_op {
            RatingOp::AtLeast => "≥",
            RatingOp::Exactly => "=",
            RatingOp::AtMost => "≤",
        };
        add(format!("Rating {op} {}★", f.rating), json!({"rating": 0}));
    }
    if let Some(fl) = f.flag {
        add(format!("Flag: {}", format!("{fl:?}").to_lowercase()), json!({"flag": Null}));
    }
    let mut labels = f.labels.clone();
    labels.extend(f.label.filter(|l| !labels.contains(l)));
    if !labels.is_empty() {
        let names: Vec<String> = labels.iter().map(|l| cat.label_name(*l)).collect();
        add(format!("Label: {}", names.join(" or ")), json!({"label": Null, "labels": []}));
    }
    if let Some(k) = f.kind {
        let n = match k {
            MediaKind::Image => "Photos",
            MediaKind::Raw => "Raw",
            MediaKind::Video => "Videos",
        };
        add(format!("Type: {n}"), json!({"kind": Null}));
    }
    if let Some(m) = &f.merged {
        let n = match m.as_str() {
            "hdr" => "HDR",
            "panorama" => "Panoramas",
            "hdrPanorama" => "HDR panoramas",
            _ => "Merged",
        };
        add(format!("Type: {n}"), json!({"merged": Null}));
    }
    if let Some(e) = f.edited {
        add(if e { "Edited".into() } else { "Unedited".into() }, json!({"edited": Null}));
    }
    for (name, key, val) in
        [("Camera", "camera", &f.camera), ("Lens", "lens", &f.lens), ("Keyword", "keyword", &f.keyword), ("Person", "person", &f.person)]
    {
        if let Some(x) = val {
            add(format!("{name}: {x}"), json!({key: Null}));
        }
    }
    if let Some(d) = &f.date {
        add(format!("Date: {}", date_label(d)), json!({"date": Null}));
    }
    if let Some(d) = f.library_folder.as_deref().filter(|d| !d.trim().is_empty()) {
        // the last two names, so two folders called "Pictures" are told apart
        let name = lightcraft_catalog::folders::folder_label(d);
        add(format!("Folder: {name}"), json!({"libraryFolder": Null}));
    }
    if let Some(d) = &f.imported {
        add(format!("Imported: {}", date_label(d)), json!({"imported": Null}));
    }
    match (&f.date_from, &f.date_to) {
        (Some(a), Some(b)) => add(format!("Captured {a} – {b}"), json!({"dateFrom": Null, "dateTo": Null})),
        (Some(a), None) => add(format!("Captured from {a}"), json!({"dateFrom": Null})),
        (None, Some(b)) => add(format!("Captured until {b}"), json!({"dateTo": Null})),
        _ => {}
    }
    if let Some(rs) = f.rule_set.as_ref().filter(|r| !r.rules.is_empty()) {
        add(format!("Rules: {}", rs.describe_with(cat)), json!({"ruleSet": Null}));
    }
    if !f.only.is_empty() {
        add(format!("Only {} photos", f.only.len()), json!({"only": []}));
    }
    v
}

/// `2026-01-16` → "January 16, 2026"; `2026-01` → "January 2026"; anything else as is.
fn date_label(d: &str) -> String {
    const MONTHS: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let parts: Vec<&str> = d.split('-').collect();
    let month = parts.get(1).and_then(|m| m.parse::<usize>().ok()).filter(|m| (1..=12).contains(m)).map(|m| MONTHS[m - 1]);
    match (parts.as_slice(), month) {
        ([y, _, day], Some(m)) => format!("{m} {}, {y}", day.trim_start_matches('0')),
        ([y, _], Some(m)) => format!("{m} {y}"),
        _ => d.to_string(),
    }
}

#[cfg(test)]
mod selection_state_tests {
    use super::*;
    use lightcraft_catalog::PhotoId;

    #[test]
    fn select_all_marks_the_others_selected_and_one_active() {
        let ids: Vec<PhotoId> = (1..=3).map(PhotoId).collect();
        let s = Selection { ids: ids.clone(), active: Some(ids[0]) };
        assert_eq!(s.state_of(ids[0]), SelectionState::Active);
        assert_eq!(s.state_of(ids[1]), SelectionState::Selected);
        assert_eq!(s.state_of(PhotoId(9)), SelectionState::NotSelected);
    }
}

/// The most photos an edit may touch for the view to follow it photo by photo; a larger edit is worked out whole.
const MAX_FOLLOWED: usize = 256;

/// Source of [`Session::visible_shared`] generations (process-wide, so two sessions never share one).
static VISIBLE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The grid order for the current source, filter and sort, cached by catalog revision, and the
/// view it was worked out from (so a small edit is followed instead of asking every photo again).
#[derive(Default)]
pub(crate) struct ViewCache {
    /// The grid order. Shared, so frontends can hold it across frames without copying ([`Session::visible_shared`]).
    pub(crate) visible: std::sync::Arc<[PhotoId]>,
    pub(crate) visible_key: Option<(u64, String)>,
    /// The view last worked out, the catalog's revision then, and the photos in it before stacks are arranged.
    /// A small edit is followed from it: only the photos the edit touched are asked about again and put at their
    /// place in the order ([`Session::visible`]). It is also what a held view holds (`library.holdView`).
    /// Another source, filter or sort is another view, and so is a smart album whose rules were changed.
    pub(crate) base: Option<(String, u64, Vec<PhotoId>)>,
    /// How many times the view followed an edit from the list it had, and how many times it was worked out from
    /// every photo (`ui.inspect` → `perf`): what tells a slow library from a slow edit.
    pub(crate) view_followed: u64,
    pub(crate) view_worked_out: u64,
    /// Tests: work the view out whole every time (what following has to equal).
    #[cfg(test)]
    pub(crate) never_follow: bool,
    /// Identifies the current `visible` list: a new value (unique in the process) every time it
    /// is recomputed, so frontends can key their per-view caches on it instead of hashing the ids.
    pub(crate) visible_gen: u64,
    /// Photos in the current source with no filter on (cached like `visible`).
    pub(crate) total: Option<((u64, String), usize)>,
    /// [`Session::selection_facts`] and what they were worked out for.
    facts: std::sync::Mutex<Option<(FactsKey, SelectionFacts)>>,
}

/// The library, the catalog's revision and the selection: what [`SelectionFacts`] depend on.
type FactsKey = (u64, u64, Vec<PhotoId>, Option<PhotoId>);

/// What menus and their commands' `enabled()` ask of the whole selection on every frame
/// ([`Session::selection_facts`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionFacts {
    /// A selected photo (or the active one) is in Recently Deleted.
    pub deleted: bool,
    /// A photo the selection acts on renders with an older process than the newest.
    pub outdated: bool,
    /// A photo the selection acts on is in a stack.
    pub stacked: bool,
}

impl Session {
    /// [`SelectionFacts`], asked of every selected photo once and then remembered until the
    /// catalog or the selection changes: menus ask on every frame, and a selection can be a whole
    /// library.
    pub fn selection_facts(&self) -> SelectionFacts {
        let sel = &self.selection;
        let mut held = self.view.facts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(((library, revision, ids, active), facts)) = held.as_ref()
            && *library == self.library_generation()
            && *revision == self.catalog.revision
            && *active == sel.active
            && *ids == sel.ids
        {
            return *facts;
        }
        let photo = |id: &PhotoId| self.catalog.photo(*id);
        let targets = if sel.ids.is_empty() { sel.active.as_slice() } else { sel.ids.as_slice() };
        let facts = SelectionFacts {
            deleted: sel.ids.iter().chain(&sel.active).any(|id| photo(id).is_some_and(|p| p.deleted)),
            outdated: targets.iter().any(|id| photo(id).is_some_and(|p| p.develop.process.is_outdated())),
            stacked: targets.iter().any(|id| self.catalog.stack_of(*id).is_some()),
        };
        *held = Some(((self.library_generation(), self.catalog.revision, sel.ids.clone(), sel.active), facts));
        facts
    }

    /// Photos shown in the grid/filmstrip for the current source, filter and sort.
    pub fn visible(&mut self) -> &[PhotoId] {
        let mut key =
            (self.catalog.revision, format!("{:?}|{:?}|{:?}|{:?}|{:?}", self.source, self.filter, self.sort, self.browse, self.library_folder));
        // What a held view is told apart by: what is shown and how, and for a smart album its rules (rules that were
        // changed ask something else). Not the checks of Missing Photos below: they fill the view in, they are
        // no other view.
        let asks = matches!(self.source, LibrarySource::Picks)
            || matches!(self.source, LibrarySource::Album(a) if self.catalog.album(a).is_some_and(|al| al.is_smart()));
        let rules = match self.source {
            LibrarySource::Album(a) if asks => self.catalog.album(a).map(|al| format!("{:?}", al.smart)).unwrap_or_default(),
            _ => String::new(),
        };
        let view = format!("{}|{rules}", key.1);
        if self.source == LibrarySource::Missing && self.media.availability.is_background() {
            // the view fills in as the background checks find files gone
            key.1.push_str(&format!("|{}", self.media.availability.generation()));
        }
        if self.view.visible_key.as_ref() != Some(&key) {
            // "in the last N days" rules count back from the session's clock
            lightcraft_catalog::rules::set_now(Some((self.clock)()));
            let f = self.view_filter(self.source, &self.filter);
            // A held view keeps the photos it shows that stopped matching what is asked of them (a smart album's
            // rules, a flag, the filter bar), as long as they are still where the view looks: in the library for
            // a view that is only a question, else in the album or folder shown. Taking a photo out of an album
            // or the library takes it out of the view at once, held or not.
            // (only a view that asks something can lose a photo to an edit: the others are queried as ever, at no
            // cost for the hold)
            let holds = self.prefs.hold_view && (asks || self.filter != Filter::default());
            let within = holds.then(|| {
                if asks {
                    self.view_filter(LibrarySource::All, &Filter::default())
                } else if self.source == LibrarySource::RecentlyAdded {
                    // (how far back it reaches was worked out for `f`: not a second pass over the library)
                    Filter { imported_from: f.imported_from.clone(), ..Default::default() }
                } else {
                    self.view_filter(self.source, &Filter::default())
                }
            });
            // Views whose order or photos are not the query's alone are worked out whole each time: an album in
            // its own order, Recently Added (how far back it reaches moves with every import), Missing Photos.
            let own_order = matches!(self.source, LibrarySource::Album(a) if self.sort.key == lightcraft_catalog::SortKey::CaptureDate
                && self.filter == Filter::default()
                && self.catalog.album(a).is_some_and(|al| !al.is_smart()));
            let whole = own_order || matches!(self.source, LibrarySource::RecentlyAdded | LibrarySource::Missing);
            // A view that asks about "the last N days" (itself, or through a smart album it looks into) changes
            // with the clock as well as with edits: a photo nobody touched can stop matching. It is asked whole.
            let clocked = f.depends_on_now()
                || (!f.albums_tested().is_empty() && self.catalog.albums().any(|al| al.smart.as_deref().is_some_and(Filter::depends_on_now)));
            let base = self.view.base.take().filter(|(of, _, _)| *of == view);
            // A small edit to the same view: the photos it touched are taken out, asked about again, and put back
            // at their place in the order. The others neither stopped matching nor moved.
            #[cfg(test)]
            let clocked = clocked || self.view.never_follow;
            let followed = base.as_ref().filter(|_| !whole && !clocked).and_then(|(_, revision, ids)| {
                let touched = self.catalog.touched_since(*revision).filter(|touched| touched.len() <= MAX_FOLLOWED)?;
                Some(self.catalog.follow(ids, &touched, &f, &self.sort, within.as_ref()))
            });
            match followed {
                Some(_) => self.view.view_followed += 1,
                None => self.view.view_worked_out += 1,
            }
            let mut visible = match (followed, &base, &within) {
                (Some(ids), _, _) => ids,
                (None, Some((_, _, held)), Some(within)) => {
                    let keep: std::collections::HashSet<PhotoId> = held.iter().copied().collect();
                    self.catalog.query_keeping(&f, &self.sort, &keep, within)
                }
                _ => self.catalog.query(&f, &self.sort),
            };
            // what the view shows now is what the next edit is followed from, and what it holds until it is
            // refreshed (taken before stacks are arranged, so a photo folded into a stack is held with it)
            self.view.base = (!whole || holds).then(|| (view, self.catalog.revision, visible.clone()));
            if visible.is_empty() && self.source == LibrarySource::LibraryFolder && !self.folder_holds_photos() {
                // the shown folder lost its last photo (deleted, moved, removed): everything, not
                // an empty grid under the name of a folder that is gone from the sidebar
                self.source = LibrarySource::All;
                self.library_folder = None;
                return self.visible();
            }
            if matches!(self.source, LibrarySource::Album(_))
                && self.sort.key == lightcraft_catalog::SortKey::CaptureDate
                && let LibrarySource::Album(a) = self.source
                && let Some(al) = self.catalog.album(a)
                && !al.is_smart()
                && self.filter == Filter::default()
            {
                // each photo's place looked up, not searched for (a big album sorted in O(n log n))
                let mut place: std::collections::HashMap<PhotoId, usize> = std::collections::HashMap::with_capacity(al.photos.len());
                for (i, id) in al.photos.iter().enumerate() {
                    place.entry(*id).or_insert(i);
                }
                visible.sort_by_key(|id| place.get(id).copied().unwrap_or(usize::MAX));
                if !self.sort.ascending {
                    visible.reverse();
                }
            }
            if self.source == LibrarySource::RecentlyAdded {
                // newest import first, whatever the sort direction (the grid groups by import day);
                // a batch stays in file order (name, then id)
                let cat = &self.catalog;
                visible.sort_by(|a, b| {
                    let key = |id: &PhotoId| cat.photo(*id).map(|p| p.imported.clone()).unwrap_or_default();
                    let name = |id: &PhotoId| cat.photo(*id).map(|p| p.file_name.as_str());
                    let by_name = match (name(a), name(b)) {
                        (Some(x), Some(y)) => lightcraft_catalog::query::cmp_name(x, y),
                        _ => std::cmp::Ordering::Equal,
                    };
                    key(b).cmp(&key(a)).then(by_name).then(a.cmp(b))
                });
            }
            if self.source == LibrarySource::Missing {
                // only the photos the query kept (library photos, not Local browse records) are checked
                let (cat, avail) = (&self.catalog, &self.media.availability);
                visible.retain(|id| cmd::missing::is_missing(cat, avail, *id));
            }
            if self.source != LibrarySource::RecentlyDeleted {
                visible = self.catalog.arrange_stacks(&visible);
            }
            self.view.visible = visible.into();
            self.view.visible_gen = VISIBLE_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.view.visible_key = Some(key);
        }
        &self.view.visible
    }

    /// `source` as a filter over `base` (the filter bar's, or none), with the folder it shows.
    fn view_filter(&self, source: LibrarySource, base: &Filter) -> Filter {
        let mut f = source.to_filter(base, &self.catalog);
        if source == LibrarySource::Folder {
            // no folder chosen: nothing (an empty path matches nothing)
            let b = self.browse.clone().unwrap_or_default();
            f.folder = Some(b.path);
            f.subfolders = b.subfolders;
        }
        if source == LibrarySource::LibraryFolder {
            // no folder chosen: nothing (`.` names no folder)
            f.library_folder = Some(self.library_folder.clone().unwrap_or_else(|| ".".into()));
        }
        f
    }

    /// Let go of what a held view kept: the view shows what matches now ([`Session::hold_view`]).
    pub fn refresh_view(&mut self) {
        self.view.base = None;
        self.view.visible_key = None;
    }

    /// [`Session::visible`] worked out from every photo, whatever was followed before: what following an edit
    /// has to equal.
    /// Nothing the session keeps is changed by asking: the next edit is followed from where the last one left the
    /// view, so a list followed many times over is compared, not only one followed once.
    #[cfg(test)]
    pub(crate) fn visible_in_full(&mut self) -> Vec<PhotoId> {
        let kept = (
            self.view.base.clone(),
            self.view.visible_key.clone(),
            self.view.visible.clone(),
            self.view.visible_gen,
            self.view.view_followed,
            self.view.view_worked_out,
        );
        self.view.never_follow = true;
        self.view.visible_key = None;
        let whole = self.visible().to_vec();
        self.view.never_follow = false;
        (self.view.base, self.view.visible_key, self.view.visible, self.view.visible_gen, self.view.view_followed, self.view.view_worked_out) = kept;
        whole
    }

    /// Whether the library still holds a photo imported from the folder a `LibraryFolder` source
    /// shows.
    fn folder_holds_photos(&self) -> bool {
        let f = Filter { library_folder: self.library_folder.clone(), ..Default::default() };
        self.library_folder.is_some() && !self.catalog.query(&f, &Sort::default()).is_empty()
    }

    /// Photos in the current source (folder, album, …) before the filter bar and search narrow
    /// them; `None` where that is not a separate number (Missing Photos).
    pub fn source_total(&mut self) -> Option<usize> {
        if self.source == LibrarySource::Missing {
            return None;
        }
        if self.filter == Filter::default() && !self.prefs.hold_view {
            // nothing narrows the source: what the view lists is all of it, and is counted already (not a view
            // that holds photos that stopped matching: those are not of the source any more)
            let _ = self.visible();
            if let Some((_, revision, ids)) = &self.view.base
                && *revision == self.catalog.revision
            {
                return Some(ids.len());
            }
        }
        let key = (self.catalog.revision, format!("{:?}|{:?}|{:?}", self.source, self.browse, self.library_folder));
        if self.view.total.as_ref().map(|t| &t.0) != Some(&key) {
            let mut f = self.source.to_filter(&Filter::default(), &self.catalog);
            if self.source == LibrarySource::Folder {
                let b = self.browse.clone().unwrap_or_default();
                f.folder = Some(b.path);
                f.subfolders = b.subfolders;
            }
            if self.source == LibrarySource::LibraryFolder {
                f.library_folder = Some(self.library_folder.clone().unwrap_or_else(|| ".".into()));
            }
            // (how many, not which: nothing to put in order)
            let n = self.catalog.count(&f);
            self.view.total = Some((key, n));
        }
        self.view.total.as_ref().map(|t| t.1)
    }

    pub fn visible_cloned(&mut self) -> Vec<PhotoId> {
        self.visible().to_vec()
    }

    /// After the source changes, the active photo follows the rule every source shares: it stays
    /// when the view shows it, else the view's first photo becomes the active (and only selected)
    /// one, or nothing when the view is empty. A view never keeps an active photo it doesn't show
    /// (issue #1447: a Local folder kept a library photo active, so Detail and the info showed a
    /// photo the grid didn't).
    pub fn keep_active_in_view(&mut self) {
        let active = self.selection.active;
        let vis = self.visible();
        if active.is_some_and(|a| vis.contains(&a)) {
            return;
        }
        let first = vis.first().copied();
        self.selection = first.map(Selection::single).unwrap_or_default();
    }

    /// The visible photos without copying them, with their generation: equal generations mean
    /// the very same list (same catalog revision, source, filter, sort), so a frontend can cache
    /// whatever it derives from the list under that number.
    pub fn visible_shared(&mut self) -> (u64, Arc<[PhotoId]>) {
        self.visible();
        (self.view.visible_gen, self.view.visible.clone())
    }

    /// How many times the view followed an edit, and how many times it was worked out from every
    /// photo (`ui.inspect` → `perf`).
    pub fn view_counts(&self) -> (u64, u64) {
        (self.view.view_followed, self.view.view_worked_out)
    }
}
