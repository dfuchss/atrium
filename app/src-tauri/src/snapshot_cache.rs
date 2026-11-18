//! The last snapshot the core emitted, shared between the actor and the Tauri side.

use std::sync::{Arc, Mutex};

use fs_util::lock_ignoring_poison;
use protocol::Snapshot;

#[derive(Clone, Default)]
pub(crate) struct SnapshotCache(Arc<Mutex<Snapshot>>);

impl SnapshotCache {
    pub(crate) fn get(&self) -> Snapshot {
        lock_ignoring_poison(&self.0).clone()
    }

    pub(crate) fn set(&self, snapshot: Snapshot) {
        *lock_ignoring_poison(&self.0) = snapshot;
    }

    /// The shared cell, for the host's sweep of unused data stores.
    pub(crate) fn shared(&self) -> Arc<Mutex<Snapshot>> {
        self.0.clone()
    }
}
