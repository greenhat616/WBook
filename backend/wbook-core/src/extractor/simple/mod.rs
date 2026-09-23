use super::{Content, Encoding, Extractor, ExtractorError, ParsedContent, ProcessOptions};
use camino::Utf8Path;
use str_utils::EndsWithIgnoreAsciiCase;
use tokio_util::sync::CancellationToken;

mod encoding;

#[cfg(test)]
mod tests;

pub struct SimpleExtractor;

impl SimpleExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SimpleExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for SimpleExtractor {
    fn accept(&self, path: &Utf8Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.ends_with_ignore_ascii_case("txt"))
    }

    fn process(
        &self,
        ct: &CancellationToken,
        _opt: &ProcessOptions,
        path: &Utf8Path,
    ) -> Result<ParsedContent, ExtractorError> {
        if ct.is_cancelled() {
            return Err(ExtractorError::Shutdown);
        }
        let bytes = std::fs::read(path)?;
        if ct.is_cancelled() {
            return Err(ExtractorError::Shutdown);
        }
        let (detected, bom_len) = encoding::detect_encoding(&bytes);
        let (text, actual) = encoding::decode_body(detected, &bytes[bom_len..]);
        Ok(ParsedContent {
            encoding: Encoding {
                name: actual.name().to_string(),
                bom: bom_len > 0,
            },
            content: Content::Text(text.into_owned()),
            source_path: Some(path.to_path_buf()),
        })
    }
}
