//! Cover images: the generated default cover, and custom images with an
//! optional title overlay, encoded as the JPEG that goes into the package.

use std::io::Cursor;
use std::sync::LazyLock;

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, Limits, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum CoverKind {
    None,
    #[default]
    Generated,
    /// The image the user chose for this book.
    Image,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CoverSettings {
    pub kind: CoverKind,
    /// Draws the title and author over a custom image; the generated cover
    /// always shows them.
    pub overlay: bool,
    /// E-ink readers show color covers with poor contrast.
    pub grayscale: bool,
}

/// Larger uploads are rejected before decoding.
pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = 12_000;
const GENERATED: (u32, u32) = (1600, 2400);
/// Kindle's recommended cover size; larger images only add weight.
const MAX_OUTPUT: (u32, u32) = (1600, 2560);
const QUALITY: u8 = 90;

#[derive(Debug, snafu::Snafu)]
pub enum CoverError {
    #[snafu(display("cover image is larger than {} MiB", MAX_IMAGE_BYTES / 1024 / 1024))]
    TooLarge,
    #[snafu(display("unsupported or damaged cover image: {source}"))]
    Decode { source: image::ImageError },
    #[snafu(display("the cover is set to a custom image, but none was chosen"))]
    MissingImage,
    #[snafu(display("cannot encode the cover: {source}"))]
    Encode { source: image::ImageError },
}

/// Decodes `bytes` to make sure the image is usable before it is stored.
pub fn check_image(bytes: &[u8]) -> Result<(), CoverError> {
    decode(bytes).map(drop)
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, CoverError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(CoverError::TooLarge);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| CoverError::Decode {
            source: error.into(),
        })?;
    reader.limits(limits);
    reader
        .decode()
        .map_err(|source| CoverError::Decode { source })
}

/// Renders the cover as JPEG, or `None` for a book without a cover.
pub fn render(
    settings: &CoverSettings,
    image: Option<&[u8]>,
    title: &str,
    author: Option<&str>,
) -> Result<Option<Vec<u8>>, CoverError> {
    let canvas = match settings.kind {
        CoverKind::None => return Ok(None),
        CoverKind::Generated => generated(title, author),
        CoverKind::Image => {
            let image = decode(image.ok_or(CoverError::MissingImage)?)?;
            let image = if image.width() > MAX_OUTPUT.0 || image.height() > MAX_OUTPUT.1 {
                image.resize(MAX_OUTPUT.0, MAX_OUTPUT.1, FilterType::Lanczos3)
            } else {
                image
            };
            let mut canvas = image.to_rgba8();
            if settings.overlay {
                overlay(&mut canvas, title, author);
            }
            canvas
        }
    };
    let image = if settings.grayscale {
        DynamicImage::ImageLuma8(DynamicImage::ImageRgba8(canvas).to_luma8())
    } else {
        // JPEG has no alpha channel.
        DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(canvas).to_rgb8())
    };
    let mut output = Vec::new();
    // `encode_image` would widen a DynamicImage to RGB; this keeps grayscale.
    image
        .write_with_encoder(JpegEncoder::new_with_quality(&mut output, QUALITY))
        .map_err(|source| CoverError::Encode { source })?;
    Ok(Some(output))
}

const PALETTE: [[u8; 3]; 6] = [
    [0x1f, 0x3a, 0x5f],
    [0x6b, 0x1e, 0x2e],
    [0x23, 0x4d, 0x3a],
    [0x33, 0x33, 0x3d],
    [0x1d, 0x50, 0x5c],
    [0x4a, 0x2c, 0x5a],
];

fn generated(title: &str, author: Option<&str>) -> RgbaImage {
    let (width, height) = GENERATED;
    // Books differ in color but each keeps its own across exports.
    let hash = title.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    let base = PALETTE[hash as usize % PALETTE.len()];
    let mut canvas = RgbaImage::from_fn(width, height, |_, y| {
        let shade = 1.0 - 0.35 * y as f32 / height as f32;
        let [r, g, b] = base.map(|channel| (f32::from(channel) * shade) as u8);
        Rgba([r, g, b, 255])
    });
    let margin = width / 20;
    frame(&mut canvas, margin, 4, [255, 255, 255], 0.45);
    let text_width = width as f32 - 4.0 * margin as f32;
    if let Some(font) = FONT.as_ref() {
        let title_size = width as f32 * 0.09;
        let lines = wrap(font, title_size, title, text_width, 4);
        let line_height = title_size * 1.3;
        let top = height as f32 * 0.36 - line_height * lines.len() as f32 / 2.0;
        for (index, line) in lines.iter().enumerate() {
            let baseline = top + line_height * (index as f32 + 0.8);
            draw_line(
                &mut canvas,
                font,
                title_size,
                line,
                baseline,
                [255, 255, 255],
            );
        }
        if let Some(author) = author {
            let size = width as f32 * 0.045;
            for (index, line) in wrap(font, size, author, text_width, 2).iter().enumerate() {
                let baseline = height as f32 * 0.74 + size * 1.4 * index as f32;
                draw_line(&mut canvas, font, size, line, baseline, [235, 235, 235]);
            }
        }
    }
    canvas
}

fn overlay(canvas: &mut RgbaImage, title: &str, author: Option<&str>) {
    let (width, height) = canvas.dimensions();
    let Some(font) = FONT.as_ref() else { return };
    let title_size = width as f32 * 0.08;
    let author_size = width as f32 * 0.042;
    let text_width = width as f32 * 0.86;
    let lines = wrap(font, title_size, title, text_width, 2);
    let authors = author.map_or_else(Vec::new, |author| {
        wrap(font, author_size, author, text_width, 1)
    });
    let block = title_size * 1.3 * lines.len() as f32 + author_size * 1.8 * authors.len() as f32;
    let top = (height as f32 - block - title_size * 1.2).max(0.0);
    // Darken the bottom so the text reads on any picture.
    for y in top as u32..height {
        let fade = ((y as f32 - top) / (title_size * 1.5)).min(1.0);
        for x in 0..width {
            blend(canvas, x as i64, y as i64, [0, 0, 0], 0.55 * fade);
        }
    }
    let mut baseline = top + title_size * 1.3;
    for line in &lines {
        draw_line(canvas, font, title_size, line, baseline, [255, 255, 255]);
        baseline += title_size * 1.3;
    }
    for line in &authors {
        baseline += author_size * 0.3;
        draw_line(canvas, font, author_size, line, baseline, [230, 230, 230]);
    }
}

/// The first installed CJK-capable face. Bundling a CJK font would add
/// megabytes to the app, and every desktop system ships one of these.
static FONT: LazyLock<Option<FontVec>> = LazyLock::new(|| {
    use fontdb::{Database, Family, Query, Weight};
    let mut database = Database::new();
    database.load_system_fonts();
    let families = [
        Family::Name("Microsoft YaHei"),
        Family::Name("PingFang SC"),
        Family::Name("Hiragino Sans GB"),
        Family::Name("Noto Sans CJK SC"),
        Family::Name("Source Han Sans SC"),
        Family::Name("Noto Sans SC"),
        Family::Name("WenQuanYi Micro Hei"),
        Family::Name("SimHei"),
        Family::SansSerif,
    ];
    let id = database.query(&Query {
        families: &families,
        weight: Weight::BOLD,
        ..Query::default()
    });
    let font = id.and_then(|id| {
        database.with_face_data(id, |data, index| {
            FontVec::try_from_vec_and_index(data.to_vec(), index).ok()
        })
    });
    let font = font.flatten();
    if font.is_none() {
        tracing::warn!("No system font found; covers are drawn without text");
    }
    font
});

fn advance(font: &FontVec, size: f32, text: &str) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut previous = None;
    text.chars()
        .map(|ch| {
            let id = font.glyph_id(ch);
            let kern = previous.map_or(0.0, |previous| scaled.kern(previous, id));
            previous = Some(id);
            kern + scaled.h_advance(id)
        })
        .sum()
}

/// Breaks at any character, which suits CJK titles; a Latin word moves to
/// the next line whole when it can. Text beyond `max_lines` ends in `…`.
fn wrap(font: &FontVec, size: f32, text: &str, width: f32, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for ch in text.trim().chars() {
        line.push(ch);
        if advance(font, size, &line) <= width || line.chars().count() == 1 {
            continue;
        }
        line.pop();
        let rest = match line.rfind(' ').filter(|_| ch.is_ascii_alphanumeric()) {
            Some(space) if space > 0 => line.split_off(space + 1),
            _ => String::new(),
        };
        lines.push(line.trim_end().to_owned());
        line = rest;
        // A space where the line breaks would indent the next line.
        if !(line.is_empty() && ch.is_whitespace()) {
            line.push(ch);
        }
    }
    if !line.trim().is_empty() {
        lines.push(line);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        let last = lines.last_mut().expect("max_lines is positive");
        while !last.is_empty() && advance(font, size, &format!("{last}…")) > width {
            last.pop();
        }
        last.push('…');
    }
    lines
}

fn draw_line(
    canvas: &mut RgbaImage,
    font: &FontVec,
    size: f32,
    text: &str,
    baseline: f32,
    color: [u8; 3],
) {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut x = (canvas.width() as f32 - advance(font, size, text)) / 2.0;
    let mut previous = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(previous) = previous {
            x += scaled.kern(previous, id);
        }
        previous = Some(id);
        let glyph = id.with_scale_and_position(size, point(x, baseline));
        x += scaled.h_advance(id);
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                blend(
                    canvas,
                    bounds.min.x as i64 + i64::from(gx),
                    bounds.min.y as i64 + i64::from(gy),
                    color,
                    coverage,
                );
            });
        }
    }
}

fn frame(canvas: &mut RgbaImage, inset: u32, thickness: u32, color: [u8; 3], alpha: f32) {
    let (width, height) = canvas.dimensions();
    for y in inset..height - inset {
        for x in inset..width - inset {
            let edge = x < inset + thickness
                || y < inset + thickness
                || x >= width - inset - thickness
                || y >= height - inset - thickness;
            if edge {
                blend(canvas, i64::from(x), i64::from(y), color, alpha);
            }
        }
    }
}

fn blend(canvas: &mut RgbaImage, x: i64, y: i64, color: [u8; 3], alpha: f32) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    let Some(pixel) = canvas.get_pixel_mut_checked(x, y) else {
        return;
    };
    let alpha = alpha.clamp(0.0, 1.0);
    for (channel, value) in pixel.0.iter_mut().zip(color) {
        *channel = (f32::from(*channel) * (1.0 - alpha) + f32::from(value) * alpha).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb([200, 40, 40]),
        ))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
        bytes
    }

    fn settings(kind: CoverKind, overlay: bool, grayscale: bool) -> CoverSettings {
        CoverSettings {
            kind,
            overlay,
            grayscale,
        }
    }

    fn jpeg(bytes: Option<Vec<u8>>) -> DynamicImage {
        let bytes = bytes.expect("a cover");
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Jpeg
        );
        image::load_from_memory(&bytes).unwrap()
    }

    #[test]
    fn no_cover_renders_nothing() {
        let image = png(10, 10);
        let cover = render(
            &settings(CoverKind::None, true, true),
            Some(&image),
            "书",
            None,
        );
        assert!(cover.unwrap().is_none());
    }

    #[test]
    fn generated_cover_has_a_fixed_size_and_needs_no_image() {
        let cover = render(
            &settings(CoverKind::Generated, false, false),
            None,
            "测试书名",
            Some("作者"),
        );
        assert_eq!(jpeg(cover.unwrap()).dimensions(), GENERATED);
    }

    #[test]
    fn custom_images_keep_their_shape_and_shrink_to_the_output_limit() {
        let small = png(300, 450);
        let cover = render(
            &settings(CoverKind::Image, true, false),
            Some(&small),
            "书名",
            Some("作者"),
        );
        assert_eq!(jpeg(cover.unwrap()).dimensions(), (300, 450));
        let large = png(3200, 4000);
        let cover = render(
            &settings(CoverKind::Image, false, false),
            Some(&large),
            "书名",
            None,
        );
        assert_eq!(jpeg(cover.unwrap()).dimensions(), (1600, 2000));
    }

    #[test]
    fn grayscale_covers_are_encoded_without_color() {
        let image = png(60, 90);
        let cover = render(
            &settings(CoverKind::Image, false, true),
            Some(&image),
            "书",
            None,
        );
        assert_eq!(jpeg(cover.unwrap()).color(), image::ColorType::L8);
        let cover = render(
            &settings(CoverKind::Image, false, false),
            Some(&image),
            "书",
            None,
        );
        assert_eq!(jpeg(cover.unwrap()).color(), image::ColorType::Rgb8);
    }

    #[test]
    fn unusable_images_are_rejected() {
        assert!(matches!(
            check_image(b"not an image"),
            Err(CoverError::Decode { .. })
        ));
        assert!(matches!(
            check_image(&vec![0; MAX_IMAGE_BYTES + 1]),
            Err(CoverError::TooLarge)
        ));
        assert!(check_image(&png(4, 4)).is_ok());
        assert!(matches!(
            render(&settings(CoverKind::Image, false, false), None, "书", None),
            Err(CoverError::MissingImage)
        ));
    }

    #[test]
    fn long_titles_wrap_and_end_with_an_ellipsis() {
        // Hosts without a CJK-capable font draw no text at all.
        let Some(font) = FONT.as_ref() else { return };
        let lines = wrap(font, 100.0, "一二三四五六七八九十", 350.0, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| advance(font, 100.0, line) <= 350.0));
        assert!(lines[1].ends_with('…'));
        assert_eq!(wrap(font, 10.0, "Short title", 1000.0, 2), ["Short title"]);
        let words = wrap(
            font,
            20.0,
            "alpha beta gamma",
            advance(font, 20.0, "alpha beta g"),
            3,
        );
        assert_eq!(words, ["alpha beta", "gamma"]);
        let width = advance(font, 20.0, "abc");
        assert_eq!(
            wrap(font, 20.0, "一二三 四", width * 0.9, 3)
                .iter()
                .filter(|l| l.starts_with(' '))
                .count(),
            0
        );
    }
}
