use super::config::{
    HeadingRuleConfig, LevelRulesConfig, NumeralStyle, PatternRuleConfig, SimpleRuleConfig,
    TocRulesConfig,
};
use super::leveled::RuleSetTocParser;
use super::vbook::{ChapterMode, VBookConfig, VolumeMode};

pub const EXTRA_PATTERN: &str = r"^\s*(简介|序言|序曲|楔子|前言|后记|尾声|番外[^\n]{0,25})$";

pub(super) const MAX_TITLE_LEN: usize = 25;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}

fn simple_rule(
    prefixes: Vec<String>,
    suffixes: Vec<String>,
    max_title_len: usize,
) -> HeadingRuleConfig {
    HeadingRuleConfig::Simple(SimpleRuleConfig {
        allow_leading_space: true,
        prefixes,
        numeral: NumeralStyle::Mixed,
        suffixes,
        min_numeral_len: 1,
        max_numeral_len: Some(9),
        max_title_len,
    })
}

/// Matches both "第十章" and the prefix-only "章十" forms of the given marks.
pub(super) fn heading_rules(marks: &[String], max_title_len: usize) -> Vec<HeadingRuleConfig> {
    vec![
        simple_rule(strings(&["第"]), marks.to_vec(), max_title_len),
        simple_rule(marks.to_vec(), vec![], max_title_len),
    ]
}

pub(super) fn extra_pattern() -> HeadingRuleConfig {
    HeadingRuleConfig::Regex(PatternRuleConfig {
        pattern: EXTRA_PATTERN.into(),
        title_group: None,
    })
}

pub(super) fn chapter_marks() -> Vec<String> {
    strings(&["章", "回", "节", "集"])
}

pub(super) fn volume_marks() -> Vec<String> {
    strings(&["部", "卷"])
}

pub fn chapter_rules() -> Vec<HeadingRuleConfig> {
    heading_rules(&chapter_marks(), MAX_TITLE_LEN)
}

pub fn volume_rules() -> Vec<HeadingRuleConfig> {
    heading_rules(&volume_marks(), MAX_TITLE_LEN)
}

pub fn digit_chapter_rule() -> SimpleRuleConfig {
    SimpleRuleConfig {
        allow_leading_space: true,
        prefixes: vec![],
        numeral: NumeralStyle::Arabic,
        suffixes: vec![],
        min_numeral_len: 1,
        max_numeral_len: None,
        max_title_len: 25,
    }
}

pub fn chapter_only_config() -> TocRulesConfig {
    let mut rules = chapter_rules();
    rules.push(extra_pattern());
    TocRulesConfig {
        levels: vec![LevelRulesConfig { level: 1, rules }],
    }
}

pub fn volume_and_chapter_config() -> TocRulesConfig {
    let mut volumes = volume_rules();
    volumes.push(extra_pattern());
    TocRulesConfig {
        levels: vec![
            LevelRulesConfig {
                level: 1,
                rules: volumes,
            },
            LevelRulesConfig {
                level: 2,
                rules: chapter_rules(),
            },
        ],
    }
}

pub fn easy_pub_config() -> TocRulesConfig {
    TocRulesConfig {
        levels: vec![LevelRulesConfig {
            level: 1,
            rules: vec![
                simple_rule(
                    strings(&["第", "卷"]),
                    strings(&["章", "回", "卷", "节", "集", "部"]),
                    MAX_TITLE_LEN,
                ),
                simple_rule(strings(&["卷", "部"]), vec![], MAX_TITLE_LEN),
                extra_pattern(),
            ],
        }],
    }
}

pub fn vbook_config() -> VBookConfig {
    VBookConfig {
        chapters: ChapterMode::Rules(chapter_rules()),
        volumes: VolumeMode::Normal {
            rules: volume_rules(),
            fallback_chapters_per_volume: None,
        },
    }
}

pub fn chapter_only() -> RuleSetTocParser {
    RuleSetTocParser::from_config("chapter-only", &chapter_only_config())
        .expect("built-in preset compiles")
}

pub fn volume_and_chapter() -> RuleSetTocParser {
    RuleSetTocParser::from_config("volume-and-chapter", &volume_and_chapter_config())
        .expect("built-in preset compiles")
}
