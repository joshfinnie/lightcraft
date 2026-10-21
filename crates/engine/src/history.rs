//! Undo and redo: every change is an op committed as one step, with its inverse kept to undo it.

use std::sync::Arc;

use lightcraft_catalog::{Op, PhotoId};
use lightcraft_develop::DevelopSettings;

use crate::{AUTO_VERSIONS, EngineError, Result, Selection, Session, cmd, rename};

/// Undo and redo, the slider drag in progress and the ops still to log.
#[derive(Default)]
pub struct History {
    pub undo: Vec<UndoEntry>,
    /// Undo steps committed in this session, ever (the history drops its oldest past 1000, so
    /// its length can't tell how many steps a command just made; see [`Session::commits`]).
    pub(crate) commits: u64,
    pub redo: Vec<UndoEntry>,
    /// Exact filesystem repairs from discarded history; catalog inverses stay discarded.
    pub(crate) pending_sidecar_repairs: Vec<rename::SidecarMove>,
    pub interaction: Option<Interaction>,
    /// Ops applied since the last `drain_log` (for persistence).
    pub(crate) pending_log: Vec<Op>,
}

/// One undo step: the inverse op and a label.
#[derive(Clone, Debug)]
pub struct UndoEntry {
    pub label: String,
    pub op: Op,
    /// A folder to rename on disk (`from` → `to`) before `op` is applied: Rename / Move Folder
    /// (whose `op` relinks the photos inside). Never overwrites; refused when `to` exists. An
    /// empty `to` removes the (empty) folder `from`, an empty `from` creates `to` (Create Folder).
    pub folder: Option<FolderMove>,
    /// Inverses of logged path corrections from a partially failed history step.
    pub(crate) retry_inverse: Vec<Op>,
    /// Sidecars still separated from their photo after a failed move or rollback.
    pub(crate) retry_sidecars: Vec<rename::SidecarMove>,
}

/// The one photo an undo step changes, when it changes exactly one through its edit, rating, flag,
/// label, metadata or versions (`depth` bounds nested batches).
fn single_photo(op: &Op, depth: usize) -> Option<PhotoId> {
    match op {
        Op::SetDevelop { id, .. }
        | Op::SetRating { id, .. }
        | Op::SetFlag { id, .. }
        | Op::SetLabel { id, .. }
        | Op::SetMeta { id, .. }
        | Op::SetVersions { id, .. }
        | Op::SetHistory { id, .. }
        | Op::PushHistory { id, .. }
        | Op::SetHistoryCursor { id, .. } => Some(*id),
        Op::Batch { ops } if depth < 8 => {
            let mut ids = ops.iter().map(|o| single_photo(o, depth + 1));
            let first = ids.next()??;
            ids.all(|id| id == Some(first)).then_some(first)
        }
        _ => None,
    }
}

/// A folder renamed or moved on disk as part of an undo step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderMove {
    pub from: String,
    pub to: String,
}

impl FolderMove {
    fn reversed(&self) -> Self {
        Self { from: self.to.clone(), to: self.from.clone() }
    }
}

/// An in-progress slider drag / brush stroke: one undo step when it ends.
#[derive(Clone, Debug)]
pub struct Interaction {
    pub label: String,
    pub photo: PhotoId,
    pub original: Arc<DevelopSettings>,
}

impl Session {
    /// Apply an op as one undoable step.
    pub fn commit(&mut self, label: &str, op: Op) -> Result<()> {
        let fwd = op.clone();
        let inv = self.apply_catalog(op)?;
        self.history.pending_log.push(fwd);
        self.history.undo.push(UndoEntry { label: label.to_string(), op: inv, folder: None, retry_inverse: Vec::new(), retry_sidecars: Vec::new() });
        self.history.commits = self.history.commits.wrapping_add(1);
        if self.history.undo.len() > 1000 {
            let mut dropped = self.history.undo.remove(0);
            self.history.pending_sidecar_repairs.append(&mut dropped.retry_sidecars);
        }
        for mut dropped in self.history.redo.drain(..) {
            self.history.pending_sidecar_repairs.append(&mut dropped.retry_sidecars);
        }
        Ok(())
    }

    pub(crate) fn apply_catalog(&mut self, op: Op) -> Result<Op> {
        // Retain only cache IDs while the catalog consumes the operation. Cloning an import
        // batch here also cloned every photo, although commit already keeps the journal copy.
        let mut invalidated = Vec::new();
        // photos whose file was replaced: the faces made from the old pixels go too (issue #1524)
        let mut replaced = Vec::new();
        let mut pending = vec![&op];
        while let Some(op) = pending.pop() {
            match op {
                Op::SetContent { id, .. } => {
                    invalidated.push(*id);
                    replaced.push(*id);
                }
                Op::SetFile { id, .. } | Op::Relink { id, .. } | Op::RemovePhoto { id } => invalidated.push(*id),
                Op::AddPhoto { photo } => invalidated.push(photo.id),
                Op::Batch { ops } => pending.extend(ops),
                _ => {}
            }
        }
        let inv = self.catalog.apply(op)?;
        for id in invalidated {
            self.media.forget(id);
        }
        #[cfg(not(target_arch = "wasm32"))]
        for id in replaced {
            self.faces.content_replaced(id);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = replaced;
        Ok(inv)
    }

    /// [`Session::commit`] for an op that goes with a folder already renamed on disk; the undo
    /// step renames it back (`folder` is the undo direction: current place → old place).
    pub(crate) fn commit_with_folder(&mut self, label: &str, op: Op, folder: FolderMove) -> Result<()> {
        self.commit(label, op)?;
        if let Some(e) = self.history.undo.last_mut() {
            e.folder = Some(folder);
        }
        Ok(())
    }

    /// Apply an op that is LightCraft's own bookkeeping rather than something the user did (faces found by the background
    /// scan, a raw's as-shot white read by a decode, raws rechecked after an update): journaled like any op, but not
    /// an undo step, and it leaves the redo stack alone.
    pub(crate) fn apply_system(&mut self, op: Op) -> Result<()> {
        let fwd = op.clone();
        self.catalog.apply(op)?;
        self.history.pending_log.push(fwd);
        Ok(())
    }

    /// Leaving photo `id` after editing it: keep its settings as an automatic version (when they
    /// differ from its latest version; at most [`AUTO_VERSIONS`] auto versions, oldest dropped).
    /// Saved with the library but not an undo step.
    pub fn auto_version(&mut self, id: PhotoId) {
        let Some(p) = self.catalog.photo(id) else { return };
        if !p.is_edited() || p.versions.last().is_some_and(|v| *v.settings == *p.develop) || self.history.interaction.is_some() {
            return;
        }
        let mut versions = p.versions.clone();
        let created = (self.clock)();
        let name = lightcraft_catalog::dates::display_time(&created);
        versions.push(lightcraft_catalog::Version { name, created, settings: p.develop.clone(), auto: true });
        let autos = versions.iter().filter(|v| v.auto).count();
        if autos > AUTO_VERSIONS
            && let Some(i) = versions.iter().position(|v| v.auto)
        {
            versions.remove(i);
        }
        let op = Op::SetVersions { id, versions };
        if self.catalog.apply(op.clone()).is_ok() {
            self.history.pending_log.push(op);
        }
    }

    /// How many undo steps were committed in this session so far: what a command that commits
    /// several steps compares before and after to merge them ([`Session::merge_undo`]).
    pub fn commits(&self) -> u64 {
        self.history.commits
    }

    /// Fold the last `n` undo steps into one (commands that commit step by step because each op
    /// depends on the state the previous one left).
    pub fn merge_undo(&mut self, n: usize, label: &str) {
        if n < 2 || n > self.history.undo.len() {
            return;
        }
        let Some(entries) = self.history.undo.get(self.history.undo.len() - n..) else { return };
        // Folder moves and partially applied file steps stay on their own.
        if entries.iter().any(|e| e.folder.is_some() || !e.retry_inverse.is_empty() || !e.retry_sidecars.is_empty()) {
            return;
        }
        let tail = self.history.undo.split_off(self.history.undo.len() - n);
        let ops = tail.into_iter().rev().map(|e| e.op).collect();
        self.history.undo.push(UndoEntry {
            label: label.to_string(),
            op: Op::Batch { ops },
            folder: None,
            retry_inverse: Vec::new(),
            retry_sidecars: Vec::new(),
        });
    }

    /// Apply without recording undo (interactive previews).
    pub(crate) fn apply_silent(&mut self, op: Op) -> Result<()> {
        self.catalog.apply(op)?;
        Ok(())
    }

    pub fn undo_step(&mut self) -> Result<String> {
        self.undo_step_with(&rename::RealFs)
    }

    pub(crate) fn undo_step_with(&mut self, fs: &dyn rename::MoveFs) -> Result<String> {
        let mut e = self.history.undo.pop().ok_or_else(|| EngineError::Other("nothing to undo".into()))?;
        let redo = match self.apply_with_files(&mut e, fs) {
            Ok(r) => r,
            Err(err) => {
                self.history.undo.push(e);
                return Err(err);
            }
        };
        self.show_undone(&e.op);
        self.history.pending_log.push(e.op);
        self.history.redo.push(UndoEntry {
            label: e.label.clone(),
            op: redo,
            folder: e.folder.as_ref().map(FolderMove::reversed),
            retry_inverse: Vec::new(),
            retry_sidecars: Vec::new(),
        });
        Ok(e.label)
    }

    pub fn redo_step(&mut self) -> Result<String> {
        self.redo_step_with(&rename::RealFs)
    }

    pub(crate) fn redo_step_with(&mut self, fs: &dyn rename::MoveFs) -> Result<String> {
        let mut e = self.history.redo.pop().ok_or_else(|| EngineError::Other("nothing to redo".into()))?;
        let undo = match self.apply_with_files(&mut e, fs) {
            Ok(r) => r,
            Err(err) => {
                self.history.redo.push(e);
                return Err(err);
            }
        };
        self.show_undone(&e.op);
        self.history.pending_log.push(e.op);
        self.history.undo.push(UndoEntry {
            label: e.label.clone(),
            op: undo,
            folder: e.folder.as_ref().map(FolderMove::reversed),
            retry_inverse: Vec::new(),
            retry_sidecars: Vec::new(),
        });
        Ok(e.label)
    }

    /// After an undo or redo that changed one photo (its edit, rating, flag, label, metadata or
    /// versions), that photo becomes the active one, as in Lightroom Classic, so the change is on
    /// screen (issue #293). It joins the selection if it isn't in it; nothing changes for steps
    /// that touch several photos, albums or files, or for a photo not in the current view.
    fn show_undone(&mut self, op: &Op) {
        let Some(id) = single_photo(op, 0) else { return };
        if self.selection.active == Some(id) || !self.visible().contains(&id) {
            return;
        }
        if !self.selection.contains(id) {
            self.selection = Selection { ids: vec![id], active: Some(id) };
        } else {
            self.selection.active = Some(id);
        }
        self.active_mask = None;
        self.active_spot = None;
    }

    /// Apply an undo/redo op, first renaming the step's folder and moving the files its renames
    /// imply (all or nothing). Files that can't be moved back after a failure are reported and the
    /// library follows them (a logged change outside the undo history), so none goes missing; the
    /// folder is only moved back when no file was left behind at its new path.
    pub(crate) fn apply_with_files(&mut self, entry: &mut UndoEntry, fs: &dyn rename::MoveFs) -> Result<Op> {
        // The entry is detached from self.undo/self.redo: include it before any global
        // repair can retire, then retry revalidates it before moving its own packets.
        rename::validate_sidecars(fs, &entry.retry_sidecars).map_err(|e| EngineError::Other(format!("can't move the files back: {e}")))?;
        // Repair discarded file steps before another path change can stale their exact paths.
        // Unrelated metadata history must remain usable while a sidecar is still unwritable.
        if entry.folder.is_some() || !entry.retry_sidecars.is_empty() || !self.file_moves(&entry.op).is_empty() {
            self.repair_sidecars_with(fs).map_err(|e| EngineError::Other(format!("can't move the files back: {e}")))?;
        }
        rename::retry_sidecars(fs, &mut entry.retry_sidecars).map_err(|e| EngineError::Other(format!("can't move the files back: {e}")))?;
        let op = &entry.op;
        let folder = entry.folder.as_ref();
        if let Some(f) = folder {
            cmd::browse::folder_on_disk(&f.from, &f.to).map_err(|e| EngineError::Other(format!("can't move the folder back: {e}")))?;
        }
        let undo_folder = || {
            if let Some(f) = folder {
                let _ = cmd::browse::folder_on_disk(&f.to, &f.from);
            }
        };
        let moves = self.file_moves(op);
        if let Err(mut e) = rename::move_all(fs, &moves) {
            entry.retry_sidecars.append(&mut e.pending_sidecars);
            if e.stuck.is_empty() {
                undo_folder();
            }
            let (message, mut inverses) = self.follow_stuck(op, e, false);
            // A later successful retry observes the corrected paths. Retain their old paths
            // now, so the opposite history direction still restores the entire original step.
            inverses.append(&mut entry.retry_inverse);
            entry.retry_inverse = inverses;
            return Err(EngineError::Other(format!("can't move the files back: {message}")));
        }
        match self.apply_catalog(op.clone()) {
            Ok(inv) => {
                if let Some(f) = folder {
                    cmd::browse::follow_folder(self, &f.from, &f.to);
                }
                if entry.retry_inverse.is_empty() {
                    Ok(inv)
                } else {
                    let mut ops = match inv {
                        Op::Batch { ops } => ops,
                        op => vec![op],
                    };
                    ops.append(&mut entry.retry_inverse);
                    Ok(Op::Batch { ops })
                }
            }
            Err(e) => {
                let back: Vec<(String, String)> = moves.iter().rev().map(|(a, b)| (b.clone(), a.clone())).collect();
                if let Err(mut be) = rename::move_all(fs, &back) {
                    entry.retry_sidecars.append(&mut be.pending_sidecars);
                    return Err(EngineError::Other(format!("{e}; the files could not all be moved back: {}", be.message)));
                }
                undo_folder();
                Err(e)
            }
        }
    }

    /// Ops applied since the last call, for the op-log store.
    pub fn drain_log(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.history.pending_log)
    }
}
