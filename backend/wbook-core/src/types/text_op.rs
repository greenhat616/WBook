use serde::{Deserialize, Serialize};
use specta::Type;

use super::TextRange;

/// A single text-edit operator, expressed purely as byte offsets into the
/// decoded text. Per DESIGN_NOTE the source text stays immutable: operators
/// only describe edits and are materialized lazily at split/export time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TextOp {
    Replace { range: TextRange, replacement: String },
    Delete { range: TextRange },
    Insert { range: TextRange, insertion: String },
}
