use tokio_util::sync::CancellationToken;

use super::config::{TocConfigError, TocRulesConfig};
use super::rule::{build_toc, scan_lines, LineRule};
use crate::extractor::{Content, ParsedContent};
use crate::parser::{MatchConfidence, ParserError, TocParser};
use crate::toc::TocRoot;

pub struct RuleSetTocParser {
    name: &'static str,
    rules: Vec<LineRule>,
}

impl RuleSetTocParser {
    pub fn from_config(
        name: &'static str,
        config: &TocRulesConfig,
    ) -> Result<Self, TocConfigError> {
        Ok(Self {
            name,
            rules: config.compile()?,
        })
    }
}

pub(super) fn rule_confidence(text: &str, rules: &[LineRule]) -> MatchConfidence {
    let hits = text
        .lines()
        .take(2000)
        .filter(|line| rules.iter().any(|rule| rule.match_title(line).is_some()))
        .count();
    // Any detected heading must outrank the length-split fallback (confidence 1).
    MatchConfidence(if hits == 0 {
        0
    } else {
        hits.clamp(2, 100) as u8
    })
}

impl TocParser for RuleSetTocParser {
    fn name(&self) -> &'static str {
        self.name
    }

    fn accept(&self, content: &ParsedContent) -> MatchConfidence {
        let Content::Text(text) = &content.content;
        rule_confidence(text, &self.rules)
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<TocRoot, ParserError> {
        let Content::Text(text) = &content.content;
        build_toc(scan_lines(text, &self.rules, ct)?, ct)
    }
}
