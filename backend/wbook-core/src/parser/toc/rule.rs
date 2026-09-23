use regex::Regex;
use tokio_util::sync::CancellationToken;

use super::config::{HeadingRuleConfig, NumeralStyle, SimpleRuleConfig, TocConfigError};
use crate::parser::{check_cancelled, ParserError};
use crate::toc::{TocBuilder, TocEvent, TocRoot};
use crate::types::TextRange;

pub struct LineRule {
    pub(super) level: usize,
    matcher: Matcher,
}

enum Matcher {
    Simple(SimpleRuleConfig),
    Regex {
        pattern: Regex,
        title_group: Option<usize>,
    },
}

impl LineRule {
    pub fn new(level: usize, config: &HeadingRuleConfig) -> Result<Self, TocConfigError> {
        if level == 0 {
            return Err(TocConfigError::Invalid(
                "TOC levels must be positive".into(),
            ));
        }
        let matcher = match config {
            HeadingRuleConfig::Simple(config) => {
                config.validate()?;
                Matcher::Simple(config.clone())
            }
            HeadingRuleConfig::Regex(config) => {
                let pattern = Regex::new(&config.pattern)?;
                if config
                    .title_group
                    .is_some_and(|group| group >= pattern.captures_len())
                {
                    return Err(TocConfigError::Invalid(
                        "title capture group does not exist".into(),
                    ));
                }
                Matcher::Regex {
                    pattern,
                    title_group: config.title_group,
                }
            }
        };
        Ok(Self { level, matcher })
    }

    pub fn match_title<'a>(&self, line: &'a str) -> Option<&'a str> {
        match &self.matcher {
            Matcher::Simple(config) => match_simple(config, line),
            Matcher::Regex {
                pattern,
                title_group,
            } => {
                let captures = pattern.captures(line)?;
                let title = match title_group {
                    Some(group) => captures.get(*group)?.as_str().trim(),
                    None => line.trim(),
                };
                (!title.is_empty()).then_some(title)
            }
        }
    }
}

fn strip_affix<'a>(text: &'a str, alternatives: &[String]) -> Option<&'a str> {
    if alternatives.is_empty() {
        return Some(text);
    }
    alternatives
        .iter()
        .filter(|affix| text.starts_with(affix.as_str()))
        .max_by_key(|affix| affix.len())
        .map(|affix| &text[affix.len()..])
}

fn match_simple<'a>(config: &SimpleRuleConfig, line: &'a str) -> Option<&'a str> {
    let heading = if config.allow_leading_space {
        line.trim_start()
    } else {
        line
    };
    let after_prefix = strip_affix(heading, &config.prefixes)?;
    // A bounded regex can backtrack and treat excess or disallowed numerals
    // as the title of a bare heading. Validate the run without backtracking.
    let mut numeral_end = 0;
    let mut numeral_len = 0;
    for (offset, c) in after_prefix.char_indices() {
        if !NumeralStyle::Mixed.contains(c) {
            break;
        }
        numeral_len += 1;
        if !config.numeral.contains(c)
            || config.max_numeral_len.is_some_and(|max| numeral_len > max)
        {
            return None;
        }
        numeral_end = offset + c.len_utf8();
    }
    if numeral_len < config.min_numeral_len {
        return None;
    }
    let title = strip_affix(&after_prefix[numeral_end..], &config.suffixes)?.trim();
    if title
        .chars()
        .take(config.max_title_len.saturating_add(1))
        .count()
        > config.max_title_len
    {
        return None;
    }
    Some(heading.trim_end())
}

pub(super) fn text_lines(text: &str) -> impl Iterator<Item = (u64, &str)> {
    let mut offset = 0;
    text.split_inclusive('\n').map(move |raw| {
        let start = offset;
        offset += raw.len() as u64;
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        (start, line.strip_suffix('\r').unwrap_or(line))
    })
}

// Events retain heading locations, not chapter body spans. Consumers can use
// successive starts and the original text length to determine content spans.
pub fn scan_lines(
    text: &str,
    rules: &[LineRule],
    ct: &CancellationToken,
) -> Result<Vec<TocEvent>, ParserError> {
    check_cancelled(ct)?;
    let mut events = Vec::new();
    for (start, line) in text_lines(text) {
        check_cancelled(ct)?;
        for rule in rules {
            if let Some(title) = rule.match_title(line) {
                events.push(TocEvent {
                    level: rule.level,
                    title: title.to_string(),
                    range: Some(
                        TextRange::new(start, start + line.len() as u64)
                            .expect("line range is ordered"),
                    ),
                });
                break;
            }
        }
    }
    check_cancelled(ct)?;
    Ok(events)
}

pub(super) fn build_toc(
    events: impl IntoIterator<Item = TocEvent>,
    ct: &CancellationToken,
) -> Result<TocRoot, ParserError> {
    check_cancelled(ct)?;
    let mut builder = TocBuilder::new();
    for event in events {
        check_cancelled(ct)?;
        builder
            .push(event)
            .map_err(|err| ParserError::Other(err.into()))?;
    }
    let root = builder.build();
    check_cancelled(ct)?;
    Ok(root)
}
