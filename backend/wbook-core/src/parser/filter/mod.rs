use std::sync::LazyLock;

use regex::Regex;
use tokio_util::sync::CancellationToken;

use crate::extractor::{Content, ParsedContent};
use crate::parser::{check_cancelled, FilterParser, MatchConfidence, ParserError};
use crate::types::{TextOp, TextRange};

#[cfg(test)]
mod tests;

/// Embedded ad keyword dictionary, one keyword per line.
const AD_KEYWORDS: &str = include_str!("ad_keywords.txt");

static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(https?://|www\.)\S+|[a-z0-9-]+\.(com|net|org|cc|me|io|xyz)\S*").unwrap()
});

/// Deletes whole lines (including the trailing newline) that contain an ad
/// keyword or look like a URL.
pub struct AdFilterParser;

impl AdFilterParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AdFilterParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterParser for AdFilterParser {
    fn name(&self) -> &'static str {
        "ad-filter"
    }

    /// Ad filtering is a safe default for any plain text, so always accept
    /// at low confidence.
    fn accept(&self, _content: &ParsedContent) -> MatchConfidence {
        MatchConfidence(10)
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<Vec<TextOp>, ParserError> {
        check_cancelled(ct)?;
        let Content::Text(text) = &content.content;
        let keywords: Vec<&str> = AD_KEYWORDS
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let mut ops = Vec::new();
        let mut line_start = 0u64;
        for raw_line in text.split_inclusive('\n') {
            check_cancelled(ct)?;
            let line_end = line_start + raw_line.len() as u64;
            if keywords.iter().any(|kw| raw_line.contains(kw)) || URL_PATTERN.is_match(raw_line) {
                let range = TextRange::new(line_start, line_end).expect("line range is ordered");
                ops.push(TextOp::Delete { range });
            }
            line_start = line_end;
        }
        ops.sort_by_key(|op| match op {
            TextOp::Replace { range, .. }
            | TextOp::Delete { range }
            | TextOp::Insert { range, .. } => range.start,
        });
        check_cancelled(ct)?;
        Ok(ops)
    }
}
