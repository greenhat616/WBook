use tokio_util::sync::CancellationToken;

use super::{
    chapter_only, chapter_only_config, chapter_rules, digit_chapter_rule, easy_pub_config,
    scan_lines, vbook_config, volume_and_chapter, volume_and_chapter_config, volume_rules,
    ChapterMode, HeadingRuleConfig, LevelRulesConfig, LineRule, NumeralStyle, PatternRuleConfig,
    RuleSetTocParser, SimpleRuleConfig, SplitEvenlyParser, TocConfigError, TocParserConfig,
    TocRulesConfig, VBookTocParser, VolumeMode,
};
use crate::extractor::{Content, Encoding, ParsedContent};
use crate::parser::{MatchConfidence, ParserError, TocParser};
use crate::toc::{TocRoot, TocSnapshot};
use crate::types::TextRange;

fn content(text: &str) -> ParsedContent {
    ParsedContent {
        encoding: Encoding {
            name: "utf-8".to_string(),
            bom: false,
        },
        content: Content::Text(text.to_string()),
        source_path: None,
    }
}

fn parse(parser: &dyn TocParser, text: &str) -> TocRoot {
    parser
        .parse(&CancellationToken::new(), &content(text))
        .unwrap()
}

#[test]
fn scan_lines_first_matching_rule_wins() {
    let rules = [regex_rule(2, r"^第.+章", None), regex_rule(1, r"^第", None)];
    let events = scan_lines("第一章 a\n第 1 行\n", &rules, &CancellationToken::new()).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].level, 2);
    assert_eq!(events[1].level, 1);
}

#[test]
fn scan_lines_uses_capture_group_as_title() {
    let rules = [regex_rule(1, r"^序:(.+)$", Some(1))];
    let events = scan_lines("序:前言\n正文\n", &rules, &CancellationToken::new()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title, "前言");
}

#[test]
fn scan_lines_reports_byte_offsets_for_multibyte_text() {
    let rules = [regex_rule(1, r"^第.+章", None)];
    let text = "说明\n第一章 始\n";
    let events = scan_lines(text, &rules, &CancellationToken::new()).unwrap();
    assert_eq!(events.len(), 1);
    let range = events[0].range.unwrap();
    assert_eq!(range, TextRange::new(7, 20).unwrap());
    assert_eq!(&text[range.start as usize..range.end as usize], "第一章 始");
}

#[test]
fn scan_lines_stops_when_cancelled() {
    let rules = [regex_rule(1, r".", None)];
    let ct = CancellationToken::new();
    ct.cancel();
    assert!(matches!(
        scan_lines("a\nb\n", &rules, &ct),
        Err(ParserError::Cancelled)
    ));
}

#[test]
fn volume_and_chapter_builds_two_level_tree() {
    let text = "第一卷 风起\n第一章 少年\n第二章 上山\n第二卷 云动\n第一章 归来\n";
    let parser = volume_and_chapter();
    assert!(parser.accept(&content(text)) > MatchConfidence::NONE);
    let root = parse(&parser, text);
    let snapshot = TocSnapshot::from(&root);

    assert_eq!(snapshot.len(), 2);
    let volume1 = &snapshot[0];
    assert_eq!(volume1.title, "第一卷 风起");
    assert_eq!(volume1.children.len(), 2);
    let chapter1 = &volume1.children[0];
    assert_eq!(chapter1.title, "第一章 少年");
    let chapter1_range = chapter1.meta.range.unwrap();
    assert_eq!(
        &text[chapter1_range.start as usize..chapter1_range.end as usize],
        "第一章 少年"
    );
    // The volume range covers its own heading line.
    let volume1_range = volume1.meta.range.unwrap();
    assert_eq!(
        &text[volume1_range.start as usize..volume1_range.end as usize],
        "第一卷 风起"
    );

    let volume2 = &snapshot[1];
    assert_eq!(volume2.title, "第二卷 云动");
    assert_eq!(volume2.children.len(), 1);
}

#[test]
fn rule_set_accept_is_none_without_hits() {
    let parser = volume_and_chapter();
    assert_eq!(
        parser.accept(&content("hello\nworld\n")),
        MatchConfidence::NONE
    );
}

#[test]
fn chapter_only_puts_everything_at_level_one() {
    let root = parse(&chapter_only(), "前言\n第一章 a\n第二章 b\n");
    let snapshot = TocSnapshot::from(&root);
    assert_eq!(snapshot.len(), 3);
    assert_eq!(snapshot[0].title, "前言");
}

#[test]
fn split_evenly_aligns_parts_to_char_boundaries() {
    // Each output span must remain safe to slice as UTF-8.
    let text = "中中中中";
    let root = parse(&SplitEvenlyParser::new(3).unwrap(), text);
    let snapshot = TocSnapshot::from(&root);
    assert_eq!(snapshot.len(), 3);
    let mut recovered = String::new();
    let mut prev_end = 0u64;
    for (i, entry) in snapshot.iter().enumerate() {
        assert_eq!(entry.title, format!("第 {} 部分", i + 1));
        let range = entry.meta.range.unwrap();
        assert_eq!(range.start, prev_end);
        recovered.push_str(&text[range.start as usize..range.end as usize]);
        prev_end = range.end;
    }
    assert_eq!(recovered, text);
}

#[test]
fn split_evenly_empty_text_yields_no_parts() {
    let root = parse(&SplitEvenlyParser::new(3).unwrap(), "");
    assert!(TocSnapshot::from(&root).is_empty());
}

#[test]
fn volume_and_chapter_parses_bare_headings() {
    let text = "卷一 风起\n第一章 少年\n卷二 云动\n章一 归来\n";
    let parser = volume_and_chapter();
    assert!(parser.accept(&content(text)) > MatchConfidence::NONE);
    let root = parse(&parser, text);
    let snapshot = TocSnapshot::from(&root);

    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0].title, "卷一 风起");
    assert_eq!(snapshot[0].children.len(), 1);
    assert_eq!(snapshot[0].children[0].title, "第一章 少年");
    assert_eq!(snapshot[1].title, "卷二 云动");
    assert_eq!(snapshot[1].children.len(), 1);
    assert_eq!(snapshot[1].children[0].title, "章一 归来");
}

#[test]
fn chapter_only_parses_bare_chapter() {
    let root = parse(&chapter_only(), "章一 a\n章二 b\n");
    let snapshot = TocSnapshot::from(&root);
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0].title, "章一 a");
    assert_eq!(snapshot[1].title, "章二 b");
}

#[test]
fn custom_config_drives_parser() {
    let config = level_config(vec![pattern(r"^序:(.+)$", Some(1))]);
    let parser = RuleSetTocParser::from_config("custom", &config).unwrap();
    let root = parse(&parser, "序:前言\n正文\n");
    let snapshot = TocSnapshot::from(&root);
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].title, "前言");
}

#[test]
fn numeral_style_restricts_serial_number() {
    let mut config = volume_and_chapter_config();
    for rule in simple_rules_mut(&mut config) {
        rule.numeral = NumeralStyle::Arabic;
    }
    let parser = RuleSetTocParser::from_config("arabic", &config).unwrap();
    let root = parse(&parser, "第1卷 a\n第2章 b\n第一章 c\n");
    let snapshot = TocSnapshot::from(&root);
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].title, "第1卷 a");
    assert_eq!(snapshot[0].children.len(), 1);
    assert_eq!(snapshot[0].children[0].title, "第2章 b");
}

#[test]
fn config_roundtrips_through_json() {
    let config = volume_and_chapter_config();
    let json = serde_json::to_string(&config).unwrap();
    let back: TocRulesConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, back);
}

#[test]
fn simple_rule_affixes_are_literals() {
    let rule = SimpleRuleConfig {
        allow_leading_space: false,
        prefixes: vec!["^".into(), "-".into(), "]".into()],
        max_numeral_len: Some(9),
        ..digit_chapter_rule()
    };
    let config = level_config(vec![HeadingRuleConfig::Simple(rule)]);
    let rules = config.compile().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].match_title("^12 title"), Some("^12 title"));
    assert!(rules[0].match_title("x12 title").is_none());
}

#[test]
fn regression_bare_heading_respects_full_numeral_length() {
    let mut config = volume_and_chapter_config();
    for rule in simple_rules_mut(&mut config) {
        rule.max_numeral_len = Some(1);
    }
    let parser = RuleSetTocParser::from_config("limited", &config).unwrap();
    assert!(TocSnapshot::from(&parse(&parser, "卷十二 标题\n")).is_empty());
    assert_eq!(TocSnapshot::from(&parse(&parser, "卷一风起\n")).len(), 1);
}

#[test]
fn regression_confidence_does_not_wrap() {
    for count in [255, 256, 257, 512] {
        assert_eq!(
            volume_and_chapter().accept(&content(&"第一章 标题\n".repeat(count))),
            MatchConfidence(100)
        );
    }
}

#[test]
fn regression_cancelled_rule_parser_never_succeeds() {
    let ct = CancellationToken::new();
    ct.cancel();
    for text in ["", "正文", "第一章 标题"] {
        assert!(matches!(
            volume_and_chapter().parse(&ct, &content(text)),
            Err(ParserError::Cancelled)
        ));
    }
}

#[test]
fn regression_single_heading_beats_split_fallback() {
    use crate::parser::{CombineStrategy, CombinedParser};
    let mut parser = CombinedParser::new(CombineStrategy::BestMatch);
    parser
        .add(SplitEvenlyParser::new(1).unwrap())
        .add(chapter_only());
    let root = parse(&parser, "第一章 开始\n正文");
    assert_eq!(TocSnapshot::from(&root)[0].title, "第一章 开始");
}

fn pattern(regex: &str, title_group: Option<usize>) -> HeadingRuleConfig {
    HeadingRuleConfig::Regex(PatternRuleConfig {
        pattern: regex.into(),
        title_group,
    })
}

fn regex_rule(level: usize, regex: &str, group: Option<usize>) -> LineRule {
    LineRule::new(level, &pattern(regex, group)).unwrap()
}

fn level_config(rules: Vec<HeadingRuleConfig>) -> TocRulesConfig {
    TocRulesConfig {
        levels: vec![LevelRulesConfig { level: 1, rules }],
    }
}

fn simple_rules_mut(config: &mut TocRulesConfig) -> impl Iterator<Item = &mut SimpleRuleConfig> {
    config
        .levels
        .iter_mut()
        .flat_map(|level| &mut level.rules)
        .filter_map(|rule| match rule {
            HeadingRuleConfig::Simple(config) => Some(config),
            _ => None,
        })
}

#[test]
fn levels_support_three_depths_and_parent_priority() {
    let config = TocRulesConfig {
        levels: vec![
            LevelRulesConfig {
                level: 3,
                rules: vec![pattern(r"^节", None)],
            },
            LevelRulesConfig {
                level: 2,
                rules: vec![pattern(r"^(卷|章)", None)],
            },
            LevelRulesConfig {
                level: 1,
                rules: vec![pattern(r"^卷", None)],
            },
        ],
    };
    let parser = RuleSetTocParser::from_config("levels", &config).unwrap();
    let entries = TocSnapshot::from(&parse(&parser, "卷一\n章一\n节一\n卷三\n章二\n"));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].title, "卷一");
    assert_eq!(entries[0].children[0].title, "章一");
    assert_eq!(entries[0].children[0].children[0].title, "节一");
    assert_eq!(entries[1].title, "卷三");
}

#[test]
fn rules_within_a_level_keep_the_configured_order() {
    let config = level_config(vec![pattern(r"^序:(.+)$", Some(1)), pattern(r"^序:", None)]);
    let parser = RuleSetTocParser::from_config("ordered", &config).unwrap();
    assert_eq!(
        TocSnapshot::from(&parse(&parser, "序:前言"))[0].title,
        "前言"
    );
}

#[test]
fn ordinary_regex_groups_do_not_change_titles() {
    let config = level_config(vec![pattern(r"^(第一章|第二章).*$", None)]);
    let parser = RuleSetTocParser::from_config("regex", &config).unwrap();
    assert_eq!(
        TocSnapshot::from(&parse(&parser, "第一章 完整标题"))[0].title,
        "第一章 完整标题"
    );
    assert!(regex_rule(1, r"^序(?::(.+))?$", Some(1))
        .match_title("序")
        .is_none());
}

#[test]
fn simple_rules_support_literal_words_and_character_limits() {
    let rule = SimpleRuleConfig {
        prefixes: vec!["正文第".into()],
        suffixes: vec!["章节".into()],
        max_numeral_len: Some(2),
        max_title_len: 2,
        ..digit_chapter_rule()
    };
    let compiled = LineRule::new(1, &HeadingRuleConfig::Simple(rule.clone())).unwrap();
    assert_eq!(
        compiled.match_title("  正文第12章节  风起  "),
        Some("正文第12章节  风起")
    );
    for rejected in [
        "正文第123章节 风起",
        "正文第12章节 风起云",
        "正文第1章 风起",
        "正1章节 风起",
    ] {
        assert!(compiled.match_title(rejected).is_none(), "{rejected}");
    }
    let strict = LineRule::new(
        1,
        &HeadingRuleConfig::Simple(SimpleRuleConfig {
            allow_leading_space: false,
            ..rule
        }),
    )
    .unwrap();
    assert!(strict.match_title(" 正文第1章节 风起").is_none());
    assert!(strict.match_title("正文第1章节 风起").is_some());
}

#[test]
fn digit_headings_support_unlimited_and_exact_serial_width() {
    let mut rule = digit_chapter_rule();
    let unlimited = LineRule::new(1, &HeadingRuleConfig::Simple(rule.clone())).unwrap();
    assert!(unlimited.match_title("1234567890123：标题").is_some());
    rule.min_numeral_len = 3;
    rule.max_numeral_len = Some(3);
    rule.suffixes = vec!["：".into()];
    let exact = LineRule::new(1, &HeadingRuleConfig::Simple(rule)).unwrap();
    assert!(exact.match_title("001：标题").is_some());
    for rejected in ["1：标题", "0001：标题", "001标题", "001二标题"] {
        assert!(exact.match_title(rejected).is_none(), "{rejected}");
    }
}

#[test]
fn vbook_normal_keeps_real_volumes_and_root_chapters() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::Normal {
        rules: volume_rules(),
        fallback_chapters_per_volume: Some(1),
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let entries = TocSnapshot::from(&parse(
        &parser,
        "第一章 卷前\n第二章 卷前二\n卷一\n第三章 甲\n第四章 乙\n卷三\n第五章 丙\n卷四\n",
    ));
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[0].title, "第一章 卷前");
    assert_eq!(entries[1].title, "第二章 卷前二");
    assert_eq!(entries[2].title, "卷一");
    assert_eq!(entries[2].children.len(), 2);
    assert_eq!(entries[3].title, "卷三");
    assert_eq!(entries[3].children.len(), 1);
    assert!(entries[4].children.is_empty());
}

#[test]
fn vbook_normal_fallback_groups_only_when_no_volumes_exist() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::Normal {
        rules: volume_rules(),
        fallback_chapters_per_volume: Some(2),
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let entries = TocSnapshot::from(&parse(&parser, "第一章 甲\n第二章 乙\n第三章 丙"));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].children.len(), 2);
    assert_eq!(entries[1].children.len(), 1);
    assert!(entries[0].meta.range.is_some());
    assert!(TocSnapshot::from(&parse(&parser, "正文没有标题")).is_empty());
    config.volumes = VolumeMode::Normal {
        rules: volume_rules(),
        fallback_chapters_per_volume: None,
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let entries = TocSnapshot::from(&parse(&parser, "第一章 甲\n第二章 乙"));
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.children.is_empty()));
}

#[test]
fn vbook_forced_uses_chapter_count_despite_existing_volume_lines() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::Forced {
        chapters_per_volume: 2,
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let entries = TocSnapshot::from(&parse(
        &parser,
        "卷一\n第一章 甲\n卷三\n第二章 乙\n第三章 丙",
    ));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].title, "第 1 卷");
    assert_eq!(entries[0].children.len(), 2);
    assert_eq!(entries[1].children[0].title, "第三章 丙");
}

#[test]
fn vbook_none_keeps_all_detected_headings_flat() {
    let mut config = vbook_config();
    let mut rules = volume_rules();
    rules.extend(chapter_rules());
    config.chapters = ChapterMode::Rules(rules);
    config.volumes = VolumeMode::None;
    let parser = VBookTocParser::from_config(&config).unwrap();
    let entries = TocSnapshot::from(&parse(&parser, "卷一\n第一章 甲\n卷三\n第二章 乙"));
    assert_eq!(entries.len(), 4);
    assert!(entries.iter().all(|entry| entry.children.is_empty()));
}

#[test]
fn vbook_inline_volumes_remove_repeated_prefixes_and_keep_offsets() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::FromChapterTitles {
        rules: volume_rules(),
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let text = "第一章 卷前\r\n第一卷 人生若只如初见 第1章 苏铭\r\n第一卷 人生若只如初见 第2章 蛮启\r\n卷三 风起 章一 开始\r\n章二 继续";
    assert!(parser.accept(&content(text)) > MatchConfidence::NONE);
    let entries = TocSnapshot::from(&parse(&parser, text));
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].title, "第一章 卷前");
    assert_eq!(entries[1].title, "第一卷 人生若只如初见");
    assert_eq!(entries[1].children.len(), 2);
    assert_eq!(entries[1].children[0].title, "第1章 苏铭");
    assert_eq!(entries[1].children[1].title, "第2章 蛮启");
    assert_eq!(entries[2].title, "卷三 风起");
    assert_eq!(entries[2].children.len(), 2);
    let range = entries[1].children[1].meta.range.unwrap();
    assert_eq!(
        text[range.start as usize..range.end as usize].trim(),
        "第2章 蛮启"
    );
}

#[test]
fn vbook_inline_accepts_shared_regex_rules() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::FromChapterTitles {
        rules: vec![pattern(r"^Book [A-Z]$", None)],
    };
    config.chapters = ChapterMode::Rules(vec![pattern(r"^Chapter \d+ .+$", None)]);
    let parser = VBookTocParser::from_config(&config).unwrap();
    let entries = TocSnapshot::from(&parse(
        &parser,
        "Book A Chapter 1 Start\nBook A Chapter 2 End",
    ));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "Book A");
    assert_eq!(entries[0].children.len(), 2);
}

#[test]
fn end_marker_skips_empty_segments_and_keeps_unterminated_tail() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::None;
    config.chapters = ChapterMode::EndMarker {
        marker: "------------".into(),
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let text = "\r\n------------\r\n 标题甲 \r\n正文\r\n ------------ \r\n------------\r\n标题乙\r\n尾部正文";
    let entries = TocSnapshot::from(&parse(&parser, text));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].title, "标题甲");
    assert_eq!(entries[1].title, "标题乙");
    let range = entries[0].meta.range.unwrap();
    assert_eq!(&text[range.start as usize..range.end as usize], " 标题甲 ");
    assert!(TocSnapshot::from(&parse(&parser, "\n------------\n")).is_empty());
}

#[test]
fn end_marker_works_with_normal_volume_detection() {
    let mut config = vbook_config();
    config.chapters = ChapterMode::EndMarker {
        marker: "---".into(),
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let entries = TocSnapshot::from(&parse(
        &parser,
        "卷一\n开端\n正文\n---\n继续\n正文\n---\n卷三\n归来\n正文",
    ));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].children.len(), 2);
    assert_eq!(entries[0].children[0].title, "开端");
    assert_eq!(entries[1].children[0].title, "归来");
}

#[test]
fn easy_pub_rules_and_presets_can_be_customized_independently() {
    let original = easy_pub_config();
    let mut modified = original.clone();
    for rule in simple_rules_mut(&mut modified) {
        rule.numeral = NumeralStyle::Arabic;
    }
    let parser = RuleSetTocParser::from_config("easy-pub", &original).unwrap();
    let entries = TocSnapshot::from(&parse(&parser, "简介\n第一章 开始\n卷三\n第二回 继续"));
    assert_eq!(entries.len(), 4);
    assert!(entries.iter().all(|entry| entry.children.is_empty()));
    let parser = RuleSetTocParser::from_config("modified", &modified).unwrap();
    assert!(TocSnapshot::from(&parse(&parser, "第一章 开始")).is_empty());
}

#[test]
fn parser_configs_roundtrip_and_build_a_single_trait_object() {
    for config in [
        TocParserConfig::Levels(volume_and_chapter_config()),
        TocParserConfig::Levels(easy_pub_config()),
        TocParserConfig::VBook(vbook_config()),
        TocParserConfig::SplitEvenly { parts: 3 },
    ] {
        let json = serde_json::to_string(&config).unwrap();
        let restored: TocParserConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, config);
        assert!(!TocSnapshot::from(&parse(
            config.build().unwrap().as_ref(),
            "卷一\n第一章 开始"
        ))
        .is_empty());
    }
}

#[test]
fn invalid_configuration_returns_errors_instead_of_panicking() {
    assert!(matches!(
        SplitEvenlyParser::new(0),
        Err(TocConfigError::Invalid(_))
    ));
    assert!(TocParserConfig::SplitEvenly { parts: 0 }.build().is_err());
    for (regex, capture) in [("[", None), ("^章$", Some(1))] {
        assert!(level_config(vec![pattern(regex, capture)])
            .compile()
            .is_err());
    }
    let mut config = chapter_only_config();
    config.levels[0].level = 0;
    assert!(config.compile().is_err());
    config.levels[0].level = 1;
    config.levels.push(config.levels[0].clone());
    assert!(config.compile().is_err());
    for (min, max) in [(0, None), (1, Some(0)), (3, Some(2))] {
        let rule = SimpleRuleConfig {
            min_numeral_len: min,
            max_numeral_len: max,
            ..digit_chapter_rule()
        };
        assert!(LineRule::new(1, &HeadingRuleConfig::Simple(rule)).is_err());
    }
    for affix in ["", "第\n"] {
        let rule = SimpleRuleConfig {
            prefixes: vec![affix.into()],
            ..digit_chapter_rule()
        };
        assert!(LineRule::new(1, &HeadingRuleConfig::Simple(rule)).is_err());
    }
    let mut config = vbook_config();
    for volumes in [
        VolumeMode::Forced {
            chapters_per_volume: 0,
        },
        VolumeMode::Normal {
            rules: volume_rules(),
            fallback_chapters_per_volume: Some(0),
        },
    ] {
        config.volumes = volumes;
        assert!(VBookTocParser::from_config(&config).is_err());
    }
    config.volumes = VolumeMode::None;
    for marker in ["", " \t", "a\nb"] {
        config.chapters = ChapterMode::EndMarker {
            marker: marker.into(),
        };
        assert!(VBookTocParser::from_config(&config).is_err());
    }
    config.chapters = ChapterMode::EndMarker {
        marker: "---".into(),
    };
    config.volumes = VolumeMode::FromChapterTitles {
        rules: volume_rules(),
    };
    assert!(VBookTocParser::from_config(&config).is_err());
}

#[test]
fn split_evenly_balances_characters_and_bounds_part_count() {
    for (text, count) in [("中a中a中a中", 3), ("ab", usize::MAX), ("", 3)] {
        let parser = SplitEvenlyParser::new(count).unwrap();
        let entries = TocSnapshot::from(&parse(&parser, text));
        assert_eq!(entries.len(), count.min(text.chars().count()));
        let mut recovered = String::new();
        let mut sizes = Vec::new();
        for entry in entries {
            let range = entry.meta.range.unwrap();
            let slice = &text[range.start as usize..range.end as usize];
            recovered.push_str(slice);
            sizes.push(slice.chars().count());
        }
        assert_eq!(recovered, text);
        if !sizes.is_empty() {
            assert!(sizes.iter().max().unwrap() - sizes.iter().min().unwrap() <= 1);
        }
    }
}

#[test]
fn cancelled_all_toc_modes_return_the_same_error_even_for_empty_text() {
    let ct = CancellationToken::new();
    ct.cancel();
    let mut configs = vec![
        TocParserConfig::Levels(volume_and_chapter_config()),
        TocParserConfig::SplitEvenly { parts: 2 },
    ];
    for volumes in [
        VolumeMode::None,
        VolumeMode::Forced {
            chapters_per_volume: 2,
        },
        VolumeMode::Normal {
            rules: volume_rules(),
            fallback_chapters_per_volume: Some(2),
        },
        VolumeMode::FromChapterTitles {
            rules: volume_rules(),
        },
    ] {
        let mut config = vbook_config();
        config.volumes = volumes;
        configs.push(TocParserConfig::VBook(config));
    }
    let mut marker = vbook_config();
    marker.chapters = ChapterMode::EndMarker {
        marker: "---".into(),
    };
    configs.push(TocParserConfig::VBook(marker));
    for config in configs {
        let parser = config.build().unwrap();
        for text in ["", "第一章 开始\n正文"] {
            assert!(matches!(
                parser.parse(&ct, &content(text)),
                Err(ParserError::Cancelled)
            ));
        }
    }
}

#[test]
fn cancellation_during_tree_build_discards_partial_results() {
    let ct = CancellationToken::new();
    let events = (0..3).map(|index| {
        if index == 1 {
            ct.cancel();
        }
        crate::toc::TocEvent {
            level: 1,
            title: index.to_string(),
            range: None,
        }
    });
    assert!(matches!(
        super::rule::build_toc(events, &ct),
        Err(ParserError::Cancelled)
    ));
}
