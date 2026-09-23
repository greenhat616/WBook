use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use crate::extractor::{Content, ParsedContent};
use crate::parser::{check_cancelled, MatchConfidence, MetadataParser, ParserError};

#[cfg(test)]
mod tests;

/// Only the head of the text is scanned for metadata.
const HEAD_CHARS: usize = 1000;

static BOOK_TITLE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"《([^》]+)》").unwrap());
static AUTHOR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*作者\s*[:：]\s*(.+?)\s*$").unwrap());
static TITLE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*书名\s*[:：]\s*(.+?)\s*$").unwrap());

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Heuristic metadata extraction from the head of the text:
/// `《title》` brackets first, then `书名:`/`作者:` label lines, and finally
/// the source file name as the title fallback.
pub struct SimpleMetadataParser;

impl SimpleMetadataParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimpleMetadataParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataParser for SimpleMetadataParser {
    fn name(&self) -> &'static str {
        "simple-metadata"
    }

    /// Any plain text may carry metadata, so always accept at low confidence.
    fn accept(&self, _content: &ParsedContent) -> MatchConfidence {
        MatchConfidence(10)
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<Metadata, ParserError> {
        check_cancelled(ct)?;
        let Content::Text(text) = &content.content;
        // Slice on a char boundary: the head may end mid-codepoint otherwise.
        let head_end = text
            .char_indices()
            .nth(HEAD_CHARS)
            .map(|(idx, _)| idx)
            .unwrap_or(text.len());
        let head = &text[..head_end];

        let mut metadata = Metadata::default();
        if let Some(caps) = BOOK_TITLE_PATTERN.captures(head) {
            metadata.title = Some(caps[1].trim().to_string());
        }
        if let Some(caps) = AUTHOR_PATTERN.captures(head) {
            metadata.author = Some(caps[1].trim().to_string());
        }
        if metadata.title.is_none() {
            if let Some(caps) = TITLE_PATTERN.captures(head) {
                metadata.title = Some(caps[1].trim().to_string());
            }
        }
        if metadata.title.is_none() {
            metadata.title = content
                .source_path
                .as_ref()
                .and_then(|path| path.file_stem())
                .map(|stem| stem.to_string());
        }
        check_cancelled(ct)?;
        Ok(metadata)
    }
}
