use tokio_util::sync::CancellationToken;

use super::AdFilterParser;
use crate::extractor::{Content, Encoding, ParsedContent};
use crate::parser::{FilterParser, MatchConfidence};
use crate::types::{TextOp, TextRange};

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

fn deleted_ranges(text: &str) -> Vec<TextRange> {
    let ops = AdFilterParser::new()
        .parse(&CancellationToken::new(), &content(text))
        .unwrap();
    ops.iter()
        .map(|op| match op {
            TextOp::Delete { range } => *range,
            other => panic!("expected Delete, got {other:?}"),
        })
        .collect()
}

#[test]
fn accept_always_low_confidence() {
    assert_eq!(AdFilterParser::new().accept(&content("")), MatchConfidence(10));
}

#[test]
fn deletes_keyword_lines_and_keeps_normal_lines() {
    let text = "正文第一行。\n请记住本书首发域名。\n正常的一行。\n无弹窗阅读。\n最后";
    let ranges = deleted_ranges(text);
    assert_eq!(ranges.len(), 2);
    // Deleted ranges cover the whole ad line including its trailing newline.
    assert_eq!(&text[ranges[0].start as usize..ranges[0].end as usize], "请记住本书首发域名。\n");
    assert_eq!(&text[ranges[1].start as usize..ranges[1].end as usize], "无弹窗阅读。\n");
    assert!(ranges[0].start < ranges[1].start);
}

#[test]
fn deletes_url_lines() {
    let text = "正常\n请访问 www.example.com 下载\n见 https://abc.xyz/x\n结尾";
    let ranges = deleted_ranges(text);
    assert_eq!(ranges.len(), 2);
    assert_eq!(&text[ranges[0].start as usize..ranges[0].end as usize], "请访问 www.example.com 下载\n");
    assert_eq!(&text[ranges[1].start as usize..ranges[1].end as usize], "见 https://abc.xyz/x\n");
}

#[test]
fn clean_text_produces_no_ops() {
    let text = "第一章 开始\n正文内容。\n";
    assert!(deleted_ranges(text).is_empty());
}

#[test]
fn cancellation_including_empty_input_returns_cancelled() {
    let ct = CancellationToken::new();
    ct.cancel();
    for text in ["", "正文\n请访问 www.example.com"] {
        assert!(matches!(
            AdFilterParser::new().parse(&ct, &content(text)),
            Err(crate::parser::ParserError::Cancelled)
        ));
    }
}
