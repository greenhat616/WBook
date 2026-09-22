use super::Extractor;
use chardetng::EncodingDetector;
use str_utils::EndsWithIgnoreAsciiCase;
use unicode_bom::Bom;

pub struct SimpleExtractor;

impl SimpleExtractor {
    pub fn new() -> Self {
        Self
    }

    fn detect_encoding(&self, first_bytes: &[u8]) -> String {
        // Try to find the Bom header
        let bom: Bom = first_bytes.into();
        todo!()
        // Continue with encoding detection using chardetng if no BOM is found
    }
}

impl Extractor for SimpleExtractor {
    fn accept(&self, path: &camino::Utf8Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.ends_with_ignore_ascii_case("txt"))
    }

    fn process(
        &self,
        ct: &tokio_util::sync::CancellationToken,
        opt: &super::ProcessOptions,
        path: &camino::Utf8Path,
    ) -> Result<super::ParsedContent, super::ExtractorError> {
        todo!()
    }
}
