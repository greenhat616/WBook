mod config;
mod leveled;
mod presets;
mod rule;
mod split;
mod vbook;

#[cfg(test)]
mod tests;

pub use config::{
    HeadingRuleConfig, LevelRulesConfig, NumeralStyle, PatternRuleConfig, SimpleRuleConfig,
    TocConfigError, TocParserConfig, TocRulesConfig,
};
pub use leveled::RuleSetTocParser;
pub use presets::{
    chapter_only, chapter_only_config, chapter_rules, digit_chapter_rule, easy_pub_config,
    vbook_config, volume_and_chapter, volume_and_chapter_config, volume_rules, EXTRA_PATTERN,
};
pub use rule::{scan_lines, LineRule};
pub use split::SplitEvenlyParser;
pub use vbook::{ChapterMode, VBookConfig, VBookTocParser, VolumeMode};
