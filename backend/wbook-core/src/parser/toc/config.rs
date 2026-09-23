use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::rule::LineRule;

#[derive(Debug, thiserror::Error)]
pub enum TocConfigError {
    #[error("invalid TOC configuration: {0}")]
    Invalid(String),
    #[error(transparent)]
    Regex(#[from] regex::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum NumeralStyle {
    Arabic,
    Chinese,
    Mixed,
}

impl NumeralStyle {
    pub(super) fn contains(self, c: char) -> bool {
        let arabic = c.is_ascii_digit();
        let chinese = "零〇一二三四五六七八九十百千两".contains(c);
        match self {
            Self::Arabic => arabic,
            Self::Chinese => chinese,
            Self::Mixed => arabic || chinese,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SimpleRuleConfig {
    pub allow_leading_space: bool,
    // Literal alternatives support both EasyPub's single characters and
    // VBook's multi-character affixes, without interpreting either as regex.
    pub prefixes: Vec<String>,
    pub numeral: NumeralStyle,
    pub suffixes: Vec<String>,
    pub min_numeral_len: usize,
    pub max_numeral_len: Option<usize>,
    // Count Unicode scalar values in the trimmed text after the suffix.
    pub max_title_len: usize,
}

impl SimpleRuleConfig {
    pub(super) fn validate(&self) -> Result<(), TocConfigError> {
        if self.min_numeral_len == 0
            || self
                .max_numeral_len
                .is_some_and(|max| max < self.min_numeral_len)
        {
            return Err(TocConfigError::Invalid(
                "invalid numeral length bounds".into(),
            ));
        }
        if self
            .prefixes
            .iter()
            .chain(&self.suffixes)
            .any(|s| s.is_empty() || s.contains(['\r', '\n']))
        {
            return Err(TocConfigError::Invalid(
                "affixes must be nonempty single-line literals; use an empty list for no affix"
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PatternRuleConfig {
    pub pattern: String,
    // Ordinary grouping must not silently truncate the displayed heading.
    // None keeps the complete line; Some selects a capture explicitly.
    pub title_group: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum HeadingRuleConfig {
    Simple(SimpleRuleConfig),
    Regex(PatternRuleConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LevelRulesConfig {
    pub level: usize,
    pub rules: Vec<HeadingRuleConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TocRulesConfig {
    pub levels: Vec<LevelRulesConfig>,
}

impl TocRulesConfig {
    pub fn compile(&self) -> Result<Vec<LineRule>, TocConfigError> {
        let mut seen = HashSet::new();
        let mut levels: Vec<_> = self.levels.iter().collect();
        for config in &levels {
            if config.level == 0 || !seen.insert(config.level) {
                return Err(TocConfigError::Invalid(
                    "TOC levels must be positive and unique".into(),
                ));
            }
        }
        // A parent match wins when the same line also matches a child rule.
        // Within a level, preserve the caller's rule order.
        levels.sort_by_key(|config| config.level);
        levels
            .into_iter()
            .flat_map(|config| {
                config
                    .rules
                    .iter()
                    .map(move |rule| LineRule::new(config.level, rule))
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TocParserConfig {
    Levels(TocRulesConfig),
    VBook(super::vbook::VBookConfig),
    SplitEvenly { parts: usize },
}

impl TocParserConfig {
    pub fn build(&self) -> Result<Box<dyn crate::parser::TocParser>, TocConfigError> {
        match self {
            Self::Levels(config) => Ok(Box::new(super::leveled::RuleSetTocParser::from_config(
                "level-rules",
                config,
            )?)),
            Self::VBook(config) => Ok(Box::new(super::vbook::VBookTocParser::from_config(config)?)),
            Self::SplitEvenly { parts } => {
                Ok(Box::new(super::split::SplitEvenlyParser::new(*parts)?))
            }
        }
    }
}
