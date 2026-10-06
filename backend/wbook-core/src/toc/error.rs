use snafu::Snafu;
use specta::Type;

use super::NodeId;

#[derive(Snafu, Debug, Type)]
pub enum TocError {
    #[snafu(display("the parent id: `{node_id}` is not exist in container"))]
    NodeParentNotFound { node_id: NodeId },

    #[snafu(display("invalid toc snapshot: {message}"))]
    InvalidSnapshot { message: String },

    #[snafu(context(false), display("{source}"))]
    // anyhow::Error is internal-only and has no specta/serde representation.
    #[specta(skip)]
    Other { source: anyhow::Error },
}
