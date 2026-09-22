use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use std::borrow::Cow;
use unicode_bom::Bom;

/// The number of leading bytes fed to the encoding detector.
const DETECTION_WINDOW: usize = 8192;

/// Map a detected BOM to a WHATWG encoding, if the BOM belongs to an
/// encoding supported by `encoding_rs`.
fn encoding_from_bom(bom: Bom) -> Option<&'static encoding_rs::Encoding> {
    match bom {
        Bom::Utf8 => Some(encoding_rs::UTF_8),
        Bom::Utf16Le => Some(encoding_rs::UTF_16LE),
        Bom::Utf16Be => Some(encoding_rs::UTF_16BE),
        // The WHATWG standard routes the GBK and gb18030 labels to the same decoder.
        Bom::Gb18030 => Some(encoding_rs::GBK),
        _ => None,
    }
}

/// Detect the encoding of `bytes`.
///
/// Returns the encoding and the byte length of the BOM (0 when no BOM was
/// found). A BOM always wins; otherwise chardetng guesses the encoding.
/// Note that chardetng never guesses UTF-16, so BOM sniffing is the only
/// detection path for UTF-16 content.
pub(super) fn detect_encoding(bytes: &[u8]) -> (&'static encoding_rs::Encoding, usize) {
    let bom = Bom::from(bytes);
    if let Some(encoding) = encoding_from_bom(bom) {
        return (encoding, bom.len());
    }
    let window = &bytes[..bytes.len().min(DETECTION_WINDOW)];
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
    detector.feed(window, true);
    (detector.guess(None, Utf8Detection::Allow), 0)
}

/// Decode `body` with `encoding`, falling back to the system encoding when
/// the content cannot be decoded without replacement characters.
pub(super) fn decode_body<'a>(
    encoding: &'static encoding_rs::Encoding,
    body: &'a [u8],
) -> (Cow<'a, str>, &'static encoding_rs::Encoding) {
    match encoding.decode_without_bom_handling_and_without_replacement(body) {
        Some(text) => (text, encoding),
        None => {
            let fallback = system_encoding();
            let (text, _) = fallback.decode_without_bom_handling(body);
            (text, fallback)
        }
    }
}

/// The encoding of the system's ANSI code page.
///
/// On Windows this resolves `GetACP` to a WHATWG encoding, defaulting to GBK
/// when the code page has no WHATWG counterpart. Elsewhere UTF-8 is assumed,
/// matching the default locale of modern Unix-likes.
pub(super) fn system_encoding() -> &'static encoding_rs::Encoding {
    #[cfg(windows)]
    {
        static CACHE: std::sync::OnceLock<&'static encoding_rs::Encoding> =
            std::sync::OnceLock::new();
        CACHE.get_or_init(|| {
            // SAFETY: GetACP has no preconditions.
            let acp = unsafe { windows_sys::Win32::Globalization::GetACP() };
            codepage_to_encoding(acp).unwrap_or(encoding_rs::GBK)
        })
    }
    #[cfg(not(windows))]
    {
        encoding_rs::UTF_8
    }
}

/// Map a Windows code page identifier to a WHATWG encoding.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn codepage_to_encoding(codepage: u32) -> Option<&'static encoding_rs::Encoding> {
    match codepage {
        936 => return Some(encoding_rs::GBK),
        950 => return Some(encoding_rs::BIG5),
        932 => return Some(encoding_rs::SHIFT_JIS),
        949 => return Some(encoding_rs::EUC_KR),
        65001 => return Some(encoding_rs::UTF_8),
        _ => {}
    }
    let label = format!("windows-{codepage}");
    encoding_rs::Encoding::for_label(label.as_bytes())
}
