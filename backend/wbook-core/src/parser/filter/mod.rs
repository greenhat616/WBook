use std::sync::LazyLock;

use regex::Regex;
use tokio_util::sync::CancellationToken;

use crate::document::TextEdit;
use crate::document::TextView;
use crate::parser::{check_cancelled, FilterParser, MatchConfidence, ParserError};

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
    fn accept(&self, _content: TextView<'_>) -> MatchConfidence {
        MatchConfidence(10)
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: TextView<'_>,
    ) -> Result<Vec<TextEdit>, ParserError> {
        check_cancelled(ct)?;
        let text = content;
        let keywords: Vec<&str> = AD_KEYWORDS
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let mut ops = Vec::new();
        for line in text.lines(ct) {
            let line = line?;
            let raw_line = line.raw.as_ref();
            if keywords.iter().any(|kw| raw_line.contains(kw)) || URL_PATTERN.is_match(raw_line) {
                ops.push(TextEdit {
                    range: line.range,
                    insert: String::new(),
                });
            }
        }
        check_cancelled(ct)?;
        Ok(ops)
    }
}
