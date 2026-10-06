use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use crate::document::TextView;
use crate::parser::{check_cancelled, MatchConfidence, MetadataParser, ParserError};

#[cfg(test)]
mod tests;

/// Only the head of the text is scanned for metadata.
const HEAD_CHARS: usize = 1000;

/// Inline `《…》` in prose usually cites another work, so only a line holding
/// nothing but the brackets counts as the title.
static TITLE_LINE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*《([^》]+)》\s*$").unwrap());
static BOOK_TITLE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"《([^》]+)》").unwrap());
static AUTHOR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*作者\s*[:：]\s*(.+?)\s*$").unwrap());
static TITLE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*书名\s*[:：]\s*(.+?)\s*$").unwrap());
/// Download sites decorate file names with tags such as `[搜书吧]`, so the
/// author name ends at the first bracket or separator.
static STEM_AUTHOR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"作者\s*[:：]\s*([^\s\[\]【】()（）《》<>_]+)").unwrap());

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Heuristic metadata extraction. The head of the text wins over the file
/// name; within the file name, `《title》` and `作者：name` are preferred over
/// the bare stem.
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
    fn accept(&self, _content: TextView<'_>) -> MatchConfidence {
        MatchConfidence(10)
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: TextView<'_>,
    ) -> Result<Metadata, ParserError> {
        check_cancelled(ct)?;
        let text = content;
        let head = text.prefix(ct, HEAD_CHARS)?;

        let stem = content.source_path().and_then(|path| path.file_stem());
        let capture = |pattern: &Regex, haystack: &str| {
            pattern
                .captures(haystack)
                .map(|caps| caps[1].trim().to_string())
        };

        let metadata = Metadata {
            title: capture(&TITLE_LINE_PATTERN, &head)
                .or_else(|| capture(&TITLE_PATTERN, &head))
                .or_else(|| stem.and_then(|stem| capture(&BOOK_TITLE_PATTERN, stem)))
                .or_else(|| stem.map(str::to_string)),
            author: capture(&AUTHOR_PATTERN, &head)
                .or_else(|| stem.and_then(|stem| capture(&STEM_AUTHOR_PATTERN, stem))),
        };
        check_cancelled(ct)?;
        Ok(metadata)
    }
}
