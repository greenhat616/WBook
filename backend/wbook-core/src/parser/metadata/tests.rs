use camino::Utf8PathBuf;
use tokio_util::sync::CancellationToken;

use super::SimpleMetadataParser;
use crate::extractor::{Content, Encoding, ParsedContent};
use crate::parser::MetadataParser;

fn content(text: &str, source_path: Option<&str>) -> ParsedContent {
    ParsedContent {
        encoding: Encoding {
            name: "utf-8".to_string(),
            bom: false,
        },
        content: Content::Text(text.to_string()),
        source_path: source_path.map(Utf8PathBuf::from),
    }
}

fn parse(content: &ParsedContent) -> super::Metadata {
    SimpleMetadataParser::new()
        .parse(&CancellationToken::new(), content)
        .unwrap()
}

#[test]
fn extracts_title_from_book_brackets() {
    let metadata = parse(&content("《凡人修仙传》\n作者：忘语\n正文……", None));
    assert_eq!(metadata.title.as_deref(), Some("凡人修仙传"));
    assert_eq!(metadata.author.as_deref(), Some("忘语"));
}

#[test]
fn extracts_title_and_author_from_label_lines() {
    let metadata = parse(&content("书名：雪中悍刀行\n作者: 烽火戏诸侯\n正文", None));
    assert_eq!(metadata.title.as_deref(), Some("雪中悍刀行"));
    assert_eq!(metadata.author.as_deref(), Some("烽火戏诸侯"));
}

#[test]
fn book_brackets_win_over_label_line() {
    let metadata = parse(&content("书名：错误\n《正确》\n", None));
    assert_eq!(metadata.title.as_deref(), Some("正确"));
}

#[test]
fn falls_back_to_file_stem() {
    let metadata = parse(&content("正文内容，没有任何标题线索。\n", Some("/books/我的小说.txt")));
    assert_eq!(metadata.title.as_deref(), Some("我的小说"));
    assert_eq!(metadata.author, None);
}

#[test]
fn ignores_metadata_beyond_head_limit() {
    let text = format!("正文\n{}《太晚了》", "水".repeat(2000));
    let metadata = parse(&content(&text, None));
    assert_eq!(metadata.title, None);
}

#[test]
fn cancellation_including_empty_input_returns_cancelled() {
    let ct = CancellationToken::new();
    ct.cancel();
    for text in ["", "《标题》\n作者：甲"] {
        assert!(matches!(
            SimpleMetadataParser::new().parse(&ct, &content(text, None)),
            Err(crate::parser::ParserError::Cancelled)
        ));
    }
}
