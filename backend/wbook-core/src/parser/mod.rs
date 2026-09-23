//! Parsers consume the intermediate representation (`ParsedContent`) produced by
//! an Extractor and turn it into document structures.
//!
//! TOC parsing follows calibre's conventions: the `--level1-toc` / `--level2-toc`
//! / `--level3-toc` xpath detection produces a flat (level, title, position)
//! event stream, which is assembled into a tree by [`crate::toc::TocBuilder`]
//! (levels are 1-based, level jumps auto-create container nodes).

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use crate::extractor::ParsedContent;
use crate::toc::TocRoot;
use crate::types::{ByteRange, TextRange};

/// A position in the content: character range in the decoded text plus byte
/// range in the raw file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct ContentRange {
    pub chars: TextRange,
    pub bytes: ByteRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TransformOperation {
    Replace {
        range: ContentRange,
        replacement: String,
    },
    Delete {
        range: ContentRange,
    },
    Insert {
        range: ContentRange,
        insertion: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum ParserKind {
    /// Filter parser that applies transformations based on specific rules.
    Filter,

    /// Table of contents parser that generates a structured representation of headings.
    Toc,

    /// Metadata parser that extracts document metadata such as title and author.
    Metadata,
}

#[derive(Debug, thiserror::Error)]
pub enum ParserError {
    #[error("parsing cancelled")]
    Cancelled,

    #[error("no parser accepted the content: {0}")]
    NoMatch(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub(crate) fn check_cancelled(ct: &CancellationToken) -> Result<(), ParserError> {
    if ct.is_cancelled() {
        Err(ParserError::Cancelled)
    } else {
        Ok(())
    }
}

/// Match confidence, modelled after calibre's input plugin selection.
/// `0` means the parser does not accept the content at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
pub struct MatchConfidence(pub u8);

impl MatchConfidence {
    pub const NONE: Self = Self(0);
}

pub trait TocParser: Send + Sync {
    fn name(&self) -> &'static str;

    /// Whether this parser can handle the given content, and with what confidence.
    fn accept(&self, content: &ParsedContent) -> MatchConfidence;

    fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<TocRoot, ParserError>;

    fn kind(&self) -> ParserKind {
        ParserKind::Toc
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineStrategy {
    /// Use the highest-confidence parser; on failure fall back in descending
    /// confidence order.
    BestMatch,

    /// Reserved, not implemented: several parsers each produce a `TocEvent`
    /// stream (e.g. a "volume" regex parser for level 1 and a "chapter" regex
    /// parser for level 2), and the streams are merged into one tree through
    /// `TocBuilder` by level priority.
    Merge,
}

pub struct CombinedParser {
    parsers: Vec<Box<dyn TocParser>>,
    strategy: CombineStrategy,
}

impl CombinedParser {
    pub fn new(strategy: CombineStrategy) -> Self {
        Self {
            parsers: Vec::new(),
            strategy,
        }
    }

    pub fn add(&mut self, parser: impl TocParser + 'static) -> &mut Self {
        self.parsers.push(Box::new(parser));
        self
    }

    pub fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<TocRoot, ParserError> {
        check_cancelled(ct)?;
        match self.strategy {
            CombineStrategy::BestMatch => self.parse_best_match(ct, content),
            CombineStrategy::Merge => Err(ParserError::Other(anyhow::anyhow!(
                "CombineStrategy::Merge is reserved and not implemented yet"
            ))),
        }
    }

    fn parse_best_match(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<TocRoot, ParserError> {
        let mut candidates: Vec<&dyn TocParser> = self
            .parsers
            .iter()
            .map(|parser| parser.as_ref())
            .filter(|parser| parser.accept(content) > MatchConfidence::NONE)
            .collect();
        candidates.sort_by_key(|parser| std::cmp::Reverse(parser.accept(content)));

        let mut errors = Vec::new();
        for parser in candidates {
            check_cancelled(ct)?;
            let result = parser.parse(ct, content);
            check_cancelled(ct)?;
            match result {
                Ok(toc) => return Ok(toc),
                Err(ParserError::Cancelled) => return Err(ParserError::Cancelled),
                Err(err) => errors.push(format!("{}: {err}", parser.name())),
            }
        }
        check_cancelled(ct)?;
        Err(ParserError::NoMatch(errors.join("; ")))
    }
}

impl TocParser for CombinedParser {
    fn name(&self) -> &'static str {
        "combined"
    }

    fn accept(&self, content: &ParsedContent) -> MatchConfidence {
        self.parsers
            .iter()
            .map(|parser| parser.accept(content))
            .max()
            .unwrap_or(MatchConfidence::NONE)
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<TocRoot, ParserError> {
        CombinedParser::parse(self, ct, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractor::{Content, Encoding};
    use crate::toc::{Toc, TocSnapshot};

    fn content() -> ParsedContent {
        ParsedContent {
            encoding: Encoding {
                name: "utf-8".to_string(),
                bom: false,
            },
            content: Content::Text("第一章 ……".to_string()),
        }
    }

    struct StubParser {
        name: &'static str,
        confidence: MatchConfidence,
        fail: bool,
    }

    impl TocParser for StubParser {
        fn name(&self) -> &'static str {
            self.name
        }

        fn accept(&self, _content: &ParsedContent) -> MatchConfidence {
            self.confidence
        }

        fn parse(
            &self,
            _ct: &CancellationToken,
            _content: &ParsedContent,
        ) -> Result<TocRoot, ParserError> {
            if self.fail {
                return Err(ParserError::Other(anyhow::anyhow!("{} failed", self.name)));
            }
            let mut toc = TocRoot::new();
            toc.add_with_meta(self.name, None, None).unwrap();
            Ok(toc)
        }
    }

    fn first_title(toc: &TocRoot) -> String {
        TocSnapshot::from(toc)[0].title.clone()
    }

    #[test]
    fn best_match_selects_highest_confidence() {
        let mut combined = CombinedParser::new(CombineStrategy::BestMatch);
        combined.add(StubParser {
            name: "low",
            confidence: MatchConfidence(10),
            fail: false,
        });
        combined.add(StubParser {
            name: "high",
            confidence: MatchConfidence(50),
            fail: false,
        });
        let toc = combined
            .parse(&CancellationToken::new(), &content())
            .unwrap();
        assert_eq!(first_title(&toc), "high");
    }

    #[test]
    fn best_match_falls_back_on_error() {
        let mut combined = CombinedParser::new(CombineStrategy::BestMatch);
        combined.add(StubParser {
            name: "high",
            confidence: MatchConfidence(50),
            fail: true,
        });
        combined.add(StubParser {
            name: "low",
            confidence: MatchConfidence(10),
            fail: false,
        });
        let toc = combined
            .parse(&CancellationToken::new(), &content())
            .unwrap();
        assert_eq!(first_title(&toc), "low");
    }

    #[test]
    fn no_match_when_nobody_accepts() {
        let mut combined = CombinedParser::new(CombineStrategy::BestMatch);
        combined.add(StubParser {
            name: "picky",
            confidence: MatchConfidence::NONE,
            fail: false,
        });
        let result = combined.parse(&CancellationToken::new(), &content());
        assert!(matches!(result, Err(ParserError::NoMatch(_))));
    }

    #[test]
    fn no_match_collects_error_summaries() {
        let mut combined = CombinedParser::new(CombineStrategy::BestMatch);
        combined.add(StubParser {
            name: "first",
            confidence: MatchConfidence(50),
            fail: true,
        });
        combined.add(StubParser {
            name: "second",
            confidence: MatchConfidence(10),
            fail: true,
        });
        let result = combined.parse(&CancellationToken::new(), &content());
        match result {
            Err(ParserError::NoMatch(summary)) => {
                assert!(summary.contains("first"));
                assert!(summary.contains("second"));
            }
            other => panic!("expected NoMatch, got {other:?}"),
        }
    }

    #[test]
    fn merge_strategy_is_reserved() {
        let combined = CombinedParser::new(CombineStrategy::Merge);
        let result = combined.parse(&CancellationToken::new(), &content());
        assert!(matches!(result, Err(ParserError::Other(_))));
    }

    struct CancellingParser {
        set_token: bool,
    }

    impl TocParser for CancellingParser {
        fn name(&self) -> &'static str {
            "cancel"
        }

        fn accept(&self, _: &ParsedContent) -> MatchConfidence {
            MatchConfidence(100)
        }

        fn parse(&self, ct: &CancellationToken, _: &ParsedContent) -> Result<TocRoot, ParserError> {
            if self.set_token {
                ct.cancel();
                Ok(TocRoot::new())
            } else {
                Err(ParserError::Cancelled)
            }
        }
    }

    #[test]
    fn cancellation_never_falls_back_or_becomes_success() {
        for set_token in [false, true] {
            let mut combined = CombinedParser::new(CombineStrategy::BestMatch);
            combined.add(CancellingParser { set_token });
            combined.add(StubParser {
                name: "fallback",
                confidence: MatchConfidence(10),
                fail: false,
            });
            assert!(matches!(
                combined.parse(&CancellationToken::new(), &content()),
                Err(ParserError::Cancelled)
            ));
        }
    }

    #[test]
    fn cancelled_combined_parser_without_candidates_is_not_no_match() {
        let ct = CancellationToken::new();
        ct.cancel();
        let combined = CombinedParser::new(CombineStrategy::BestMatch);
        assert!(matches!(
            combined.parse(&ct, &content()),
            Err(ParserError::Cancelled)
        ));
    }
}
