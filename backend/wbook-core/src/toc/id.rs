use serde::{Deserialize, Serialize};
use specta::Type;

/// Strongly typed id of a TOC node. Internally it is a slab key, so
/// serialization is free (`serde(transparent)`).
///
/// Caveat: slab reuses the keys of removed nodes, so a stale `NodeId` held
/// after a `remove` may alias a newly inserted node. If that ever becomes a
/// problem, switch the container to `generational-arena`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(transparent)]
pub struct NodeId(pub usize);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
