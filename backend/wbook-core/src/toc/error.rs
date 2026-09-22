use specta::Type;
use thiserror::Error;

use super::NodeId;

#[derive(Error, Debug, Type)]
pub enum TocError {
    #[error("the parent id: `{0}` is not exist in container")]
    NodeParentNotFound(NodeId),

    #[error("invalid toc snapshot: {0}")]
    InvalidSnapshot(String),

    #[error(transparent)]
    // anyhow::Error is internal-only and has no specta/serde representation.
    #[specta(skip)]
    Other(#[from] anyhow::Error),
}
