use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::document::tests::document;
use crate::document::TextEdit;
use crate::parser::toc::{
    chapter_only, easy_pub_config, vbook_config, volume_and_chapter_config, volume_rules,
    ChapterMode, TocParserConfig, VolumeMode,
};
use crate::parser::{AdFilterParser, MatchConfidence, SimpleMetadataParser};
use crate::toc::{Toc, TocSnapshot};

struct Rewrite(&'static str, &'static str);

impl FilterParser for Rewrite {
    fn name(&self) -> &'static str {
        "rewrite"
    }
    fn accept(&self, _: TextView<'_>) -> MatchConfidence {
        MatchConfidence(1)
    }
    fn parse(
        &self,
        ct: &CancellationToken,
        view: TextView<'_>,
    ) -> Result<Vec<TextEdit>, ParserError> {
        let mut edits = Vec::new();
        for line in view.lines(ct) {
            let line = line?;
            for (offset, found) in line.raw.match_indices(self.0) {
                edits.push(TextEdit {
                    range: TextRange {
                        start: line.range.start + offset as u64,
                        end: line.range.start + (offset + found.len()) as u64,
                    },
                    insert: self.1.into(),
                });
            }
        }
        Ok(edits)
    }
}

fn all_text(doc: &ProcessingDocument) -> String {
    let view = doc.view();
    view.read(
        &CancellationToken::new(),
        view.version(),
        TextRange {
            start: 0,
            end: view.len(),
        },
    )
    .unwrap()
    .into_owned()
}

#[test]
fn filters_feed_current_text_to_later_rules_toc_and_metadata() {
    let ct = CancellationToken::new();
    let mut doc = ProcessingDocument::new(document(
        "书名：旧名\n作者：网站 www.example.com\n第一回 起点\n正文",
    ));
    let result = doc
        .run(
            &ct,
            &[
                &AdFilterParser::new(),
                &Rewrite("回", "章"),
                &Rewrite("旧名", "新名"),
            ],
            &chapter_only(),
            &SimpleMetadataParser::new(),
        )
        .unwrap();
    assert_eq!(TocSnapshot::from(&result.toc)[0].title, "第一章 起点");
    assert_eq!(result.metadata.title.as_deref(), Some("新名"));
    assert_eq!(result.metadata.author, None);
    assert_eq!(result.version, doc.view().version());
    assert!(!all_text(&doc).contains("www.example.com"));
    assert!(doc.results().is_none());
    doc.install(&ct, result).unwrap();
    assert!(doc.current_results().is_ok());

    for (rules, expected) in [
        ([&Rewrite("a", "b"), &Rewrite("b", "c")], "c"),
        ([&Rewrite("b", "c"), &Rewrite("a", "b")], "b"),
    ] {
        let mut doc = ProcessingDocument::new(document("a"));
        doc.run(
            &ct,
            &[rules[0], rules[1]],
            &chapter_only(),
            &SimpleMetadataParser::new(),
        )
        .unwrap();
        assert_eq!(all_text(&doc), expected);
    }
}

#[test]
fn stale_results_and_plans_are_rejected_without_discarding_manual_work() {
    let ct = CancellationToken::new();
    let mut doc = ProcessingDocument::new(document("第一章 起点\n正文\n"));
    let mut initial = doc
        .parse(&ct, &chapter_only(), &SimpleMetadataParser::new())
        .unwrap();
    let id = TocSnapshot::from(&initial.toc)[0].id;
    initial.toc.get_mut(id).unwrap().title = "Manual title".into();
    doc.install(&ct, initial.clone()).unwrap();
    doc.metadata_overrides.title = Some("Manual book title".into());
    let plan = doc
        .output_plan(
            &ct,
            vec![TextRange {
                start: 0,
                end: doc.view().len(),
            }],
        )
        .unwrap();
    doc.apply(
        &ct,
        EditBatch {
            base: doc.view().version(),
            edits: vec![TextEdit {
                range: TextRange {
                    start: doc.view().len(),
                    end: doc.view().len(),
                },
                insert: "第二章 新增\n".into(),
            }],
        },
    )
    .unwrap();
    assert!(matches!(
        doc.current_results(),
        Err(PipelineError::Document {
            source: DocumentError::StaleVersion { .. }
        })
    ));
    assert_eq!(
        TocSnapshot::from(&doc.results().unwrap().toc)[0].title,
        "Manual title"
    );
    assert!(doc.install(&ct, initial).is_err());
    assert!(doc.write_plan(&ct, &plan, &mut Vec::new()).is_err());
    let candidate = doc
        .parse(&ct, &chapter_only(), &SimpleMetadataParser::new())
        .unwrap();
    assert_eq!(TocSnapshot::from(&candidate.toc).len(), 2);
    assert_eq!(
        TocSnapshot::from(&doc.results().unwrap().toc)[0].title,
        "Manual title"
    );
    doc.install(&ct, candidate).unwrap();
    assert_eq!(
        doc.metadata().unwrap().title.as_deref(),
        Some("Manual book title")
    );
    assert!(doc.write_plan(&ct, &plan, &mut Vec::new()).is_err());
    let other = ProcessingDocument::new(document("第一章"));
    let wrong = other
        .parse(&ct, &chapter_only(), &SimpleMetadataParser::new())
        .unwrap();
    assert!(matches!(
        doc.install(&ct, wrong),
        Err(PipelineError::Document {
            source: DocumentError::WrongDocument
        })
    ));
}

#[test]
fn all_parser_modes_match_contiguous_text_after_fragmentation() {
    let source = "书名：片段测试\r\n作者：甲\n卷一 起始\n第一章 开始\r\n正文\n---\n第二章 继续\n卷二 后续\n第一章 结束\n";
    let mut fragmented = document(source);
    let edits = source
        .char_indices()
        .map(|(offset, ch)| TextEdit {
            range: TextRange {
                start: offset as u64,
                end: (offset + ch.len_utf8()) as u64,
            },
            insert: ch.to_string(),
        })
        .collect();
    let ct = CancellationToken::new();
    fragmented
        .apply(
            &ct,
            EditBatch {
                base: fragmented.version(),
                edits,
            },
        )
        .unwrap();
    let plain = document(source);
    let mut configs = vec![
        TocParserConfig::Levels(volume_and_chapter_config()),
        TocParserConfig::Levels(easy_pub_config()),
        TocParserConfig::SplitEvenly { parts: 7 },
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
        assert_eq!(
            parser.accept(plain.view()),
            parser.accept(fragmented.view())
        );
        assert_eq!(
            TocSnapshot::from(parser.parse(&ct, plain.view()).unwrap()),
            TocSnapshot::from(parser.parse(&ct, fragmented.view()).unwrap())
        );
    }
    let metadata = SimpleMetadataParser::new();
    assert_eq!(
        metadata.parse(&ct, plain.view()).unwrap(),
        metadata.parse(&ct, fragmented.view()).unwrap()
    );
    let ad_source = "正文\n请访问 www.example.com\r\n尾声";
    let mut ad_doc = ProcessingDocument::new(document(ad_source));
    ad_doc
        .run(
            &ct,
            &[&Rewrite("example", "exAMPLE"), &AdFilterParser::new()],
            &chapter_only(),
            &metadata,
        )
        .unwrap();
    assert_eq!(all_text(&ad_doc), "正文\n尾声");
}

struct FailingFilter {
    calls: AtomicUsize,
    cancel: bool,
}
impl FilterParser for FailingFilter {
    fn name(&self) -> &'static str {
        "failure"
    }
    fn accept(&self, _: TextView<'_>) -> MatchConfidence {
        MatchConfidence(1)
    }
    fn parse(&self, ct: &CancellationToken, _: TextView<'_>) -> Result<Vec<TextEdit>, ParserError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.cancel {
            ct.cancel();
            Ok(vec![])
        } else {
            Err(ParserError::Other {
                source: anyhow::anyhow!("intentional parser failure"),
            })
        }
    }
}

#[test]
fn pipeline_failure_and_cancellation_keep_prior_commits_and_stop_following_stages() {
    for cancel in [false, true] {
        let ct = CancellationToken::new();
        let failure = FailingFilter {
            calls: AtomicUsize::new(0),
            cancel,
        };
        let later = FailingFilter {
            calls: AtomicUsize::new(0),
            cancel: false,
        };
        let mut doc = ProcessingDocument::new(document("a"));
        assert!(doc
            .run(
                &ct,
                &[&Rewrite("a", "b"), &failure, &later],
                &chapter_only(),
                &SimpleMetadataParser::new()
            )
            .is_err());
        assert_eq!(all_text(&doc), "b");
        assert_eq!(doc.view().version().revision(), 1);
        assert_eq!(failure.calls.load(Ordering::Relaxed), 1);
        assert_eq!(later.calls.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn preview_and_partitioned_output_share_current_text() {
    let ct = CancellationToken::new();
    let mut doc = ProcessingDocument::new(document("第一章 a\n正文\n第二章 b\n结尾"));
    let result = doc
        .run(
            &ct,
            &[&Rewrite("正文", "新增🙂正文")],
            &chapter_only(),
            &SimpleMetadataParser::new(),
        )
        .unwrap();
    doc.install(&ct, result).unwrap();
    let expected = all_text(&doc);
    let split = expected.find("第二章").unwrap() as u64;
    let ranges = vec![
        TextRange {
            start: 0,
            end: split,
        },
        TextRange {
            start: split,
            end: doc.view().len(),
        },
    ];
    let plan = doc.output_plan(&ct, ranges).unwrap();
    let mut bytes = Vec::new();
    doc.write_plan(&ct, &plan, &mut bytes).unwrap();
    assert_eq!(bytes, expected.as_bytes());
}
