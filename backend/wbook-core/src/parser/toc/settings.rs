use serde::{Deserialize, Serialize};
use specta::Type;

use super::config::{LevelRulesConfig, TocConfigError, TocParserConfig, TocRulesConfig};
use super::presets::{chapter_marks, extra_pattern, heading_rules, volume_marks, MAX_TITLE_LEN};
use super::vbook::{ChapterMode, VBookConfig, VolumeMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TocMode {
    /// Chapters grouped into volumes by volume headings or by count.
    VBook,
    Chapters,
    /// Two levels: volume headings above chapter headings.
    Volumes,
    /// No headings; split the text evenly.
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum VolumeSplit {
    Titles,
    Forced,
    None,
}

/// The user-facing parser options; [`TocSettings::to_config`] expands them
/// into the general rule configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TocSettings {
    pub mode: TocMode,
    pub chapter_marks: Vec<String>,
    pub volume_marks: Vec<String>,
    pub max_title_len: usize,
    pub volume_split: VolumeSplit,
    /// In VBook mode with volume titles, sizes volumes for a book that has no
    /// volume headings; 0 leaves its chapters ungrouped.
    pub chapters_per_volume: usize,
    pub parts: usize,
}

impl Default for TocSettings {
    fn default() -> Self {
        Self {
            mode: TocMode::VBook,
            chapter_marks: chapter_marks(),
            volume_marks: volume_marks(),
            max_title_len: MAX_TITLE_LEN,
            volume_split: VolumeSplit::Titles,
            // Invented volumes only add a level readers must open.
            chapters_per_volume: 0,
            parts: 10,
        }
    }
}

fn invalid(message: &str) -> TocConfigError {
    TocConfigError::Invalid {
        message: message.into(),
    }
}

impl TocSettings {
    /// Rejects option combinations that would compile but silently match the
    /// wrong lines, such as an empty mark list; [`TocParserConfig::build`]
    /// checks the rest.
    pub fn to_config(&self) -> Result<TocParserConfig, TocConfigError> {
        if self.mode == TocMode::Split {
            return Ok(TocParserConfig::SplitEvenly { parts: self.parts });
        }
        let needs_volume_marks = match self.mode {
            TocMode::Volumes => true,
            TocMode::VBook => self.volume_split == VolumeSplit::Titles,
            _ => false,
        };
        if self.chapter_marks.is_empty() {
            return Err(invalid("at least one chapter mark is required"));
        }
        if needs_volume_marks && self.volume_marks.is_empty() {
            return Err(invalid("at least one volume mark is required"));
        }
        if self.max_title_len == 0 {
            return Err(invalid("the title length limit must be positive"));
        }
        if self.mode == TocMode::VBook
            && self.volume_split == VolumeSplit::Forced
            && self.chapters_per_volume == 0
        {
            return Err(invalid("chapters per volume must be positive"));
        }

        let chapters = heading_rules(&self.chapter_marks, self.max_title_len);
        let volumes = heading_rules(&self.volume_marks, self.max_title_len);
        Ok(match self.mode {
            TocMode::Split => unreachable!(),
            TocMode::Chapters => TocParserConfig::Levels(TocRulesConfig {
                levels: vec![LevelRulesConfig {
                    level: 1,
                    rules: [chapters, vec![extra_pattern()]].concat(),
                }],
            }),
            TocMode::Volumes => TocParserConfig::Levels(TocRulesConfig {
                levels: vec![
                    LevelRulesConfig {
                        level: 1,
                        rules: [volumes, vec![extra_pattern()]].concat(),
                    },
                    LevelRulesConfig {
                        level: 2,
                        rules: chapters,
                    },
                ],
            }),
            TocMode::VBook => TocParserConfig::VBook(VBookConfig {
                chapters: ChapterMode::Rules(chapters),
                volumes: match self.volume_split {
                    VolumeSplit::None => VolumeMode::None,
                    VolumeSplit::Forced => VolumeMode::Forced {
                        chapters_per_volume: self.chapters_per_volume,
                    },
                    VolumeSplit::Titles => VolumeMode::Normal {
                        rules: volumes,
                        fallback_chapters_per_volume: (self.chapters_per_volume > 0)
                            .then_some(self.chapters_per_volume),
                    },
                },
            }),
        })
    }
}
