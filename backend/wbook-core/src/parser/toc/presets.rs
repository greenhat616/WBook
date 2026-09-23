use super::config::{
    HeadingRuleConfig, LevelRulesConfig, NumeralStyle, PatternRuleConfig, SimpleRuleConfig,
    TocRulesConfig,
};
use super::leveled::RuleSetTocParser;
use super::vbook::{ChapterMode, VBookConfig, VolumeMode};

pub const EXTRA_PATTERN: &str = r"^\s*(简介|序言|序曲|楔子|前言|后记|尾声|番外[^\n]{0,25})$";

fn simple_rule(prefixes: &[&str], suffixes: &[&str]) -> HeadingRuleConfig {
    HeadingRuleConfig::Simple(SimpleRuleConfig {
        allow_leading_space: true,
        prefixes: prefixes.iter().map(|s| (*s).into()).collect(),
        numeral: NumeralStyle::Mixed,
        suffixes: suffixes.iter().map(|s| (*s).into()).collect(),
        min_numeral_len: 1,
        max_numeral_len: Some(9),
        max_title_len: 25,
    })
}

fn extra_pattern() -> HeadingRuleConfig {
    HeadingRuleConfig::Regex(PatternRuleConfig {
        pattern: EXTRA_PATTERN.into(),
        title_group: None,
    })
}

pub fn chapter_rules() -> Vec<HeadingRuleConfig> {
    vec![
        simple_rule(&["第"], &["章", "回", "节", "集"]),
        simple_rule(&["章", "回", "节", "集"], &[]),
    ]
}

pub fn volume_rules() -> Vec<HeadingRuleConfig> {
    vec![
        simple_rule(&["第"], &["部", "卷"]),
        simple_rule(&["卷", "部"], &[]),
    ]
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
                simple_rule(&["第", "卷"], &["章", "回", "卷", "节", "集", "部"]),
                simple_rule(&["卷", "部"], &[]),
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
            fallback_chapters_per_volume: Some(50),
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
