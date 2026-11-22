//! Stable handles for entities owned by a scene.

use serde::Serialize;

/// Opaque, monotonically allocated handle scoped to the owning scene.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct EntityId(pub(crate) u64);
