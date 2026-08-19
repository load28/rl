//! A snapshot — the project at one moment, immutable.
//!
//! Every semantic request runs against a snapshot, so an answer is always
//! about one consistent state of the project: the file set, each file's
//! text, and each file's projection, as they were when the snapshot was
//! taken. A later edit produces a *new* snapshot (see
//! [`super::Project::update`]); it never changes this one. That is how
//! stale-answer bugs are ruled out by structure rather than by care.

use std::sync::Arc;

use super::projection::ProjectedDocument;

/// The project at one moment. Cheap to hold: files are shared with the
/// project's cache, so an unchanged file costs one reference.
#[derive(Debug)]
pub struct Snapshot {
    pub(crate) id: u64,
    pub(crate) files: Vec<Arc<ProjectedDocument>>,
}

impl Snapshot {
    /// This snapshot's sequence number within its project. Monotonic: a
    /// later snapshot of the same project has a larger id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The project's `.rl` files, projected, in project order.
    pub fn files(&self) -> &[Arc<ProjectedDocument>] {
        &self.files
    }
}
