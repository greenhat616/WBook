use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::config::{HeadingRuleConfig, TocConfigError};
use super::leveled::rule_confidence;
use super::rule::{build_toc, scan_lines, text_lines, LineRule};
use crate::extractor::{Content, ParsedContent};
use crate::parser::{check_cancelled, MatchConfidence, ParserError, TocParser};
use crate::toc::{TocEvent, TocRoot};
use crate::types::TextRange;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ChapterMode {
    Rules(Vec<HeadingRuleConfig>),
    EndMarker { marker: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum VolumeMode {
    None,
    Normal {
        rules: Vec<HeadingRuleConfig>,
        fallback_chapters_per_volume: Option<usize>,
    },
    FromChapterTitles {
        rules: Vec<HeadingRuleConfig>,
    },
    Forced {
        chapters_per_volume: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct VBookConfig {
    pub chapters: ChapterMode,
    pub volumes: VolumeMode,
}

pub struct VBookTocParser {
    config: VBookConfig,
    chapter_rules: Vec<LineRule>,
    volume_rules: Vec<LineRule>,
}

impl VBookTocParser {
    pub fn from_config(config: &VBookConfig) -> Result<Self, TocConfigError> {
        let chapter_rules = match &config.chapters {
            ChapterMode::Rules(rules) => compile_rules(2, rules)?,
            ChapterMode::EndMarker { marker } => {
                if marker.trim().is_empty() || marker.contains(['\r', '\n']) {
                    return Err(TocConfigError::Invalid(
                        "chapter end marker must be a nonempty single line".into(),
                    ));
                }
                if matches!(config.volumes, VolumeMode::FromChapterTitles { .. }) {
                    return Err(TocConfigError::Invalid(
                        "splitting volume prefixes requires chapter heading rules".into(),
                    ));
                }
                vec![]
            }
        };
        let volume_rules = match &config.volumes {
            VolumeMode::Normal {
                rules,
                fallback_chapters_per_volume,
            } => {
                validate_group_size(*fallback_chapters_per_volume)?;
                compile_rules(1, rules)?
            }
            VolumeMode::FromChapterTitles { rules } => compile_rules(1, rules)?,
            VolumeMode::Forced {
                chapters_per_volume,
            } => {
                validate_group_size(Some(*chapters_per_volume))?;
                vec![]
            }
            VolumeMode::None => vec![],
        };
        Ok(Self {
            config: config.clone(),
            chapter_rules,
            volume_rules,
        })
    }

    fn inline_heading<'a>(
        &self,
        line: &'a str,
        ct: &CancellationToken,
    ) -> Result<Option<(usize, &'a str, &'a str)>, ParserError> {
        for (split, _) in line.char_indices().skip(1) {
            check_cancelled(ct)?;
            let Some(volume) = self
                .volume_rules
                .iter()
                .find_map(|rule| rule.match_title(line[..split].trim_end()))
            else {
                continue;
            };
            if let Some(chapter) = self
                .chapter_rules
                .iter()
                .find_map(|rule| rule.match_title(line[split..].trim_start()))
            {
                return Ok(Some((split, volume, chapter)));
            }
        }
        Ok(None)
    }

    fn scan_inline(
        &self,
        text: &str,
        ct: &CancellationToken,
    ) -> Result<Vec<TocEvent>, ParserError> {
        let mut events = Vec::new();
        let mut current_volume = String::new();
        for (start, line) in text_lines(text) {
            check_cancelled(ct)?;
            if let Some((split, volume, chapter)) = self.inline_heading(line, ct)? {
                if volume != current_volume {
                    events.push(heading(1, volume, start, start + split as u64));
                    current_volume = volume.to_string();
                }
                events.push(heading(
                    2,
                    chapter,
                    start + split as u64,
                    start + line.len() as u64,
                ));
            } else if let Some(title) = self
                .chapter_rules
                .iter()
                .find_map(|rule| rule.match_title(line))
            {
                events.push(heading(2, title, start, start + line.len() as u64));
            }
        }
        check_cancelled(ct)?;
        Ok(events)
    }

    fn scan_chapters(
        &self,
        text: &str,
        ct: &CancellationToken,
    ) -> Result<Vec<TocEvent>, ParserError> {
        match &self.config.chapters {
            ChapterMode::Rules(_) => scan_lines(text, &self.chapter_rules, ct),
            ChapterMode::EndMarker { marker } => {
                let mut events = Vec::new();
                let mut at_start = true;
                for (start, line) in text_lines(text) {
                    check_cancelled(ct)?;
                    if line.trim() == marker.trim()
                        || self
                            .volume_rules
                            .iter()
                            .any(|rule| rule.match_title(line).is_some())
                    {
                        at_start = true;
                    } else if at_start && !line.trim().is_empty() {
                        // The first nonempty line names the segment. Markers
                        // only locate boundaries; the source text stays intact.
                        events.push(heading(2, line.trim(), start, start + line.len() as u64));
                        at_start = false;
                    }
                }
                check_cancelled(ct)?;
                Ok(events)
            }
        }
    }
}

impl TocParser for VBookTocParser {
    fn name(&self) -> &'static str {
        "vbook"
    }

    fn accept(&self, content: &ParsedContent) -> MatchConfidence {
        let Content::Text(text) = &content.content;
        if let ChapterMode::EndMarker { marker } = &self.config.chapters {
            return MatchConfidence(
                if text
                    .lines()
                    .take(2000)
                    .any(|line| line.trim() == marker.trim())
                {
                    2
                } else {
                    0
                },
            );
        }
        if matches!(self.config.volumes, VolumeMode::FromChapterTitles { .. }) {
            let ct = CancellationToken::new();
            if text.lines().take(2000).any(|line| {
                self.inline_heading(line, &ct)
                    .is_ok_and(|heading| heading.is_some())
            }) {
                return MatchConfidence(2);
            }
        }
        rule_confidence(text, &self.chapter_rules).max(rule_confidence(text, &self.volume_rules))
    }

    fn parse(
        &self,
        ct: &CancellationToken,
        content: &ParsedContent,
    ) -> Result<TocRoot, ParserError> {
        check_cancelled(ct)?;
        let Content::Text(text) = &content.content;
        if matches!(self.config.volumes, VolumeMode::FromChapterTitles { .. }) {
            let mut events = self.scan_inline(text, ct)?;
            nest_chapters(&mut events, ct)?;
            return build_toc(events, ct);
        }
        let mut events = self.scan_chapters(text, ct)?;
        match &self.config.volumes {
            VolumeMode::None => {
                for event in &mut events {
                    check_cancelled(ct)?;
                    event.level = 1;
                }
            }
            VolumeMode::Forced {
                chapters_per_volume,
            } => {
                events = group_chapters(events, *chapters_per_volume, ct)?;
            }
            VolumeMode::Normal {
                fallback_chapters_per_volume,
                ..
            } => {
                let volumes = scan_lines(text, &self.volume_rules, ct)?;
                if volumes.is_empty() {
                    if let Some(size) = fallback_chapters_per_volume {
                        events = group_chapters(events, *size, ct)?;
                    } else {
                        nest_chapters(&mut events, ct)?;
                    }
                } else {
                    events.extend(volumes);
                    events.sort_by_key(|event| (event.range.unwrap().start, event.level));
                    // A volume wins when chapter and volume rules match the same line.
                    events.dedup_by_key(|event| event.range.unwrap().start);
                    nest_chapters(&mut events, ct)?;
                }
            }
            VolumeMode::FromChapterTitles { .. } => unreachable!(),
        }
        build_toc(events, ct)
    }
}

fn compile_rules(
    level: usize,
    rules: &[HeadingRuleConfig],
) -> Result<Vec<LineRule>, TocConfigError> {
    rules
        .iter()
        .map(|rule| LineRule::new(level, rule))
        .collect()
}

fn validate_group_size(size: Option<usize>) -> Result<(), TocConfigError> {
    if size == Some(0) {
        return Err(TocConfigError::Invalid(
            "chapters per volume must be positive".into(),
        ));
    }
    Ok(())
}

fn heading(level: usize, title: &str, start: u64, end: u64) -> TocEvent {
    TocEvent {
        level,
        title: title.to_string(),
        range: Some(TextRange::new(start, end).expect("heading range is ordered")),
    }
}

fn nest_chapters(events: &mut [TocEvent], ct: &CancellationToken) -> Result<(), ParserError> {
    let mut has_volume = false;
    for event in events {
        check_cancelled(ct)?;
        if event.level == 1 {
            has_volume = true;
        } else if !has_volume {
            // VBook-style grouping keeps chapters before the first volume at
            // the root, unlike the level parser's anonymous missing ancestors.
            event.level = 1;
        }
    }
    Ok(())
}

fn group_chapters(
    chapters: Vec<TocEvent>,
    size: usize,
    ct: &CancellationToken,
) -> Result<Vec<TocEvent>, ParserError> {
    let mut events = Vec::new();
    for (index, mut chapter) in chapters.into_iter().enumerate() {
        check_cancelled(ct)?;
        if index % size == 0 {
            events.push(TocEvent {
                level: 1,
                title: format!("第 {} 卷", index / size + 1),
                range: None,
            });
        }
        chapter.level = 2;
        events.push(chapter);
    }
    Ok(events)
}
