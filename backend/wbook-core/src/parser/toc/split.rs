use tokio_util::sync::CancellationToken;

use super::config::TocConfigError;
use super::rule::build_toc;
use crate::document::TextView;
use crate::parser::{check_cancelled, MatchConfidence, ParserError, TocParser};
use crate::toc::{TocEvent, TocRoot};
use crate::types::TextRange;

pub struct SplitEvenlyParser {
    parts: usize,
}

impl SplitEvenlyParser {
    pub fn new(parts: usize) -> Result<Self, TocConfigError> {
        if parts == 0 {
            return Err(TocConfigError::Invalid {
                message: "split parts must be positive".into(),
            });
        }
        Ok(Self { parts })
    }
}

impl TocParser for SplitEvenlyParser {
    fn name(&self) -> &'static str {
        "split-evenly"
    }

    fn accept(&self, _content: TextView<'_>) -> MatchConfidence {
        MatchConfidence(1)
    }

    fn parse(&self, ct: &CancellationToken, content: TextView<'_>) -> Result<TocRoot, ParserError> {
        check_cancelled(ct)?;
        let text = content;
        let mut char_count = 0;
        for character in text.char_indices(ct) {
            character?;
            char_count += 1;
        }
        let parts = self.parts.min(char_count);
        if parts == 0 {
            return build_toc([], ct);
        }
        let mut chars = text.char_indices(ct).peekable();
        let mut start = 0;
        let mut events = Vec::new();
        for index in 0..parts {
            // Balance decoded characters, so mixed Chinese/ASCII text is not
            // weighted by UTF-8 width. No multiplication by the text length.
            let count = char_count / parts + usize::from(index < char_count % parts);
            for _ in 0..count {
                check_cancelled(ct)?;
                if let Some(character) = chars.next() {
                    character?;
                }
            }
            let end = match chars.peek() {
                Some(Ok((offset, _))) => *offset,
                Some(Err(_)) => return Err(chars.next().unwrap().unwrap_err().into()),
                None => text.len(),
            };
            events.push(TocEvent {
                range_kind: crate::toc::TocRangeKind::Body,
                level: 1,
                title: format!("第 {} 部分", index + 1),
                range: Some(TextRange::new(start, end).expect("ranges are ordered")),
            });
            start = end;
        }
        build_toc(events, ct)
    }
}
