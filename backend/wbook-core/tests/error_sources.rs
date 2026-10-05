use std::{error::Error, io};

use tokio_util::sync::CancellationToken;
use wbook_core::{
    extractor::ExtractorError,
    parser::{
        toc::{HeadingRuleConfig, LineRule, PatternRuleConfig, TocConfigError, TocParserConfig},
        ParserError,
    },
    session::OpError,
    workspace::{OpContext, ProcessingOptions, Workspace, WorkspaceError},
};

#[test]
fn missing_input_keeps_io_source_through_workspace_and_operation_errors() {
    let directory = tempfile::tempdir().unwrap();
    let source = camino::Utf8PathBuf::from_path_buf(directory.path().join("missing.txt")).unwrap();
    let mut workspace = Workspace::new(
        source,
        ProcessingOptions {
            filters: vec![],
            toc: TocParserConfig::SplitEvenly { parts: 1 },
        },
    )
    .unwrap();
    let error = OpError::from(
        workspace
            .initialize(&OpContext {
                ct: &CancellationToken::new(),
                report: &|_| {},
            })
            .unwrap_err(),
    );
    let workspace = error
        .source()
        .unwrap()
        .downcast_ref::<WorkspaceError>()
        .unwrap();
    let extractor = workspace
        .source()
        .unwrap()
        .downcast_ref::<ExtractorError>()
        .unwrap();
    let cause = extractor
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), io::ErrorKind::NotFound);
    assert!(!workspace.is_cancelled());
    assert_eq!(error.to_string(), cause.to_string());
}

#[test]
fn invalid_toc_pattern_keeps_its_pattern_context_and_regex_source() {
    let pattern = "[";
    let error = LineRule::new(
        1,
        &HeadingRuleConfig::Regex(PatternRuleConfig {
            pattern: pattern.into(),
            title_group: None,
        }),
    )
    .err()
    .unwrap();
    assert!(matches!(&error, TocConfigError::Regex { pattern: actual, .. } if actual == pattern));
    assert!(error.to_string().contains("invalid TOC pattern \"[\""));
    assert!(error.source().unwrap().is::<regex::Error>());
    let workspace = WorkspaceError::from(error);
    assert!(workspace.source().unwrap().is::<TocConfigError>());
    assert!(workspace
        .source()
        .unwrap()
        .source()
        .unwrap()
        .is::<regex::Error>());
}

#[test]
fn existing_dynamic_parser_sources_remain_downcastable() {
    let error = ParserError::from(anyhow::Error::new(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "input access denied",
    )));
    let source = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
    assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "input access denied");
}
