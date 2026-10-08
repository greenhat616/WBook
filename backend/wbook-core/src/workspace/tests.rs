use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::document::{TextEdit, TextView};
use crate::export::{ExportStage, OutputFormat, RenderLayout, RenderOptions};
use crate::parser::toc::{chapter_only, chapter_only_config, TocMode, TocSettings};
use crate::parser::MatchConfidence;
use crate::toc::{Toc, TocRoot, TocSnapshot};

fn silent(_: Phase) {}

fn context(ct: &CancellationToken) -> OpContext<'_> {
    OpContext {
        ct,
        report: &silent,
    }
}

fn config() -> TocParserConfig {
    TocParserConfig::Levels(chapter_only_config())
}

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.toc.mode = TocMode::Chapters;
    settings
}

fn fixture(text: &str, filters: usize) -> (tempfile::TempDir, Workspace) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("book.txt");
    fs::write(&path, text).unwrap();
    let workspace = Workspace::new(
        Utf8PathBuf::from_path_buf(path).unwrap(),
        Settings {
            filters: vec![FilterConfig::Ad; filters],
            ..settings()
        },
    )
    .unwrap();
    (directory, workspace)
}

fn options() -> ExportOptions {
    ExportOptions {
        render: RenderOptions {
            layout: RenderLayout::SingleHtml,
            ..Default::default()
        },
        format: OutputFormat::Epub,
        language: "en".into(),
        identifier: Some("urn:workspace-test".into()),
    }
}

fn text(ws: &Workspace) -> String {
    let view = ws.document().unwrap().view();
    ws.read_text(
        &context(&CancellationToken::new()),
        view.version(),
        TextRange {
            start: 0,
            end: view.len(),
        },
    )
    .unwrap()
}

fn saved_state(ws: &Workspace) -> serde_json::Value {
    // WorkspaceState deliberately has no Serialize implementation: a future
    // disk schema must be independent of this owned, private memory layout.
    serde_json::json!({
        "id": ws.id(),
        "revision": ws.revision(),
        "source": ws.source(),
        "settings": ws.state.settings,
        "filters_applied": ws.state.filters_applied,
        "document": ws.state.document.as_ref().map(|doc| serde_json::json!({
            "version": doc.view().version(),
            "text": text(ws),
            "results": doc.results(),
            "overrides": doc.metadata_overrides,
        })),
    })
}

#[test]
fn creation_validates_config_without_reading_and_extraction_can_retry() {
    let (directory, mut ws) = fixture("body", 0);
    let source = ws.source().to_owned();
    fs::remove_file(&source).unwrap();
    let other = Workspace::new(source.clone(), ws.state.settings.clone()).unwrap();
    assert_ne!(ws.id(), other.id());
    assert_eq!(Uuid::from_bytes(ws.id().0).get_version_num(), 4);
    assert_eq!(other.status().document, DocumentStatus::Absent);
    assert_eq!(other.status().document_len, None);
    assert!(matches!(
        Workspace::new(
            source.clone(),
            Settings {
                filters: vec![],
                toc: TocSettings {
                    mode: TocMode::Split,
                    parts: 0,
                    ..TocSettings::default()
                },
                ..settings()
            }
        ),
        Err(WorkspaceError::InvalidConfig { source: _ })
    ));
    assert!(matches!(
        ws.initialize(&context(&CancellationToken::new())),
        Err(WorkspaceError::Extractor {
            source: ExtractorError::Io { source: _ }
        })
    ));
    assert_eq!(ws.revision(), Revision(0));
    fs::write(&source, "body").unwrap();
    assert_eq!(
        ws.initialize(&context(&CancellationToken::new())).unwrap(),
        Revision(2)
    );
    assert_eq!(ws.status().document_len, Some(4));
    assert!(ws.close().is_empty());
    assert!(source.exists());
    drop(directory);
}

#[test]
fn commit_table_and_stale_results() {
    let (_directory, mut ws) = fixture("第一章 Start\nBody", 2);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    let phases = [
        Phase::Extracting,
        Phase::Filtering { index: 0, total: 2 },
        Phase::Filtering { index: 1, total: 2 },
        Phase::Parsing,
        Phase::Installing,
    ];
    let count = AtomicUsize::new(0);
    let report = |phase| {
        assert_eq!(phase, phases[count.fetch_add(1, Ordering::Relaxed)]);
    };
    assert_eq!(
        ws.initialize(&OpContext {
            ct: &ct,
            report: &report
        })
        .unwrap(),
        Revision(4)
    );
    assert_eq!(count.load(Ordering::Relaxed), phases.len());
    assert_eq!(
        ws.status().filters,
        FilterProgress {
            applied: 2,
            total: 2
        }
    );
    assert_eq!(ws.status().document, DocumentStatus::Current);
    let version = ws.status().document_version.unwrap();
    let preview = ws.render_preview(&cx, Revision(4), options()).unwrap();
    for edits in [
        vec![],
        vec![TextEdit {
            range: TextRange { start: 0, end: 0 },
            insert: String::new(),
        }],
    ] {
        let (revision, changes) = ws
            .apply_edits(
                &cx,
                Revision(4),
                EditBatch {
                    base: version,
                    edits,
                },
            )
            .unwrap();
        assert_eq!(revision, Revision(4));
        assert_eq!(changes.before(), changes.after());
        assert_eq!(
            ws.render_preview(&cx, revision, options()).unwrap(),
            preview
        );
    }
    assert_eq!(
        ws.set_metadata_overrides(&cx, Revision(4), Metadata::default())
            .unwrap(),
        Revision(4)
    );
    assert!(matches!(
        ws.set_metadata_overrides(
            &cx,
            Revision(4),
            Metadata {
                isbn: Some("978-7-02-000220-8".into()),
                ..Metadata::default()
            }
        ),
        Err(WorkspaceError::InvalidMetadata { .. })
    ));
    assert_eq!(ws.revision(), Revision(4));
    let before = saved_state(&ws);
    let candidate = ws
        .parse(&cx, TocParserConfig::SplitEvenly { parts: 2 })
        .unwrap();
    assert!(ws.results(&cx).unwrap().current);
    assert_eq!(text(&ws), "第一章 Start\nBody");
    assert_eq!(saved_state(&ws), before);
    assert_eq!(
        ws.install(&cx, Revision(4), candidate).unwrap(),
        Revision(5)
    );
    assert!(!preview.directory.exists());
    assert!(ws.status().preview.is_none());
    assert!(ws.status().preview_id.is_none());
    let preview = ws.render_preview(&cx, Revision(5), options()).unwrap();
    let overrides = Metadata {
        title: Some("Manual book".into()),
        author: None,
        ..Metadata::default()
    };
    assert_eq!(
        ws.set_metadata_overrides(&cx, Revision(5), overrides.clone())
            .unwrap(),
        Revision(6)
    );
    assert!(!preview.directory.exists());
    assert!(ws.status().has_overrides);
    let preview = ws.render_preview(&cx, Revision(6), options()).unwrap();
    assert_eq!(
        ws.set_metadata_overrides(&cx, Revision(6), overrides)
            .unwrap(),
        Revision(6)
    );
    assert!(preview.directory.exists());
    let (revision, changes) = ws
        .apply_edits(
            &cx,
            Revision(6),
            EditBatch {
                base: version,
                edits: vec![TextEdit {
                    range: TextRange { start: 0, end: 0 },
                    insert: "Preface\n".into(),
                }],
            },
        )
        .unwrap();
    assert_eq!(revision, Revision(7));
    assert_ne!(changes.before(), changes.after());
    assert!(!preview.directory.exists());
    assert_eq!(ws.status().document, DocumentStatus::Stale);
    assert!(!ws.results(&cx).unwrap().current);
    assert!(ws.results(&cx).unwrap().results.is_some());
    assert!(matches!(
        ws.render_preview(&cx, revision, options()),
        Err(WorkspaceError::ResultsNotCurrent)
    ));
    assert!(matches!(
        ws.export_epub(&cx, revision, options(), "unused.epub"),
        Err(WorkspaceError::ResultsNotCurrent)
    ));
    assert!(matches!(
        ws.initialize(&cx),
        Err(WorkspaceError::AlreadyInitialized)
    ));
    let results = ws.parse(&cx, config()).unwrap();
    assert_eq!(ws.install(&cx, revision, results).unwrap(), Revision(8));
    assert_eq!(
        ws.set_metadata_overrides(&cx, Revision(8), Metadata::default())
            .unwrap(),
        Revision(9)
    );
    assert!(!ws.status().has_overrides);
    assert_eq!(
        ws.document().unwrap().metadata().unwrap().title.as_deref(),
        Some("book")
    );
    assert!(ws.take_warnings().is_empty());
    assert_eq!(ws.revision(), Revision(9));
    ws.close();
}

#[test]
fn stale_revision_rejects_every_guarded_operation_without_changes() {
    let (directory, mut ws) = fixture("第一章 Start\nBody", 0);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    ws.initialize(&cx).unwrap();
    let revision = ws.revision();
    let preview = ws.render_preview(&cx, revision, options()).unwrap();
    let before = saved_state(&ws);
    let candidate = ws.parse(&cx, config()).unwrap();
    let stale = Revision(0);
    let batch = EditBatch {
        base: candidate.version,
        edits: vec![TextEdit {
            range: TextRange { start: 0, end: 0 },
            insert: "wrong".into(),
        }],
    };
    assert!(matches!(
        ws.apply_edits(&cx, stale, batch),
        Err(WorkspaceError::StaleRevision { .. })
    ));
    assert!(matches!(
        ws.install(&cx, stale, candidate),
        Err(WorkspaceError::StaleRevision { .. })
    ));
    assert!(matches!(
        ws.set_metadata_overrides(
            &cx,
            stale,
            Metadata {
                title: Some("wrong".into()),
                author: None,
                ..Metadata::default()
            }
        ),
        Err(WorkspaceError::StaleRevision { .. })
    ));
    assert!(matches!(
        ws.render_preview(&cx, stale, options()),
        Err(WorkspaceError::StaleRevision { .. })
    ));
    let destination = directory.path().join("stale.epub");
    assert!(matches!(
        ws.export_epub(&cx, stale, options(), &destination),
        Err(WorkspaceError::StaleRevision { .. })
    ));
    assert!(!destination.exists());
    assert_eq!(saved_state(&ws), before);
    assert_eq!(
        ws.render_preview(&cx, revision, options()).unwrap(),
        preview
    );
    ws.close();
}

enum Behavior {
    Append,
    Empty,
    Fail,
    Cancel,
    Invalid,
}

struct Filter {
    calls: AtomicUsize,
    behavior: Behavior,
}

impl Filter {
    fn new(behavior: Behavior) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            behavior,
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl FilterParser for Filter {
    fn name(&self) -> &'static str {
        "test-filter"
    }
    fn accept(&self, _: TextView<'_>) -> MatchConfidence {
        MatchConfidence(1)
    }
    fn parse(
        &self,
        ct: &CancellationToken,
        view: TextView<'_>,
    ) -> Result<Vec<TextEdit>, ParserError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match self.behavior {
            Behavior::Append => Ok(vec![TextEdit {
                range: TextRange {
                    start: view.len(),
                    end: view.len(),
                },
                insert: "!".into(),
            }]),
            Behavior::Empty => Ok(vec![]),
            Behavior::Fail => Err(ParserError::NoMatch {
                message: "injected failure".into(),
            }),
            Behavior::Invalid => Ok(vec![TextEdit {
                range: TextRange {
                    start: 0,
                    end: view.len() + 1,
                },
                insert: String::new(),
            }]),
            Behavior::Cancel => {
                ct.cancel();
                Ok(vec![TextEdit {
                    range: TextRange { start: 0, end: 0 },
                    insert: "uncommitted".into(),
                }])
            }
        }
    }
}

struct Parsing {
    toc_calls: AtomicUsize,
    metadata_calls: AtomicUsize,
    fail_toc: bool,
    fail_metadata: bool,
    cancel: bool,
}

impl Parsing {
    fn new(fail_toc: bool, fail_metadata: bool, cancel: bool) -> Self {
        Self {
            toc_calls: AtomicUsize::new(0),
            metadata_calls: AtomicUsize::new(0),
            fail_toc,
            fail_metadata,
            cancel,
        }
    }
}

impl TocParser for Parsing {
    fn name(&self) -> &'static str {
        "test-toc"
    }
    fn accept(&self, _: TextView<'_>) -> MatchConfidence {
        MatchConfidence(1)
    }
    fn parse(&self, ct: &CancellationToken, view: TextView<'_>) -> Result<TocRoot, ParserError> {
        self.toc_calls.fetch_add(1, Ordering::Relaxed);
        if self.fail_toc {
            if self.cancel {
                ct.cancel();
                return Err(ParserError::Cancelled);
            }
            return Err(ParserError::NoMatch {
                message: "injected TOC failure".into(),
            });
        }
        chapter_only().parse(ct, view)
    }
}

impl MetadataParser for Parsing {
    fn name(&self) -> &'static str {
        "test-metadata"
    }
    fn accept(&self, _: TextView<'_>) -> MatchConfidence {
        MatchConfidence(1)
    }
    fn parse(&self, ct: &CancellationToken, view: TextView<'_>) -> Result<Metadata, ParserError> {
        self.metadata_calls.fetch_add(1, Ordering::Relaxed);
        if self.fail_metadata {
            if self.cancel {
                ct.cancel();
                return Err(ParserError::Cancelled);
            }
            return Err(ParserError::NoMatch {
                message: "injected metadata failure".into(),
            });
        }
        SimpleMetadataParser.parse(ct, view)
    }
}

#[test]
fn initialize_resumes_without_repeating_non_idempotent_filters() {
    for behavior in [Behavior::Fail, Behavior::Cancel, Behavior::Invalid] {
        let (_directory, mut ws) = fixture("Body", 3);
        let first = Filter::new(Behavior::Append);
        let failing = Filter::new(behavior);
        let last = Filter::new(Behavior::Append);
        let parsing = Parsing::new(false, false, false);
        let ct = CancellationToken::new();
        let error = ws
            .initialize_with(
                &context(&ct),
                &[&first, &failing, &last],
                &parsing,
                &parsing,
            )
            .unwrap_err();
        assert_eq!(
            error.is_cancelled(),
            matches!(failing.behavior, Behavior::Cancel)
        );
        assert_eq!(ws.revision(), Revision(2));
        assert_eq!(ws.status().document, DocumentStatus::Unparsed);
        assert_eq!(ws.status().filters.applied, 1);
        assert_eq!(text(&ws), "Body!");
        assert_eq!((first.calls(), failing.calls(), last.calls()), (1, 1, 0));
        assert_eq!(parsing.toc_calls.load(Ordering::Relaxed), 0);
        assert_eq!(parsing.metadata_calls.load(Ordering::Relaxed), 0);
        fs::remove_file(ws.source()).unwrap();
        let second = Filter::new(Behavior::Empty);
        let phases = [
            Phase::Filtering { index: 1, total: 3 },
            Phase::Filtering { index: 2, total: 3 },
            Phase::Parsing,
            Phase::Installing,
        ];
        let count = AtomicUsize::new(0);
        let report = |phase| {
            assert_eq!(phase, phases[count.fetch_add(1, Ordering::Relaxed)]);
        };
        let fresh = CancellationToken::new();
        assert_eq!(
            ws.initialize_with(
                &OpContext {
                    ct: &fresh,
                    report: &report
                },
                &[&first, &second, &last],
                &parsing,
                &parsing
            )
            .unwrap(),
            Revision(5)
        );
        assert_eq!(count.load(Ordering::Relaxed), phases.len());
        assert_eq!((first.calls(), second.calls(), last.calls()), (1, 1, 1));
        assert_eq!(text(&ws), "Body!!");
        assert_eq!(ws.status().filters.applied, 3);
        assert_eq!(parsing.toc_calls.load(Ordering::Relaxed), 1);
        assert_eq!(parsing.metadata_calls.load(Ordering::Relaxed), 1);
        assert!(matches!(
            ws.initialize_with(
                &context(&fresh),
                &[&first, &second, &last],
                &parsing,
                &parsing
            ),
            Err(WorkspaceError::AlreadyInitialized)
        ));
        ws.close();
    }
}

#[test]
fn cancellation_before_takeover_keeps_absent_and_later_boundaries_keep_prefix() {
    let (_directory, mut ws) = fixture("Body", 1);
    let ct = CancellationToken::new();
    let content = SimpleExtractor
        .process(&ct, &ProcessOptions {}, ws.source())
        .unwrap();
    ct.cancel();
    assert!(ws
        .take_over(&context(&ct), content)
        .unwrap_err()
        .is_cancelled());
    assert_eq!(ws.status().document, DocumentStatus::Absent);
    assert_eq!(ws.revision(), Revision(0));
    assert!(ws.initialize(&context(&ct)).unwrap_err().is_cancelled());
    assert_eq!(ws.revision(), Revision(0));
    for (phase, expected_revision, applied) in [
        (Phase::Filtering { index: 0, total: 1 }, Revision(1), 0),
        (Phase::Parsing, Revision(2), 1),
        (Phase::Installing, Revision(2), 1),
    ] {
        let (_directory, mut ws) = fixture("Body", 1);
        let ct = CancellationToken::new();
        let report = |reported| {
            if reported == phase {
                ct.cancel();
            }
        };
        assert!(ws
            .initialize(&OpContext {
                ct: &ct,
                report: &report
            })
            .unwrap_err()
            .is_cancelled());
        assert_eq!(ws.revision(), expected_revision);
        assert_eq!(ws.status().filters.applied, applied);
        assert_eq!(ws.status().document, DocumentStatus::Unparsed);
        assert_eq!(text(&ws), "Body");
        ws.close();
    }
}

#[test]
fn parsing_failure_or_cancellation_keeps_filters_and_stops_installation() {
    for fail_toc in [true, false] {
        for cancel in [false, true] {
            let (_directory, mut ws) = fixture("Body", 1);
            let filter = Filter::new(Behavior::Append);
            let parsing = Parsing::new(fail_toc, !fail_toc, cancel);
            let ct = CancellationToken::new();
            let error = ws
                .initialize_with(&context(&ct), &[&filter], &parsing, &parsing)
                .unwrap_err();
            assert_eq!(error.is_cancelled(), cancel);
            assert_eq!(ws.revision(), Revision(2));
            assert_eq!(ws.status().filters.applied, 1);
            assert_eq!(text(&ws), "Body!");
            assert_eq!(parsing.toc_calls.load(Ordering::Relaxed), 1);
            assert_eq!(
                parsing.metadata_calls.load(Ordering::Relaxed),
                usize::from(!fail_toc)
            );
            let fresh = CancellationToken::new();
            let parsing = Parsing::new(false, false, false);
            assert_eq!(
                ws.initialize_with(&context(&fresh), &[&filter], &parsing, &parsing)
                    .unwrap(),
                Revision(3)
            );
            assert_eq!(filter.calls(), 1);
            ws.close();
        }
    }
}

#[test]
fn explicit_install_after_partial_filters_disables_initialize() {
    let (_directory, mut ws) = fixture("Body", 2);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    let first = Filter::new(Behavior::Append);
    let failing = Filter::new(Behavior::Fail);
    assert!(ws
        .initialize_with(
            &cx,
            &[&first, &failing],
            &chapter_only(),
            &SimpleMetadataParser
        )
        .is_err());
    let result = ws.results(&cx).unwrap();
    assert!(result.results.is_none());
    assert!(!result.current);
    let results = ws.parse(&cx, config()).unwrap();
    assert_eq!(ws.install(&cx, Revision(2), results).unwrap(), Revision(3));
    assert_eq!(ws.status().filters.applied, 1);
    assert!(matches!(
        ws.initialize(&cx),
        Err(WorkspaceError::AlreadyInitialized)
    ));
    assert_eq!(first.calls(), 1);
    ws.close();
}

#[test]
fn install_validates_document_version_and_ranges_and_preserves_manual_values() {
    let (_directory, mut ws) = fixture("第一章 Start\nBody", 0);
    let (_other_directory, mut other) = fixture("第一章 Other\nBody", 0);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    ws.initialize(&cx).unwrap();
    other.initialize(&cx).unwrap();
    let revision = ws.revision();
    let before = saved_state(&ws);
    let preview = ws.render_preview(&cx, revision, options()).unwrap();
    let foreign = other.parse(&cx, config()).unwrap();
    assert!(matches!(
        ws.install(&cx, revision, foreign),
        Err(WorkspaceError::Pipeline {
            source: PipelineError::Document {
                source: DocumentError::WrongDocument
            }
        })
    ));
    let mut bad = ws.parse(&cx, config()).unwrap();
    let id = bad.toc.root_ids()[0];
    bad.toc.get_mut(id).unwrap().meta.range = Some(TextRange {
        start: 0,
        end: 10000,
    });
    assert!(matches!(
        ws.install(&cx, revision, bad),
        Err(WorkspaceError::Pipeline {
            source: PipelineError::Document {
                source: DocumentError::OutOfBounds { .. }
            }
        })
    ));
    assert_eq!(saved_state(&ws), before);
    assert_eq!(
        ws.render_preview(&cx, revision, options()).unwrap(),
        preview
    );
    let stale = ws.parse(&cx, config()).unwrap();
    ws.apply_edits(
        &cx,
        revision,
        EditBatch {
            base: stale.version,
            edits: vec![TextEdit {
                range: TextRange { start: 0, end: 0 },
                insert: "Preface\n".into(),
            }],
        },
    )
    .unwrap();
    let before = saved_state(&ws);
    assert!(matches!(
        ws.install(&cx, ws.revision(), stale),
        Err(WorkspaceError::Pipeline {
            source: PipelineError::Document {
                source: DocumentError::StaleVersion { .. }
            }
        })
    ));
    assert_eq!(saved_state(&ws), before);
    ws.set_metadata_overrides(
        &cx,
        ws.revision(),
        Metadata {
            title: Some("Override".into()),
            author: None,
            ..Metadata::default()
        },
    )
    .unwrap();
    let mut manual = ws.parse(&cx, config()).unwrap();
    let id = manual.toc.root_ids()[0];
    manual.toc.get_mut(id).unwrap().title = "Manual chapter".into();
    manual.metadata.title = Some("Manual automatic title".into());
    // Use the same validated wire representation callers use for edited TOCs.
    let manual = serde_json::from_str(&serde_json::to_string(&manual).unwrap()).unwrap();
    ws.install(&cx, ws.revision(), manual).unwrap();
    let before = saved_state(&ws);
    ws.parse(&cx, TocParserConfig::SplitEvenly { parts: 2 })
        .unwrap();
    assert_eq!(saved_state(&ws), before);
    let results = ws.results(&cx).unwrap();
    assert_eq!(
        TocSnapshot::from(&results.results.unwrap().toc)[0].title,
        "Manual chapter"
    );
    assert_eq!(results.overrides.title.as_deref(), Some("Override"));
    ws.set_metadata_overrides(&cx, ws.revision(), Metadata::default())
        .unwrap();
    assert_eq!(
        ws.document().unwrap().metadata().unwrap().title.as_deref(),
        Some("Manual automatic title")
    );
    ws.close();
    other.close();
}

#[test]
fn rejected_edits_are_atomic_and_reads_validate_versions_and_utf8_byte_limit() {
    let (_directory, mut ws) = fixture("Body🙂", 0);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    assert!(matches!(ws.results(&cx), Err(WorkspaceError::NoDocument)));
    assert!(matches!(
        ws.parse(&cx, config()),
        Err(WorkspaceError::NoDocument)
    ));
    ws.initialize(&cx).unwrap();
    let revision = ws.revision();
    let version = ws.status().document_version.unwrap();
    let before = saved_state(&ws);
    for edits in [
        vec![TextEdit {
            range: TextRange { start: 0, end: 100 },
            insert: String::new(),
        }],
        vec![TextEdit {
            range: TextRange { start: 5, end: 6 },
            insert: String::new(),
        }],
        vec![
            TextEdit {
                range: TextRange { start: 0, end: 3 },
                insert: String::new(),
            },
            TextEdit {
                range: TextRange { start: 2, end: 4 },
                insert: String::new(),
            },
        ],
    ] {
        assert!(ws
            .apply_edits(
                &cx,
                revision,
                EditBatch {
                    base: version,
                    edits
                }
            )
            .is_err());
        assert_eq!(saved_state(&ws), before);
    }
    assert!(matches!(
        ws.read_text(&cx, version, TextRange { start: 5, end: 6 }),
        Err(WorkspaceError::Document {
            source: DocumentError::InvalidBoundary { .. }
        })
    ));
    ws.apply_edits(
        &cx,
        revision,
        EditBatch {
            base: version,
            edits: vec![TextEdit {
                range: TextRange { start: 0, end: 0 },
                insert: "a".into(),
            }],
        },
    )
    .unwrap();
    assert!(matches!(
        ws.read_text(&cx, version, TextRange { start: 0, end: 1 }),
        Err(WorkspaceError::Document {
            source: DocumentError::StaleVersion { .. }
        })
    ));
    assert!(matches!(
        ws.apply_edits(
            &cx,
            ws.revision(),
            EditBatch {
                base: version,
                edits: vec![]
            }
        ),
        Err(WorkspaceError::Document {
            source: DocumentError::StaleVersion { .. }
        })
    ));
    let (_other_directory, mut other) = fixture("Other", 0);
    other.initialize(&cx).unwrap();
    assert!(matches!(
        ws.read_text(
            &cx,
            other.status().document_version.unwrap(),
            TextRange { start: 0, end: 0 }
        ),
        Err(WorkspaceError::Document {
            source: DocumentError::WrongDocument
        })
    ));
    let (_large_directory, mut large) = fixture(&"é".repeat((READ_LIMIT / 2 + 1) as usize), 0);
    large.initialize(&cx).unwrap();
    let version = large.status().document_version.unwrap();
    assert_eq!(
        large
            .read_text(
                &cx,
                version,
                TextRange {
                    start: 0,
                    end: READ_LIMIT
                }
            )
            .unwrap()
            .len(),
        READ_LIMIT as usize
    );
    assert!(
        matches!(large.read_text(&cx, version, TextRange { start: 0, end: READ_LIMIT + 2 }), Err(WorkspaceError::ReadTooLarge { requested, limit }) if requested == READ_LIMIT + 2 && limit == READ_LIMIT)
    );
    ct.cancel();
    assert!(large
        .read_text(&cx, version, TextRange { start: 0, end: 1 })
        .unwrap_err()
        .is_cancelled());
    ws.close();
    other.close();
    large.close();
}

#[test]
fn preview_reuses_equal_options_replaces_changed_options_and_keeps_old_on_failure() {
    let (_directory, mut ws) = fixture("第一章 Start\nBody", 0);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    ws.initialize(&cx).unwrap();
    let revision = ws.revision();
    let before = saved_state(&ws);
    let mut opts = options();
    let mut preview = ws.render_preview(&cx, revision, opts.clone()).unwrap();
    let first_id = preview.id.clone();
    assert_eq!(Uuid::parse_str(&first_id).unwrap().get_version_num(), 4);
    assert_eq!(ws.current_preview(), Some(preview.clone()));
    assert_eq!(ws.status().preview_id.as_deref(), Some(preview.id.as_str()));
    let marker = preview.directory.join("reuse-marker");
    fs::write(&marker, b"retained").unwrap();
    assert_eq!(
        ws.render_preview(&cx, revision, opts.clone()).unwrap(),
        preview
    );
    assert!(marker.exists());
    for change in ["layout", "language", "identifier"] {
        match change {
            "layout" => opts.render.layout = RenderLayout::SplitChapters,
            "language" => opts.language = "zh-Hant".into(),
            _ => opts.identifier = Some("urn:changed".into()),
        }
        let next = ws.render_preview(&cx, revision, opts.clone()).unwrap();
        assert_ne!(next.id, preview.id);
        assert_eq!(ws.status().preview_id.as_deref(), Some(next.id.as_str()));
        assert_eq!(next.revision, preview.revision);
        assert_ne!(next.directory, preview.directory);
        assert!(!preview.directory.exists());
        preview = next;
    }
    let mut invalid = opts.clone();
    invalid.language = "bad_language".into();
    assert!(ws.render_preview(&cx, revision, invalid).is_err());
    assert_eq!(ws.render_preview(&cx, revision, opts).unwrap(), preview);
    assert_eq!(saved_state(&ws), before);
    ct.cancel();
    assert!(ws
        .render_preview(&cx, revision, preview.options.clone())
        .unwrap_err()
        .is_cancelled());
    assert!(preview.directory.exists());
    assert_eq!(ws.current_preview(), Some(preview.clone()));
    let restored = ws
        .render_preview(&context(&CancellationToken::new()), revision, options())
        .unwrap();
    assert_ne!(restored.id, first_id);
    assert!(ws.close().is_empty());
    assert!(!restored.directory.exists());
    assert!(!preview.directory.exists());
}

#[test]
fn export_is_independent_preserves_target_and_allows_further_edits() {
    let (directory, mut ws) = fixture("第一章 Start\nBody", 0);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    ws.initialize(&cx).unwrap();
    let revision = ws.revision();
    let preview = ws.render_preview(&cx, revision, options()).unwrap();
    fs::write(preview.directory.join(&preview.files[0]), "invalid XML").unwrap();
    let before = saved_state(&ws);
    let destination = directory.path().join("book.epub");
    let artifact = ws
        .export_epub(&cx, revision, options(), &destination)
        .unwrap();
    assert_eq!(artifact.revision, revision);
    assert_eq!(
        artifact.artifact.version,
        ws.status().document_version.unwrap()
    );
    assert_eq!(artifact.artifact.identifier, "urn:workspace-test");
    assert_eq!(artifact.artifact.path, destination);
    let published = fs::read(&destination).unwrap();
    assert!(published.starts_with(b"PK"));
    assert_eq!(
        ws.render_preview(&cx, revision, options()).unwrap(),
        preview
    );
    assert_eq!(saved_state(&ws), before);
    let error = ws
        .export_epub(&cx, revision, options(), &destination)
        .unwrap_err();
    assert!(matches!(
        error,
        WorkspaceError::Export {
            source: ExportError {
                source: ExportFailure::TargetExists { path: _ },
                ..
            }
        }
    ));
    assert_eq!(fs::read(&destination).unwrap(), published);
    let cancelled = directory.path().join("cancelled.epub");
    ct.cancel();
    assert!(ws
        .export_epub(&cx, revision, options(), &cancelled)
        .unwrap_err()
        .is_cancelled());
    assert!(!cancelled.exists());
    let fresh = CancellationToken::new();
    let cx = context(&fresh);
    let version = ws.status().document_version.unwrap();
    ws.apply_edits(
        &cx,
        revision,
        EditBatch {
            base: version,
            edits: vec![TextEdit {
                range: TextRange { start: 0, end: 0 },
                insert: "Preface\n".into(),
            }],
        },
    )
    .unwrap();
    let results = ws.parse(&cx, config()).unwrap();
    ws.install(&cx, ws.revision(), results).unwrap();
    ws.export_epub(
        &cx,
        ws.revision(),
        options(),
        directory.path().join("updated.epub"),
    )
    .unwrap();
    let source = ws.source().to_owned();
    ws.close();
    assert_eq!(fs::read(&destination).unwrap(), published);
    assert!(source.exists());
}

#[test]
fn cancellation_classification_uses_the_error_category() {
    for error in [
        WorkspaceError::Extractor {
            source: ExtractorError::Shutdown,
        },
        WorkspaceError::Document {
            source: DocumentError::Cancelled,
        },
        WorkspaceError::Pipeline {
            source: PipelineError::Document {
                source: DocumentError::Cancelled,
            },
        },
        WorkspaceError::Pipeline {
            source: PipelineError::Parser {
                source: ParserError::Cancelled,
            },
        },
        WorkspaceError::Export {
            source: ExportError {
                stage: ExportStage::Publishing,
                source: ExportFailure::Cancelled,
                cleanup_failures: vec![],
            },
        },
        WorkspaceError::Export {
            source: ExportError {
                stage: ExportStage::Planning,
                source: ExportFailure::Pipeline {
                    source: PipelineError::Parser {
                        source: ParserError::Cancelled,
                    },
                },
                cleanup_failures: vec![],
            },
        },
    ] {
        assert!(error.is_cancelled());
    }
    let ct = CancellationToken::new();
    ct.cancel();
    for error in [
        WorkspaceError::NoDocument,
        WorkspaceError::Extractor {
            source: ExtractorError::Io {
                source: std::io::Error::other("read failed"),
            },
        },
        WorkspaceError::Pipeline {
            source: PipelineError::Parser {
                source: ParserError::NoMatch {
                    message: "bad content".into(),
                },
            },
        },
        WorkspaceError::Export {
            source: ExportError {
                stage: ExportStage::Publishing,
                source: ExportFailure::TargetExists {
                    path: "existing.epub".into(),
                },
                cleanup_failures: vec![],
            },
        },
    ] {
        assert!(!error.is_cancelled());
    }
}

#[cfg(windows)]
#[test]
fn preview_cleanup_failures_warn_without_overriding_commits_or_replacements() {
    use std::os::windows::fs::OpenOptionsExt;
    for replacement in [false, true] {
        let (_directory, mut ws) = fixture("Body", 0);
        let ct = CancellationToken::new();
        let cx = context(&ct);
        ws.initialize(&cx).unwrap();
        let revision = ws.revision();
        let preview = ws.render_preview(&cx, revision, options()).unwrap();
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(1 | 2)
            .open(preview.directory.join(&preview.files[0]))
            .unwrap();
        if replacement {
            let mut changed = options();
            changed.language = "zh".into();
            let next = ws.render_preview(&cx, revision, changed).unwrap();
            assert_ne!(next.directory, preview.directory);
            assert_eq!(ws.revision(), revision);
        } else {
            assert_eq!(
                ws.set_metadata_overrides(
                    &cx,
                    revision,
                    Metadata {
                        title: Some("Changed".into()),
                        author: None,
                        ..Metadata::default()
                    }
                )
                .unwrap(),
                Revision(revision.0 + 1)
            );
            assert!(ws.status().preview.is_none());
        }
        let destination = ws.source().with_extension("epub");
        let artifact = ws
            .export_epub(&cx, ws.revision(), options(), &destination)
            .unwrap();
        assert_eq!(artifact.artifact.path, destination.as_std_path());
        assert!(destination.exists());
        let warnings = ws.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].path, preview.directory);
        assert!(!warnings[0].message.is_empty());
        assert!(ws.take_warnings().is_empty());
        drop(handle);
        fs::remove_dir_all(&preview.directory).unwrap();
        assert!(ws.close().is_empty());
    }
}

#[cfg(windows)]
#[test]
fn close_returns_pending_and_new_cleanup_warnings() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_directory, mut ws) = fixture("Body", 0);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    ws.initialize(&cx).unwrap();
    let revision = ws.revision();
    let first = ws.render_preview(&cx, revision, options()).unwrap();
    let first_handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(first.directory.join(&first.files[0]))
        .unwrap();
    let mut changed = options();
    changed.language = "zh".into();
    let second = ws.render_preview(&cx, revision, changed).unwrap();
    let second_handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(second.directory.join(&second.files[0]))
        .unwrap();
    let warnings = ws.close();
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].path, first.directory);
    assert_eq!(warnings[1].path, second.directory);
    drop(first_handle);
    drop(second_handle);
    fs::remove_dir_all(first.directory).unwrap();
    fs::remove_dir_all(second.directory).unwrap();
}

#[test]
fn settings_changes_commit_close_the_preview_and_reject_invalid_values() {
    let (_directory, mut ws) = fixture("第一章 Start\nBody", 0);
    let ct = CancellationToken::new();
    let cx = context(&ct);
    let revision = ws.initialize(&cx).unwrap();
    let preview = ws
        .render_preview(&cx, revision, ws.settings().export_options())
        .unwrap();

    let unchanged = ws.settings().clone();
    assert_eq!(ws.set_settings(&cx, revision, unchanged).unwrap(), revision);
    assert!(preview.directory.exists());

    let mut invalid = ws.settings().clone();
    invalid.toc.chapter_marks.clear();
    assert!(matches!(
        ws.set_settings(&cx, revision, invalid),
        Err(WorkspaceError::InvalidConfig { source: _ })
    ));
    assert!(matches!(
        ws.set_settings(&cx, Revision(0), settings()),
        Err(WorkspaceError::StaleRevision { .. })
    ));
    assert_eq!(ws.revision(), revision);
    assert!(preview.directory.exists());

    let mut changed = ws.settings().clone();
    changed.render.layout = RenderLayout::SplitChapters;
    let next = ws.set_settings(&cx, revision, changed.clone()).unwrap();
    assert_eq!(next, Revision(revision.0 + 1));
    assert_eq!(ws.settings(), &changed);
    assert!(!preview.directory.exists());
    assert!(ws.status().preview.is_none());
    // Settings do not touch the installed results.
    assert_eq!(ws.status().document, DocumentStatus::Current);
}
