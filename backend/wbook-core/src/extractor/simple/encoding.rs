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
        Bom::Gb18030 => Some(encoding_rs::GB18030),
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
/// when the code page has no WHATWG counterpart. On Unix-likes the codeset is
/// read from the locale environment (`LC_ALL`, `LC_CTYPE`, `LANG`), so legacy
/// locales such as `zh_CN.GBK` are honored; UTF-8 is assumed when the locale
/// carries no mappable codeset.
pub(super) fn system_encoding() -> &'static encoding_rs::Encoding {
    static CACHE: std::sync::OnceLock<&'static encoding_rs::Encoding> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(detect_system_encoding)
}

fn detect_system_encoding() -> &'static encoding_rs::Encoding {
    #[cfg(windows)]
    {
        // SAFETY: GetACP has no preconditions.
        let acp = unsafe { windows_sys::Win32::Globalization::GetACP() };
        u16::try_from(acp)
            .ok()
            .and_then(codepage::to_encoding_no_replacement)
            .unwrap_or(encoding_rs::GBK)
    }
    #[cfg(not(windows))]
    {
        locale_codeset()
            .as_deref()
            .and_then(codeset_to_encoding)
            .unwrap_or(encoding_rs::UTF_8)
    }
}

/// The codeset portion of the locale environment, e.g. `GBK` for
/// `zh_CN.GBK`.
#[cfg(not(windows))]
fn locale_codeset() -> Option<String> {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .find_map(|value| parse_posix_codeset(&value).map(str::to_owned))
}

/// Extract the codeset from a POSIX locale string like `zh_CN.GBK@pinyin`.
#[cfg(any(not(windows), test))]
pub(super) fn parse_posix_codeset(locale: &str) -> Option<&str> {
    let codeset = locale.split_once('.')?.1.split('@').next()?.trim();
    if codeset.is_empty() {
        None
    } else {
        Some(codeset)
    }
}

/// Map a Unix codeset (as reported by `nl_langinfo(CODESET)` or the locale
/// environment) to a WHATWG encoding.
#[cfg(any(not(windows), test))]
pub(super) fn codeset_to_encoding(codeset: &str) -> Option<&'static encoding_rs::Encoding> {
    // Aliases used by glibc that are not WHATWG labels.
    if codeset.eq_ignore_ascii_case("euc-cn") {
        return Some(encoding_rs::GBK);
    }
    encoding_rs::Encoding::for_label(codeset.as_bytes())
}
