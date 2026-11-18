//! The last snapshot the core emitted, shared between the actor and the Tauri side.

use std::sync::{Arc, Mutex, PoisonError};

use protocol::Snapshot;

#[derive(Clone, Default)]
pub(crate) struct SnapshotCache(Arc<Mutex<Snapshot>>);

impl SnapshotCache {
    pub(crate) fn get(&self) -> Snapshot {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set(&self, snapshot: Snapshot) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = snapshot;
    }

    /// The shared cell, for the host's sweep of unused data stores.
    pub(crate) fn shared(&self) -> Arc<Mutex<Snapshot>> {
        self.0.clone()
    }
}
