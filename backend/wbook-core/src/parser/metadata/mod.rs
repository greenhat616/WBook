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
    /// ISBN-10 or ISBN-13 as typed; it is packaged without separators.
    pub isbn: Option<String>,
    pub publisher: Option<String>,
    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, the date forms EPUB requires.
    pub published: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, snafu::Snafu)]
pub enum MetadataError {
    #[snafu(display("invalid ISBN {value:?}"))]
    Isbn { value: String },
    #[snafu(display("invalid publication date {value:?}; use YYYY, YYYY-MM or YYYY-MM-DD"))]
    Published { value: String },
}

impl Metadata {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if let Some(value) = &self.isbn {
            if isbn(value).is_none() {
                return IsbnSnafu { value }.fail();
            }
        }
        if let Some(value) = &self.published {
            if !valid_date(value) {
                return PublishedSnafu { value }.fail();
            }
        }
        Ok(())
    }
}

/// The ISBN without its `ISBN` label, hyphens or spaces, or `None` if the
/// check digit is wrong. A typo would otherwise make the book look like a
/// different edition in reading apps that match by ISBN.
pub fn isbn(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value
        .get(..4)
        .filter(|label| label.eq_ignore_ascii_case("isbn"))
        .map_or(value, |_| &value[4..]);
    let value = value.trim_start_matches([':', '：', ' ']);
    let digits: String = value
        .chars()
        .filter(|ch| !matches!(ch, '-' | ' '))
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    let bytes = digits.as_bytes();
    let digit = |byte: u8| u32::from(byte - b'0');
    let valid = match bytes.len() {
        10 if bytes[..9].iter().all(u8::is_ascii_digit)
            && (bytes[9].is_ascii_digit() || bytes[9] == b'X') =>
        {
            let last = if bytes[9] == b'X' {
                10
            } else {
                digit(bytes[9])
            };
            let sum: u32 = bytes[..9]
                .iter()
                .zip((2..=10).rev())
                .map(|(byte, weight)| digit(*byte) * weight)
                .sum();
            (sum + last).is_multiple_of(11)
        }
        13 if bytes.iter().all(u8::is_ascii_digit) => {
            let sum: u32 = bytes
                .iter()
                .enumerate()
                .map(|(index, byte)| digit(*byte) * if index % 2 == 0 { 1 } else { 3 })
                .sum();
            sum.is_multiple_of(10)
        }
        _ => false,
    };
    valid.then_some(digits)
}

fn valid_date(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    let number = |part: &str, len: usize| {
        (part.len() == len && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<i16>().ok())
            .flatten()
    };
    match parts[..] {
        [year] => number(year, 4).is_some(),
        [year, month] => {
            number(year, 4).is_some() && number(month, 2).is_some_and(|m| (1..=12).contains(&m))
        }
        [year, month, day] => match (number(year, 4), number(month, 2), number(day, 2)) {
            (Some(year), Some(month), Some(day)) => {
                jiff::civil::Date::new(year, month as i8, day as i8).is_ok()
            }
            _ => false,
        },
        _ => false,
    }
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
            ..Metadata::default()
        };
        check_cancelled(ct)?;
        Ok(metadata)
    }
}
