use super::super::{Content, Extractor, ExtractorError, ProcessOptions};
use super::SimpleExtractor;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use tokio_util::sync::CancellationToken;

struct TempFile(Utf8PathBuf);

impl TempFile {
    fn new(bytes: &[u8]) -> Self {
        let path = Utf8PathBuf::from_path_buf(
            std::env::temp_dir().join(format!("wbook-core-test-{}.txt", uuid::Uuid::new_v4())),
        )
        .unwrap();
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn process(bytes: &[u8]) -> Result<super::super::ParsedContent, ExtractorError> {
    let file = TempFile::new(bytes);
    SimpleExtractor::new().process(&CancellationToken::new(), &ProcessOptions {}, &file.0)
}

fn assert_text(content: &Content, expected: &str) {
    match content {
        Content::Text(text) => assert_eq!(text, expected),
    }
}

#[test]
fn test_accept() {
    let extractor = SimpleExtractor::new();
    assert!(extractor.accept(Utf8Path::new("a.txt")));
    assert!(extractor.accept(Utf8Path::new("a.TXT")));
    assert!(extractor.accept(Utf8Path::new("a.Txt")));
    assert!(extractor.accept(Utf8Path::new("dir/a.tXt")));
    assert!(!extractor.accept(Utf8Path::new("a.md")));
    assert!(!extractor.accept(Utf8Path::new("a.txt.md")));
    assert!(!extractor.accept(Utf8Path::new("a")));
}

#[test]
fn test_process_utf8_with_bom() {
    let bytes = b"\xEF\xBB\xBFhello, world";
    let parsed = process(bytes).unwrap();
    assert_eq!(parsed.encoding.name, "UTF-8");
    assert!(parsed.encoding.bom);
    assert_text(&parsed.content, "hello, world");
}

#[test]
fn test_process_utf8_without_bom() {
    let bytes = "hello, 世界".repeat(100);
    let parsed = process(bytes.as_bytes()).unwrap();
    assert_eq!(parsed.encoding.name, "UTF-8");
    assert!(!parsed.encoding.bom);
    assert_text(&parsed.content, &bytes);
}

#[test]
fn test_process_utf16le_with_bom() {
    let text = "你好，hello world";
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let parsed = process(&bytes).unwrap();
    assert_eq!(parsed.encoding.name, "UTF-16LE");
    assert!(parsed.encoding.bom);
    assert_text(&parsed.content, text);
}

#[test]
fn test_process_utf16be_with_bom() {
    let text = "你好，hello world";
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    let parsed = process(&bytes).unwrap();
    assert_eq!(parsed.encoding.name, "UTF-16BE");
    assert!(parsed.encoding.bom);
    assert_text(&parsed.content, text);
}

#[test]
fn test_process_gbk() {
    let text = "这是一段中文文本，用于测试编码检测功能。".repeat(50);
    let (bytes, _, had_unmappable) = encoding_rs::GBK.encode(&text);
    assert!(!had_unmappable);
    let parsed = process(&bytes).unwrap();
    assert_eq!(parsed.encoding.name, "GBK");
    assert!(!parsed.encoding.bom);
    assert_text(&parsed.content, &text);
}

#[test]
fn test_process_empty_file() {
    let parsed = process(b"").unwrap();
    assert_text(&parsed.content, "");
}

#[test]
fn test_process_cancelled() {
    let file = TempFile::new(b"hello");
    let ct = CancellationToken::new();
    ct.cancel();
    let err = SimpleExtractor::new()
        .process(&ct, &ProcessOptions {}, &file.0)
        .unwrap_err();
    assert!(matches!(err, ExtractorError::Shutdown));
}

#[test]
fn test_process_not_found() {
    let path = Utf8PathBuf::from_path_buf(
        std::env::temp_dir().join(format!("wbook-core-test-{}.txt", uuid::Uuid::new_v4())),
    )
    .unwrap();
    let err = SimpleExtractor::new()
        .process(&CancellationToken::new(), &ProcessOptions {}, &path)
        .unwrap_err();
    assert!(matches!(err, ExtractorError::Io(_)));
}

#[test]
fn test_decode_body_falls_back_to_system_encoding() {
    // \xFF is never valid UTF-8, so the lossless decode fails and the
    // fallback encoding takes over.
    let (_, encoding) = super::encoding::decode_body(encoding_rs::UTF_8, b"abc\xFFdef");
    assert_eq!(encoding, super::encoding::system_encoding());
}

#[test]
fn test_decode_body_lossless() {
    let (_, encoding) = super::encoding::decode_body(encoding_rs::UTF_8, "你好".as_bytes());
    assert_eq!(encoding, encoding_rs::UTF_8);
}

#[test]
fn test_parse_posix_codeset() {
    use super::encoding::parse_posix_codeset;
    assert_eq!(parse_posix_codeset("zh_CN.GBK"), Some("GBK"));
    assert_eq!(parse_posix_codeset("ja_JP.eucJP"), Some("eucJP"));
    assert_eq!(parse_posix_codeset("en_US.UTF-8@euro"), Some("UTF-8"));
    assert_eq!(parse_posix_codeset("zh_CN.GBK@pinyin"), Some("GBK"));
    assert_eq!(parse_posix_codeset("C"), None);
    assert_eq!(parse_posix_codeset("POSIX"), None);
    assert_eq!(parse_posix_codeset("zh_CN"), None);
    assert_eq!(parse_posix_codeset("zh_CN."), None);
}

#[test]
fn test_codeset_to_encoding() {
    use super::encoding::codeset_to_encoding;
    assert_eq!(codeset_to_encoding("GBK"), Some(encoding_rs::GBK));
    assert_eq!(
        codeset_to_encoding("gb18030"),
        Some(encoding_rs::GB18030)
    );
    // A glibc alias that is not a WHATWG label.
    assert_eq!(codeset_to_encoding("EUC-CN"), Some(encoding_rs::GBK));
    assert_eq!(codeset_to_encoding("Big5"), Some(encoding_rs::BIG5));
    assert_eq!(codeset_to_encoding("utf-8"), Some(encoding_rs::UTF_8));
    assert_eq!(
        codeset_to_encoding("ISO-8859-1"),
        Some(encoding_rs::WINDOWS_1252)
    );
    assert_eq!(codeset_to_encoding("bogus-codeset"), None);
}

#[test]
fn test_system_encoding() {
    assert!(!super::encoding::system_encoding().name().is_empty());
}
