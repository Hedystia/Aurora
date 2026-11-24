//! Stable handles for entities owned by a scene.

use serde::Serialize;

/// Opaque, monotonically allocated handle scoped to the owning scene.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct EntityId(pub(crate) u64);

impl EntityId {
    /// Returns the opaque runtime handle as an integer for display and serialization.
    pub fn value(self) -> u64 {
        self.0
    }
}
