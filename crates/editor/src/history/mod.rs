//! Undo and redo for scene edits.
//!
//! Every edit already bumps [`Project::revision`]. The history snapshots the
//! document once the pointer is released and no text field has focus, so a
//! whole gizmo drag or a typed value becomes a single undo step.

use std::path::PathBuf;

use engine_core::save::SceneFile;

use crate::project::Project;

/// Maximum number of undo steps kept per document.
const LIMIT: usize = 100;

#[derive(Clone)]
struct Snapshot {
    document: SceneFile,
    selected: Option<usize>,
}

/// Undo and redo stacks for the open document.
#[derive(Default)]
pub(super) struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    committed: Option<Snapshot>,
    path: PathBuf,
    revision: u64,
}

impl History {
    fn snapshot(project: &Project) -> Snapshot {
        Snapshot {
            document: project.document.clone(),
            selected: project.selected,
        }
    }

    fn reset(&mut self, project: &Project) {
        self.undo.clear();
        self.redo.clear();
        self.committed = Some(Self::snapshot(project));
        self.path = project.path.clone();
        self.revision = project.revision;
    }

    /// Records settled edits. `busy` is true while a drag or text edit is in progress.
    pub(super) fn track(&mut self, project: &Project, busy: bool) {
        if self.committed.is_none() || self.path != project.path {
            self.reset(project);
            return;
        }
        if busy || self.revision == project.revision {
            return;
        }
        self.commit(project);
    }

    fn commit(&mut self, project: &Project) {
        if self.revision == project.revision {
            return;
        }
        if let Some(previous) = self.committed.replace(Self::snapshot(project)) {
            self.undo.push(previous);
            if self.undo.len() > LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.revision = project.revision;
    }

    /// True when there is a step to undo.
    pub(super) fn can_undo(&self, project: &Project) -> bool {
        !self.undo.is_empty() || self.revision != project.revision
    }

    /// True when there is a step to redo.
    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Restores the previous document state.
    pub(super) fn undo(&mut self, project: &mut Project) {
        if self.path != project.path {
            self.reset(project);
            return;
        }
        self.commit(project);
        let Some(previous) = self.undo.pop() else {
            return;
        };
        if let Some(current) = self.committed.replace(previous.clone()) {
            self.redo.push(current);
        }
        self.restore(project, previous);
    }

    /// Re-applies the last undone state.
    pub(super) fn redo(&mut self, project: &mut Project) {
        if self.path != project.path || self.revision != project.revision {
            return;
        }
        let Some(next) = self.redo.pop() else {
            return;
        };
        if let Some(current) = self.committed.replace(next.clone()) {
            self.undo.push(current);
        }
        self.restore(project, next);
    }

    fn restore(&mut self, project: &mut Project, snapshot: Snapshot) {
        let count = snapshot.document.nodes.len();
        project.document = snapshot.document;
        project.selected = snapshot.selected.filter(|index| *index < count);
        project.group_selection.retain(|index| *index < count);
        project.changed();
        self.revision = project.revision;
    }
}
