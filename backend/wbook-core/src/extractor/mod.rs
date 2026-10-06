//! A Extractor should run in a separate thread to make the processing non-blocking and safe to unwind or catch the exceptions across ffi boundaries.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

mod simple;

pub use simple::SimpleExtractor;

#[derive(Debug, snafu::Snafu)]
pub enum ExtractorError {
    #[snafu(display("The task is shutting down"))]
    Shutdown,
    #[snafu(context(false), display("{source}"))]
    Io { source: std::io::Error },
    #[snafu(context(false), display("{source}"))]
    Other { source: anyhow::Error },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct Encoding {
    pub name: String,
    pub bom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct ParsedContent {
    pub encoding: Encoding,
    pub content: Content,
    /// Path of the file this content was extracted from, when known
    /// (metadata parsers use it as a title fallback).
    pub source_path: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
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
