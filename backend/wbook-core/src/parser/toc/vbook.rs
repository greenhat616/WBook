use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::config::{HeadingRuleConfig, NumeralStyle, SimpleRuleConfig, TocConfigError};
use super::leveled::rule_confidence;
use super::presets::MAX_TITLE_LEN;
use super::rule::{build_toc, scan_lines, LineRule};
use crate::document::TextView;
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

// Some exports prefix every chapter with its volume, as in "第一学年 : 第二章 标题",
// where the volume name carries no volume mark. The colon only proposes a
// split; the parts must still read as a volume and a chapter.
const PREFIX_SEPARATORS: [char; 2] = [':', '：'];
// A prefix must recur before it becomes a volume, so one-off prefixes, such as
// volume names corrupted by injected noise, never start a volume of their own.
const MIN_PREFIX_LINES: usize = 3;
// Volumes without an ordinal that commonly share the chapter prefix format.
const PREFIX_VOLUME_WORDS: [&str; 3] = ["附录", "番外", "外传"];

pub struct VBookTocParser {
    config: VBookConfig,
    chapter_rules: Vec<LineRule>,
    volume_rules: Vec<LineRule>,
    ordinal: LineRule,
}

struct Prefixed<'a> {
    /// Byte offset of the separator within the line.
    split: usize,
    volume: &'a str,
    chapter: &'a str,
}

fn prefixed_splits(line: &str) -> impl Iterator<Item = Prefixed<'_>> {
    line.char_indices()
        .filter(|(_, c)| PREFIX_SEPARATORS.contains(c))
        .filter_map(move |(split, c)| {
            let volume = line[..split].trim();
            let chapter = line[split + c.len_utf8()..].trim();
            // Chapter titles here may carry injected noise, so they get more
            // room than the heading rules allow; body lines are still longer.
            (!volume.is_empty()
                && !chapter.is_empty()
                && volume.chars().count() <= MAX_TITLE_LEN
                && chapter.chars().count() <= 2 * MAX_TITLE_LEN)
                .then_some(Prefixed {
                    split,
                    volume,
                    chapter,
                })
        })
}

impl VBookTocParser {
    pub fn from_config(config: &VBookConfig) -> Result<Self, TocConfigError> {
        let chapter_rules = match &config.chapters {
            ChapterMode::Rules(rules) => compile_rules(2, rules)?,
            ChapterMode::EndMarker { marker } => {
                if marker.trim().is_empty() || marker.contains(['\r', '\n']) {
                    return Err(TocConfigError::Invalid {
                        message: "chapter end marker must be a nonempty single line".into(),
                    });
                }
                if matches!(config.volumes, VolumeMode::FromChapterTitles { .. }) {
                    return Err(TocConfigError::Invalid {
                        message: "splitting volume prefixes requires chapter heading rules".into(),
                    });
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
        // Volume names such as "第一学年" use an ordinal without a volume mark.
        let ordinal = LineRule::new(
            1,
            &HeadingRuleConfig::Simple(SimpleRuleConfig {
                allow_leading_space: true,
                prefixes: vec!["第".into()],
                numeral: NumeralStyle::Mixed,
                suffixes: vec![],
                min_numeral_len: 1,
                max_numeral_len: Some(9),
                max_title_len: MAX_TITLE_LEN,
            }),
        )?;
        Ok(Self {
            config: config.clone(),
            chapter_rules,
            volume_rules,
            ordinal,
        })
    }

    fn chapter_title<'a>(&self, line: &'a str) -> Option<&'a str> {
        self.chapter_rules
            .iter()
            .find_map(|rule| rule.match_title(line))
    }

    fn volume_title<'a>(&self, line: &'a str) -> Option<&'a str> {
        self.volume_rules
            .iter()
            .find_map(|rule| rule.match_title(line))
    }

    fn volume_like(&self, prefix: &str) -> bool {
        PREFIX_VOLUME_WORDS.contains(&prefix)
            || self.ordinal.match_title(prefix).is_some()
            || self.volume_title(prefix).is_some()
    }

    /// Collects the prefixes that name volumes. The format counts only when
    /// at least one prefixed line carries a real chapter heading, which keeps
    /// recurring labels in ordinary text from turning into volumes.
    fn volume_prefixes(
        &self,
        text: TextView<'_>,
        ct: &CancellationToken,
    ) -> Result<HashSet<String>, ParserError> {
        let mut counts = HashMap::<String, usize>::new();
        let mut has_chapter = false;
        for line in text.lines(ct) {
            let line = line?;
            let line = line.text();
            let mut seen = HashSet::new();
            for prefixed in prefixed_splits(line) {
                if !self.volume_like(prefixed.volume) || !seen.insert(prefixed.volume) {
                    continue;
                }
                *counts.entry(prefixed.volume.to_owned()).or_default() += 1;
                has_chapter |= self.chapter_title(prefixed.chapter).is_some();
            }
        }
        check_cancelled(ct)?;
        if !has_chapter {
            return Ok(HashSet::new());
        }
        Ok(counts
            .into_iter()
            .filter(|(_, count)| *count >= MIN_PREFIX_LINES)
            .map(|(prefix, _)| prefix)
            .collect())
    }

    fn scan_prefixed(
        &self,
        text: TextView<'_>,
        prefixes: &HashSet<String>,
        ct: &CancellationToken,
    ) -> Result<Vec<TocEvent>, ParserError> {
        let mut events = Vec::new();
        let mut current_volume = String::new();
        for line in text.lines(ct) {
            let line = line?;
            let start = line.range.start;
            let line = line.text();
            let end = start + line.len() as u64;
            let prefixed =
                prefixed_splits(line).find(|prefixed| prefixes.contains(prefixed.volume));
            if let Some(prefixed) = prefixed {
                let mut chapter_start = start;
                if prefixed.volume != current_volume {
                    let split = start + prefixed.split as u64;
                    events.push(heading(1, prefixed.volume, start, split));
                    current_volume = prefixed.volume.to_owned();
                    chapter_start = split;
                }
                events.push(heading(2, prefixed.chapter, chapter_start, end));
            } else if let Some(title) = self.volume_title(line) {
                events.push(heading(1, title, start, end));
                current_volume = title.to_owned();
            } else if let Some(title) = self.chapter_title(line).or_else(|| {
                // A chapter whose volume prefix is corrupted stays in the
                // current volume.
                prefixed_splits(line).find_map(|prefixed| self.chapter_title(prefixed.chapter))
            }) {
                events.push(heading(2, title, start, end));
            }
        }
        check_cancelled(ct)?;
        Ok(events)
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
        text: TextView<'_>,
        ct: &CancellationToken,
    ) -> Result<Vec<TocEvent>, ParserError> {
        let mut events = Vec::new();
        let mut current_volume = String::new();
        for line in text.lines(ct) {
            let line = line?;
            let start = line.range.start;
            let line = line.text();
            if let Some((split, volume, chapter)) = self.inline_heading(line, ct)? {
                let mut chapter_start = start;
                if volume != current_volume {
                    events.push(heading(1, volume, start, start + split as u64));
                    current_volume = volume.to_string();
                    chapter_start += split as u64;
                }
                events.push(heading(
                    2,
                    chapter,
                    chapter_start,
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
        text: TextView<'_>,
        ct: &CancellationToken,
    ) -> Result<Vec<TocEvent>, ParserError> {
        match &self.config.chapters {
            ChapterMode::Rules(_) => scan_lines(text, &self.chapter_rules, ct),
            ChapterMode::EndMarker { marker } => {
                let mut events = Vec::new();
                let mut at_start = true;
                for line in text.lines(ct) {
                    let line = line?;
                    let start = line.range.start;
                    let line = line.text();
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

    fn accept(&self, content: TextView<'_>) -> MatchConfidence {
        let text = content;
        let ct = CancellationToken::new();
        if let ChapterMode::EndMarker { marker } = &self.config.chapters {
            return MatchConfidence(
                if text
                    .lines(&ct)
                    .take(2000)
                    .any(|line| line.is_ok_and(|line| line.text().trim() == marker.trim()))
                {
                    2
                } else {
                    0
                },
            );
        }
        if matches!(self.config.volumes, VolumeMode::FromChapterTitles { .. }) {
            let ct = CancellationToken::new();
            if text.lines(&ct).take(2000).any(|line| {
                line.is_ok_and(|line| {
                    self.inline_heading(line.text(), &ct)
                        .is_ok_and(|heading| heading.is_some())
                })
            }) {
                return MatchConfidence(2);
            }
        }
        rule_confidence(text, &self.chapter_rules).max(rule_confidence(text, &self.volume_rules))
    }

    fn parse(&self, ct: &CancellationToken, content: TextView<'_>) -> Result<TocRoot, ParserError> {
        check_cancelled(ct)?;
        let text = content;
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
                let prefixes = self.volume_prefixes(text, ct)?;
                if !prefixes.is_empty() {
                    events = self.scan_prefixed(text, &prefixes, ct)?;
                    nest_chapters(&mut events, ct)?;
                    return build_toc(events, ct);
                }
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
        return Err(TocConfigError::Invalid {
            message: "chapters per volume must be positive".into(),
        });
    }
    Ok(())
}

fn heading(level: usize, title: &str, start: u64, end: u64) -> TocEvent {
    TocEvent {
        range_kind: crate::toc::TocRangeKind::Heading,
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
                range_kind: crate::toc::TocRangeKind::Container,
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
