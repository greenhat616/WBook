use tokio_util::sync::CancellationToken;

use super::config::TocConfigError;
use super::rule::build_toc;
use crate::extractor::{Content, ParsedContent};
use crate::parser::{check_cancelled, MatchConfidence, ParserError, TocParser};
use crate::toc::{TocEvent, TocRoot};
use crate::types::TextRange;

pub struct SplitEvenlyParser {
    parts: usize,
}

impl SplitEvenlyParser {
    pub fn new(parts: usize) -> Result<Self, TocConfigError> {
        if parts == 0 {
            return Err(TocConfigError::Invalid(
                "split parts must be positive".into(),
            ));
        }
        Ok(Self { parts })
    }
}

impl TocParser for SplitEvenlyParser {
    fn name(&self) -> &'static str {
        "split-evenly"
    }

    fn accept(&self, _content: &ParsedContent) -> MatchConfidence {
        MatchConfidence(1)
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<TocRoot, ParserError> {
        check_cancelled(ct)?;
        let Content::Text(text) = &content.content;
        let mut char_count = 0;
        for _ in text.chars() {
            check_cancelled(ct)?;
            char_count += 1;
        }
        let parts = self.parts.min(char_count);
        if parts == 0 {
            return build_toc([], ct);
        }
        let mut chars = text.char_indices();
        let mut start = 0;
        let mut events = Vec::new();
        for index in 0..parts {
            // Balance decoded characters, so mixed Chinese/ASCII text is not
            // weighted by UTF-8 width. No multiplication by the text length.
            let count = char_count / parts + usize::from(index < char_count % parts);
            for _ in 0..count {
                check_cancelled(ct)?;
                chars.next();
            }
            let end = chars
                .clone()
                .next()
                .map_or(text.len(), |(offset, _)| offset);
            events.push(TocEvent {
                level: 1,
                title: format!("第 {} 部分", index + 1),
                range: Some(TextRange::new(start as u64, end as u64).expect("ranges are ordered")),
            });
            start = end;
        }
        build_toc(events, ct)
    }
}
