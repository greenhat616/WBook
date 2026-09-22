//! A Extractor should run in a separate thread to make the processing non-blocking and safe to unwind or catch the exceptions across ffi boundaries.

use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

mod simple;

#[derive(Debug, thiserror::Error)]
pub enum ExtractorError {
    #[error("The task is shutting down")]
    Shutdown,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Encoding {
    pub name: String,
    pub bom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParsedContent {
    pub encoding: Encoding,
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Content {
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProcessOptions {}

pub trait Extractor {
    /// Whether this Extractor is able to handle the given file path.
    fn accept(&self, path: &Utf8Path) -> bool;

    /// Process the given file path.
    ///
    /// Returns an error if the processing fails.
    fn process(
        &self,
        ct: &CancellationToken,
        opt: &ProcessOptions,
        path: &Utf8Path,
    ) -> Result<ParsedContent, ExtractorError>;
}
