use std::fs;
use std::io::{self, Read, Write};

use super::*;
use crate::document::{EditBatch, TextEdit};
use crate::extractor::{Content, Encoding, ParsedContent};
use crate::parser::toc::{
    chapter_only, vbook_config, volume_and_chapter, volume_rules, SplitEvenlyParser,
    VBookTocParser, VolumeMode,
};
use crate::parser::{SimpleMetadataParser, TocParser};
use crate::toc::{Toc, TocRangeKind, TocRoot, TocSnapshot};
use crate::types::TextRange;

fn options(layout: RenderLayout) -> ExportOptions {
    ExportOptions {
        render: RenderOptions { layout },
        format: OutputFormat::Epub,
        language: "zh-Hans".into(),
        identifier: Some("urn:wbook:test&book".into()),
    }
}

fn book(text: &str, parser: &dyn TocParser) -> ProcessingDocument {
    let ct = CancellationToken::new();
    let mut book = ProcessingDocument::new(
        ParsedContent {
            encoding: Encoding {
                name: "utf-8".into(),
                bom: false,
            },
            content: Content::Text(text.into()),
            source_path: None,
        }
        .into(),
    );
    let results = book.parse(&ct, parser, &SimpleMetadataParser).unwrap();
    book.install(&ct, results).unwrap();
    book.metadata_overrides.title = Some("测试 & <书> \"一\"".into());
    book
}

fn adjust(book: &mut ProcessingDocument, action: impl FnOnce(&mut TocRoot)) {
    let mut results = book.results().unwrap().clone();
    action(&mut results.toc);
    book.install(&CancellationToken::new(), results).unwrap();
}

fn projection(rendered: &RenderedBook) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for file in rendered.content_files() {
        let text = fs::read_to_string(rendered.directory().join(file)).unwrap();
        let xml = roxmltree::Document::parse(&text).unwrap();
        for node in xml.descendants().filter(|n| {
            n.is_element()
                && matches!(
                    n.tag_name().name(),
                    "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                )
        }) {
            values.push((
                node.tag_name().name().into(),
                node.text().unwrap_or("").into(),
            ));
        }
    }
    values
}

fn archive(path: &Path) -> std::collections::HashMap<String, String> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut files = std::collections::HashMap::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        if file.name().ends_with(".xhtml")
            || file.name().ends_with(".xml")
            || file.name().ends_with(".opf")
        {
            roxmltree::Document::parse(&text).unwrap();
        }
        files.insert(file.name().into(), text);
    }
    files
}

#[test]
fn layouts_preserve_edited_content_order_and_epub_navigation() {
    let ct = CancellationToken::new();
    let mut document = book(
        "前文\r\n第一卷 旅程\r\n卷说明\r\n第一章 同名\r\n正文甲\r\n\r\n第二章 同名\r\n正文乙",
        &volume_and_chapter(),
    );
    document.metadata_overrides.author = Some("作者 <甲> & '乙'".into());
    adjust(&mut document, |toc| {
        let volume = toc.root_ids()[0];
        let first = toc.get(volume).unwrap().children[0];
        toc.get_mut(first).unwrap().title = "修改后的章名".into();
        toc.move_down(first);
    });
    let expected = [
        ("p", "前文"),
        ("h1", "第一卷 旅程"),
        ("p", "卷说明"),
        ("h2", "第二章 同名"),
        ("p", "正文乙"),
        ("h2", "修改后的章名"),
        ("p", "正文甲"),
        ("p", ""),
    ];
    let output = tempfile::tempdir().unwrap();
    for layout in [
        RenderLayout::SplitChapters,
        RenderLayout::Paged,
        RenderLayout::SingleHtml,
    ] {
        let rendered = render_book(&ct, &document, &options(layout)).unwrap();
        assert_eq!(
            projection(&rendered),
            expected
                .iter()
                .map(|(a, b)| ((*a).into(), (*b).into()))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            rendered.files.len(),
            if layout == RenderLayout::SplitChapters {
                4
            } else {
                1
            }
        );
        let nav_text = fs::read_to_string(rendered.directory().join("nav.xhtml")).unwrap();
        let nav = roxmltree::Document::parse(&nav_text).unwrap();
        let labels: Vec<_> = nav
            .descendants()
            .filter(|n| n.has_tag_name(("http://www.w3.org/1999/xhtml", "a")))
            .map(|n| n.text().unwrap())
            .collect();
        assert_eq!(labels, ["第一卷 旅程", "第二章 同名", "修改后的章名"]);
        let path = output.path().join(format!("{layout:?}.epub"));
        let artifact = package_epub(&ct, &rendered, &path).unwrap();
        assert_eq!(artifact.identifier, "urn:wbook:test&book");
        let files = archive(&path);
        let opf = roxmltree::Document::parse(&files["EPUB/package.opf"]).unwrap();
        assert_eq!(
            opf.descendants()
                .filter(|n| n.tag_name().name() == "itemref")
                .count(),
            rendered.files.len()
        );
        assert!(opf
            .descendants()
            .any(|n| n.tag_name().name() == "creator" && n.text() == Some("作者 <甲> & '乙'")));
        let all = rendered
            .files
            .iter()
            .map(|p| fs::read_to_string(rendered.directory().join(p)).unwrap())
            .collect::<String>();
        assert_eq!(
            all.matches(" page-break").count(),
            if layout == RenderLayout::Paged { 3 } else { 0 }
        );
    }
}

#[test]
fn escapes_text_without_interpreting_html_or_tera() {
    let text =
        "第一章 <script>\n  a & b <script>\"quoted\" 'apostrophe' {{ book.title }} 中文🙂\n\n尾行";
    let document = book(text, &chapter_only());
    let rendered = render_book(
        &CancellationToken::new(),
        &document,
        &options(RenderLayout::SingleHtml),
    )
    .unwrap();
    assert_eq!(
        projection(&rendered),
        vec![
            ("h1".into(), "第一章 <script>".into()),
            (
                "p".into(),
                "  a & b <script>\"quoted\" 'apostrophe' {{ book.title }} 中文🙂".into()
            ),
            ("p".into(), "".into()),
            ("p".into(), "尾行".into())
        ]
    );
}

#[test]
fn no_toc_and_terminal_newlines_preserve_paragraphs() {
    for (source, expected) in [
        ("a", vec!["a"]),
        ("a\n", vec!["a"]),
        ("a\n\n", vec!["a", ""]),
        ("\r\n正文\r\n", vec!["", "正文"]),
    ] {
        let document = book(source, &chapter_only());
        let rendered = render_book(
            &CancellationToken::new(),
            &document,
            &options(RenderLayout::SingleHtml),
        )
        .unwrap();
        assert_eq!(
            projection(&rendered)
                .into_iter()
                .map(|(_, s)| s)
                .collect::<Vec<_>>(),
            expected
        );
        let output = tempfile::tempdir().unwrap();
        package_epub(
            &CancellationToken::new(),
            &rendered,
            output.path().join("plain.epub"),
        )
        .unwrap();
    }
}

#[test]
fn split_body_ranges_keep_first_lines_and_all_unicode() {
    let document = book("甲乙🙂abcdef", &SplitEvenlyParser::new(3).unwrap());
    let rendered = render_book(
        &CancellationToken::new(),
        &document,
        &options(RenderLayout::SplitChapters),
    )
    .unwrap();
    let body: String = projection(&rendered)
        .into_iter()
        .filter(|(tag, _)| tag == "p")
        .map(|(_, text)| text)
        .collect();
    assert_eq!(body, "甲乙🙂abcdef");
}

#[test]
fn inline_vbook_titles_consume_repeated_volume_prefix_once() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::FromChapterTitles {
        rules: volume_rules(),
    };
    let parser = VBookTocParser::from_config(&config).unwrap();
    let document = book(
        "第一卷 春 第一章 开始\n甲\n第一卷 春 第二章 继续\n乙",
        &parser,
    );
    let rendered = render_book(
        &CancellationToken::new(),
        &document,
        &options(RenderLayout::SingleHtml),
    )
    .unwrap();
    let values: Vec<_> = projection(&rendered).into_iter().map(|(_, s)| s).collect();
    assert_eq!(
        values,
        ["第一卷 春", "第一章 开始", "甲", "第二章 继续", "乙"]
    );
}

#[test]
fn deleting_heading_keeps_its_text_as_body() {
    let mut document = book("第一章 甲\n正文甲\n第二章 乙\n正文乙", &chapter_only());
    adjust(&mut document, |toc| toc.remove(toc.root_ids()[1]));
    let rendered = render_book(
        &CancellationToken::new(),
        &document,
        &options(RenderLayout::SingleHtml),
    )
    .unwrap();
    assert_eq!(
        projection(&rendered)
            .into_iter()
            .map(|(_, s)| s)
            .collect::<Vec<_>>(),
        ["第一章 甲", "正文甲", "第二章 乙", "正文乙"]
    );
}

#[test]
fn synthetic_containers_do_not_duplicate_body() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::Forced {
        chapters_per_volume: 2,
    };
    let document = book(
        "第一章 甲\n甲\n第二章 乙\n乙",
        &VBookTocParser::from_config(&config).unwrap(),
    );
    let rendered = render_book(
        &CancellationToken::new(),
        &document,
        &options(RenderLayout::SingleHtml),
    )
    .unwrap();
    assert_eq!(
        projection(&rendered)
            .into_iter()
            .map(|(_, s)| s)
            .collect::<Vec<_>>(),
        ["第 1 卷", "第一章 甲", "甲", "第二章 乙", "乙"]
    );
}

#[test]
fn stale_results_and_unknown_legacy_ranges_are_rejected() {
    let ct = CancellationToken::new();
    let mut doc = book("第一章 甲\n正文", &chapter_only());
    let mut snapshot =
        serde_json::to_value(TocSnapshot::from(&doc.results().unwrap().toc)).unwrap();
    snapshot[0]["meta"]
        .as_object_mut()
        .unwrap()
        .remove("range_kind");
    let mut results = doc.results().unwrap().clone();
    results.toc = serde_json::from_value(snapshot).unwrap();
    doc.install(&ct, results).unwrap();
    assert!(render_book(&ct, &doc, &options(RenderLayout::SingleHtml))
        .unwrap_err()
        .to_string()
        .contains("unknown range semantics"));
    let base = doc.view().version();
    doc.apply(
        &ct,
        EditBatch {
            base,
            edits: vec![TextEdit {
                range: TextRange { start: 0, end: 0 },
                insert: "前言\n".into(),
            }],
        },
    )
    .unwrap();
    assert!(matches!(
        render_book(&ct, &doc, &options(RenderLayout::SingleHtml))
            .unwrap_err()
            .source,
        ExportFailure::Pipeline(PipelineError::Document(DocumentError::StaleVersion { .. }))
    ));
}

#[test]
fn edited_piece_boundaries_render_the_current_text() {
    let ct = CancellationToken::new();
    let mut doc = book("第一章 甲\r\n旧文", &chapter_only());
    let start = "第一章 甲\r\n".len() as u64;
    doc.apply(
        &ct,
        EditBatch {
            base: doc.view().version(),
            edits: vec![
                TextEdit {
                    range: TextRange {
                        start,
                        end: doc.view().len(),
                    },
                    insert: "新文🙂".into(),
                },
                TextEdit {
                    range: TextRange { start: 3, end: 3 },
                    insert: "".into(),
                },
            ],
        },
    )
    .unwrap();
    let results = doc
        .parse(&ct, &chapter_only(), &SimpleMetadataParser)
        .unwrap();
    doc.install(&ct, results).unwrap();
    let rendered = render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).unwrap();
    assert_eq!(projection(&rendered).last().unwrap().1, "新文🙂");
}

#[test]
fn invalid_structures_fail_before_rendering() {
    for case in ["overlap", "missing", "mixed", "cycle"] {
        let mut doc = book("第一章 甲\n正文\n第二章 乙\n尾文", &chapter_only());
        adjust(&mut doc, |toc| {
            let ids = toc.root_ids().to_vec();
            match case {
                "overlap" => {
                    toc.get_mut(ids[1]).unwrap().meta.range = toc.get(ids[0]).unwrap().meta.range
                }
                "missing" => toc.get_mut(ids[0]).unwrap().meta.range = None,
                "mixed" => toc.get_mut(ids[0]).unwrap().meta.range_kind = TocRangeKind::Body,
                _ => toc.get_mut(ids[0]).unwrap().children.push(ids[0]),
            }
        });
        assert_eq!(
            render_book(
                &CancellationToken::new(),
                &doc,
                &options(RenderLayout::SingleHtml)
            )
            .unwrap_err()
            .stage,
            ExportStage::Planning
        );
    }
    let mut doc = book("abcdef", &SplitEvenlyParser::new(2).unwrap());
    adjust(&mut doc, |toc| {
        toc.get_mut(toc.root_ids()[0])
            .unwrap()
            .meta
            .range
            .as_mut()
            .unwrap()
            .start = 1
    });
    assert!(render_book(
        &CancellationToken::new(),
        &doc,
        &options(RenderLayout::SingleHtml)
    )
    .unwrap_err()
    .to_string()
    .contains("gap"));
}

#[test]
fn invalid_metadata_and_xml_characters_are_reported() {
    let ct = CancellationToken::new();
    for source in ["", "正文\0错误", "正文\u{fffe}"] {
        let doc = book(source, &chapter_only());
        assert!(render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).is_err());
    }
    let mut doc = book("正文", &chapter_only());
    let mut opts = options(RenderLayout::SingleHtml);
    opts.language = "zh_Hans".into();
    assert!(render_book(&ct, &doc, &opts).is_err());
    opts = options(RenderLayout::SingleHtml);
    opts.identifier = Some(" ".into());
    assert!(render_book(&ct, &doc, &opts).is_err());
    doc.metadata_overrides.title = Some(" ".into());
    assert!(render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).is_err());
    assert!(serde_json::from_str::<OutputFormat>("\"Mobi\"").is_err());
}

#[test]
fn cancellation_and_existing_destination_preserve_files() {
    let ct = CancellationToken::new();
    let doc = book("正文", &chapter_only());
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("book.epub");
    fs::write(&path, b"existing").unwrap();
    assert!(matches!(
        export_epub(&ct, &doc, &options(RenderLayout::SingleHtml), &path)
            .unwrap_err()
            .source,
        ExportFailure::TargetExists(_)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"existing");
    ct.cancel();
    let cancelled = output.path().join("cancelled.epub");
    assert!(matches!(
        export_epub(&ct, &doc, &options(RenderLayout::SingleHtml), &cancelled)
            .unwrap_err()
            .source,
        ExportFailure::Cancelled
    ));
    assert!(!cancelled.exists());
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 1);
}

#[test]
fn broken_rendered_link_fails_validation_without_publication() {
    let ct = CancellationToken::new();
    let doc = book("第一章 甲\n正文", &chapter_only());
    let rendered = render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).unwrap();
    let nav = rendered.directory().join("nav.xhtml");
    let text = fs::read_to_string(&nav)
        .unwrap()
        .replace("#section-0001", "#missing");
    fs::write(nav, text).unwrap();
    let output = tempfile::tempdir().unwrap();
    let error = package_epub(&ct, &rendered, output.path().join("broken.epub")).unwrap_err();
    assert_eq!(error.stage, ExportStage::Validating);
    assert!(error.to_string().contains("broken link"));
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
}

#[test]
fn packaging_errors_and_mid_write_cancellation_stop_output() {
    struct Fail;
    impl Write for Fail {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("injected failure"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let ct = CancellationToken::new();
    assert!(CancelWriter {
        ct: &ct,
        inner: Fail
    }
    .write_all(b"data")
    .is_err());
    struct Cancel<'a>(&'a CancellationToken, usize);
    impl Write for Cancel<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.1 += bytes.len();
            self.0.cancel();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut inner = Cancel(&ct, 0);
    assert!(CancelWriter {
        ct: &ct,
        inner: &mut inner
    }
    .write_all(&vec![b'a'; 32768])
    .is_err());
    assert_eq!(inner.1, 16384);
    let ct = CancellationToken::new();
    let doc = book("正文", &chapter_only());
    let rendered = render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).unwrap();
    fs::remove_file(rendered.directory().join(&rendered.files[0])).unwrap();
    let output = tempfile::tempdir().unwrap();
    assert_eq!(
        package_epub(&ct, &rendered, output.path().join("missing.epub"))
            .unwrap_err()
            .stage,
        ExportStage::Packaging
    );
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
}

#[test]
fn rendered_resources_are_removed_when_owner_is_dropped() {
    let doc = book("正文", &chapter_only());
    let rendered = render_book(
        &CancellationToken::new(),
        &doc,
        &options(RenderLayout::SingleHtml),
    )
    .unwrap();
    let path = rendered.directory().to_owned();
    assert!(path.exists());
    drop(rendered);
    assert!(!path.exists());
}

#[test]
fn publication_race_never_overwrites_an_existing_target() {
    let ct = CancellationToken::new();
    let doc = book("正文", &chapter_only());
    let rendered = render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).unwrap();
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("race.epub");
    std::thread::scope(|scope| {
        let results: Vec<_> = (0..2)
            .map(|_| scope.spawn(|| package_epub(&ct, &rendered, &path)))
            .collect();
        let results: Vec<_> = results
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert!(results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| matches!(e.source, ExportFailure::TargetExists(_))));
    });
    archive(&path);
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 1);
}

#[test]
fn anonymous_and_deep_containers_have_valid_navigation() {
    let mut doc = book("第一章 起点\n正文", &chapter_only());
    adjust(&mut doc, |toc| {
        let chapter = toc.root_ids()[0];
        let anonymous = toc.add_with_meta("", None, None).unwrap().id;
        let mut parent = anonymous;
        for index in 0..8 {
            parent = toc
                .add_with_meta(&format!("Group {index}"), None, Some(parent))
                .unwrap()
                .id;
        }
        toc.move_belong_to(chapter, parent);
    });
    let ct = CancellationToken::new();
    let rendered = render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).unwrap();
    assert_eq!(
        projection(&rendered)
            .iter()
            .filter(|(tag, _)| tag == "h6")
            .count(),
        4
    );
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("deep.epub");
    package_epub(&ct, &rendered, &path).unwrap();
    let files = archive(&path);
    let nav = roxmltree::Document::parse(&files["EPUB/nav.xhtml"]).unwrap();
    assert_eq!(
        nav.descendants()
            .filter(|n| n.tag_name().name() == "a")
            .count(),
        9
    );
}

#[test]
fn long_line_is_preserved_without_a_chapter_sized_context() {
    let line = "中文 & < > 🙂".repeat(30000);
    let doc = book(&line, &chapter_only());
    let ct = CancellationToken::new();
    let rendered = render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).unwrap();
    assert_eq!(projection(&rendered), [("p".into(), line)]);
}

#[test]
fn range_line_iterator_handles_fragmented_crlf_and_partial_lines() {
    let ct = CancellationToken::new();
    let mut doc = book("甲\r尾", &chapter_only());
    doc.apply(
        &ct,
        EditBatch {
            base: doc.view().version(),
            edits: vec![TextEdit {
                range: TextRange { start: 4, end: 4 },
                insert: "\n\n乙".into(),
            }],
        },
    )
    .unwrap();
    let view = doc.view();
    let lines: Vec<_> = view
        .range_lines(
            &ct,
            view.version(),
            TextRange {
                start: 0,
                end: view.len(),
            },
        )
        .unwrap()
        .map(|line| {
            let line = line.unwrap();
            (line.range, line.text().to_owned())
        })
        .collect();
    assert_eq!(
        lines,
        [
            (TextRange { start: 0, end: 5 }, "甲".into()),
            (TextRange { start: 5, end: 6 }, "".into()),
            (TextRange { start: 6, end: 12 }, "乙尾".into())
        ]
    );
    let partial = view
        .range_lines(&ct, view.version(), TextRange { start: 6, end: 9 })
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(partial.text(), "乙");
    assert!(view
        .range_lines(&ct, view.version(), TextRange { start: 7, end: 9 })
        .is_err());
}

#[test]
fn typed_ranges_survive_serialization_and_container_backfill() {
    let mut config = vbook_config();
    config.volumes = VolumeMode::Forced {
        chapters_per_volume: 1,
    };
    let doc = book(
        "第一章 甲\n正文",
        &VBookTocParser::from_config(&config).unwrap(),
    );
    let encoded = serde_json::to_string(&doc.results().unwrap().toc).unwrap();
    let toc: TocRoot = serde_json::from_str(&encoded).unwrap();
    let root = toc.get(toc.root_ids()[0]).unwrap();
    assert_eq!(root.meta.range_kind, TocRangeKind::Container);
    assert!(root.meta.range.is_some());
    assert_eq!(
        toc.get(root.children[0]).unwrap().meta.range_kind,
        TocRangeKind::Heading
    );
}

#[test]
fn render_rejects_a_plan_from_another_document() {
    let ct = CancellationToken::new();
    let first = book("正文", &chapter_only());
    let second = book("正文", &chapter_only());
    let plan = plan::build(&ct, &first, &options(RenderLayout::SingleHtml)).unwrap();
    assert!(matches!(
        render::render(&ct, second.view(), plan).unwrap_err(),
        ExportFailure::Document(DocumentError::WrongDocument)
    ));
}

#[test]
fn stages_honor_cancellation_before_work_and_after_publication() {
    let ct = CancellationToken::new();
    ct.cancel();
    for phase in [
        ExportStage::Planning,
        ExportStage::Rendering,
        ExportStage::Packaging,
        ExportStage::Validating,
        ExportStage::Publishing,
    ] {
        let error = stage(&ct, phase, || -> Result<()> {
            panic!("cancelled stage ran")
        })
        .unwrap_err();
        assert_eq!(error.stage, phase);
        assert!(matches!(error.source, ExportFailure::Cancelled));
    }
    let ct = CancellationToken::new();
    let value = stage(&ct, ExportStage::Publishing, || {
        ct.cancel();
        Ok("published")
    })
    .unwrap();
    assert_eq!(value, "published");
}

#[test]
fn invalid_rendered_xml_is_rejected_and_leaves_no_output() {
    let ct = CancellationToken::new();
    let doc = book("正文", &chapter_only());
    for bad in ["<html>", "<bogus xmlns=\"http://www.w3.org/1999/xhtml\" />"] {
        let rendered = render_book(&ct, &doc, &options(RenderLayout::SingleHtml)).unwrap();
        fs::write(rendered.directory().join(&rendered.files[0]), bad).unwrap();
        let output = tempfile::tempdir().unwrap();
        assert_eq!(
            package_epub(&ct, &rendered, output.path().join("invalid.epub"))
                .unwrap_err()
                .stage,
            ExportStage::Validating
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
}

#[test]
fn lone_carriage_returns_survive_xml_normalization() {
    let doc = book("正文\r继续\r", &chapter_only());
    let rendered = render_book(
        &CancellationToken::new(),
        &doc,
        &options(RenderLayout::SingleHtml),
    )
    .unwrap();
    assert_eq!(projection(&rendered), [("p".into(), "正文\r继续\r".into())]);
}

#[cfg(windows)]
#[test]
fn cleanup_failure_keeps_the_original_error_and_reports_the_path() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let file = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
    let path = file.path().to_owned();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&path)
        .unwrap();
    let mut error = ExportError {
        stage: ExportStage::Packaging,
        source: ExportFailure::Validation("original failure".into()),
        cleanup_failures: vec![],
    };
    cleanup_file(file, &mut error);
    assert_eq!(error.cleanup_failures.len(), 1);
    assert_eq!(error.cleanup_failures[0].path, path);
    assert!(error.to_string().contains("original failure"));
    drop(handle);
    fs::remove_file(path).unwrap();
}
