use crate::document::TextDocument;
use camino::Utf8PathBuf;
use tokio_util::sync::CancellationToken;

use super::SimpleMetadataParser;
use crate::extractor::{Content, Encoding, ParsedContent};
use crate::parser::MetadataParser;

fn content(text: &str, source_path: Option<&str>) -> TextDocument {
    ParsedContent {
        encoding: Encoding {
            name: "utf-8".to_string(),
            bom: false,
        },
        content: Content::Text(text.to_string()),
        source_path: source_path.map(Utf8PathBuf::from),
    }
    .into()
}

fn parse(content: &TextDocument) -> super::Metadata {
    SimpleMetadataParser::new()
        .parse(&CancellationToken::new(), content.view())
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
    let metadata = parse(&content(
        "正文内容，没有任何标题线索。\n",
        Some("/books/我的小说.txt"),
    ));
    assert_eq!(metadata.title.as_deref(), Some("我的小说"));
    assert_eq!(metadata.author, None);
}

#[test]
fn extracts_title_and_author_from_decorated_file_name() {
    let metadata = parse(&content(
        "简介：……
经过一个多月的熬夜苦战，《沧源》这款游戏也终于通关。
",
        Some("/books/soushu2025.com@《望长天》(原名：仙子请听我解释)（完美校正版）作者：弥天大厦[搜书吧].txt"),
    ));
    assert_eq!(metadata.title.as_deref(), Some("望长天"));
    assert_eq!(metadata.author.as_deref(), Some("弥天大厦"));
}

#[test]
fn inline_book_brackets_are_not_a_title() {
    let metadata = parse(&content(
        "他玩的是《沧源》这款游戏。
",
        None,
    ));
    assert_eq!(metadata.title, None);
}

#[test]
fn text_metadata_wins_over_file_name() {
    let metadata = parse(&content(
        "《正文书名》
作者：正文作者
",
        Some("/books/《文件书名》作者：文件作者.txt"),
    ));
    assert_eq!(metadata.title.as_deref(), Some("正文书名"));
    assert_eq!(metadata.author.as_deref(), Some("正文作者"));
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
            SimpleMetadataParser::new().parse(&ct, content(text, None).view()),
            Err(crate::parser::ParserError::Cancelled)
        ));
    }
}

#[test]
fn isbn_check_digits_are_verified_and_separators_dropped() {
    use super::isbn;
    assert_eq!(isbn("978-7-02-000220-7").as_deref(), Some("9787020002207"));
    assert_eq!(isbn("ISBN：7-02-000220-X").as_deref(), Some("702000220X"));
    assert_eq!(isbn("7-02-000220-1"), None);
    assert_eq!(isbn("isbn 0-306-40615-2").as_deref(), Some("0306406152"));
    assert_eq!(isbn("0-8044-2957-x").as_deref(), Some("080442957X"));
    assert_eq!(isbn("978-7-02-000220-8"), None);
    assert_eq!(isbn("97870200022"), None);
    assert_eq!(isbn("978702000220７"), None);
}

#[test]
fn publication_dates_follow_the_epub_date_forms() {
    let valid = |published: &str| {
        super::Metadata {
            published: Some(published.into()),
            ..Default::default()
        }
        .validate()
        .is_ok()
    };
    for date in ["2024", "2024-02", "2024-02-29"] {
        assert!(valid(date), "{date}");
    }
    for date in ["24", "2024-13", "2023-02-29", "2024/02/01", "2024-2-1", ""] {
        assert!(!valid(date), "{date}");
    }
}
