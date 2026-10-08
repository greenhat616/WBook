use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::LazyLock;

use rayon::prelude::*;
use tera::{Context, Tera};
use tokio_util::sync::CancellationToken;

use super::plan::{xml_text, BookPlan};
use super::{check, CancelWriter, RenderLayout, Result, TemplateOverrides};
use crate::document::TextView;

static TEMPLATES: LazyLock<std::result::Result<Tera, tera::Error>> = LazyLock::new(|| {
    let mut tera = Tera::default();
    tera.autoescape_on([".xhtml", ".xml"]);
    // XML normalizes literal CR, including lone CR that is part of the source.
    tera.set_escape_fn(|text, writer| {
        for part in text.split_inclusive('\r') {
            if let Some(text) = part.strip_suffix('\r') {
                tera::escape_html(text, writer)?;
                writer.write_all(b"&#13;")?;
            } else {
                tera::escape_html(part, writer)?;
            }
        }
        Ok(())
    });
    tera.add_raw_templates([
        ("document.xhtml", include_str!("templates/document.xhtml")),
        ("section.xhtml", include_str!("templates/section.xhtml")),
        ("paragraph.xhtml", include_str!("templates/paragraph.xhtml")),
        (
            "nav-item.xhtml",
            "<li><a href=\"{{ href }}\">{{ title }}</a>",
        ),
        ("package.xml", include_str!("templates/package.xml")),
        ("cover.xhtml", include_str!("templates/cover.xhtml")),
    ])?;
    Ok(tera)
});

// Sections of a single-file layout render in parallel but are written in
// order, so a batch bounds how much rendered text is held in memory at once.
const SECTION_BATCH: usize = 256;
const FOOTER: &[u8] = b"</body></html>\n";

const STYLESHEET: &str = include_str!("templates/style.css");
pub(super) const COVER_IMAGE: &str = "images/cover.jpg";
pub(super) const COVER_PAGE: &str = "cover.xhtml";

fn builtin() -> Result<&'static Tera> {
    TEMPLATES
        .as_ref()
        .map_err(|error| super::ExportFailure::InvalidInput {
            message: format!("embedded template error: {error}"),
        })
}

/// Overrides replace built-in templates of the same name, so they keep the
/// built-in XML escaping and can still be rendered by the same code paths.
pub(super) fn templates(overrides: &TemplateOverrides) -> Result<Cow<'static, Tera>> {
    let tera = builtin()?;
    let custom: Vec<_> = [
        ("document.xhtml", &overrides.document),
        ("section.xhtml", &overrides.section),
        ("paragraph.xhtml", &overrides.paragraph),
    ]
    .into_iter()
    .filter_map(|(name, content)| content.as_deref().map(|content| (name, content)))
    .collect();
    if custom.is_empty() {
        return Ok(Cow::Borrowed(tera));
    }
    let mut tera = tera.clone();
    tera.add_raw_templates(custom)?;
    Ok(Cow::Owned(tera))
}

pub(super) fn defaults() -> TemplateOverrides {
    let builtin = |source: &str| Some(source.to_string());
    TemplateOverrides {
        stylesheet: builtin(STYLESHEET),
        document: builtin(include_str!("templates/document.xhtml")),
        section: builtin(include_str!("templates/section.xhtml")),
        paragraph: builtin(include_str!("templates/paragraph.xhtml")),
    }
}

pub(super) fn template(
    tera: &Tera,
    name: &str,
    context: &Context,
    writer: impl Write,
) -> Result<()> {
    Ok(tera.render_to(name, context, writer)?)
}

pub(super) fn builtin_template(name: &str, context: &Context, writer: impl Write) -> Result<()> {
    template(builtin()?, name, context, writer)
}

#[derive(Debug)]
pub struct RenderedBook {
    directory: tempfile::TempDir,
    pub(super) plan: BookPlan,
    pub(super) files: Vec<String>,
    pub(super) cover: bool,
}

impl RenderedBook {
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub fn content_files(&self) -> &[String] {
        &self.files
    }

    // Explicit close lets callers report cleanup failures; Drop is best effort.
    pub fn close(self) -> std::io::Result<()> {
        self.directory.close()
    }

    pub(super) fn section_file(&self, index: usize) -> &str {
        &self.files[if self.plan.layout == RenderLayout::SplitChapters {
            index
        } else {
            0
        }]
    }
}

pub(super) fn render(
    ct: &CancellationToken,
    view: TextView<'_>,
    plan: BookPlan,
    overrides: &TemplateOverrides,
    cover: Option<Vec<u8>>,
) -> Result<RenderedBook> {
    view.version().check(plan.version)?;
    let tera = templates(overrides)?;
    let directory = tempfile::Builder::new().prefix("wbook-render-").tempdir()?;
    fs::create_dir(directory.path().join("text"))?;
    fs::create_dir(directory.path().join("styles"))?;
    fs::write(
        directory.path().join("styles/book.css"),
        overrides.stylesheet.as_deref().unwrap_or(STYLESHEET),
    )?;
    let files = if plan.layout == RenderLayout::SplitChapters {
        plan.sections
            .iter()
            .map(|s| format!("text/{}.xhtml", s.id))
            .collect()
    } else {
        vec!["text/book.xhtml".into()]
    };
    let book = RenderedBook {
        directory,
        plan,
        files,
        cover: cover.is_some(),
    };
    if let Some(image) = cover {
        fs::create_dir(book.directory().join("images"))?;
        fs::write(book.directory().join(COVER_IMAGE), image)?;
        let mut context = Context::new();
        context.insert("book", &book.plan.metadata);
        let output = File::create(book.directory().join(COVER_PAGE))?;
        builtin_template("cover.xhtml", &context, BufWriter::new(output))?;
    }
    let mut context = Context::new();
    context.insert("book", &book.plan.metadata);
    context.insert("navigation", &false);
    let mut header = Vec::new();
    template(&tera, "document.xhtml", &context, &mut header)?;
    // is_cancelled locks the token, and paragraphs check it per line; a child
    // token per section keeps the threads off one mutex.
    let render_section = |index| section(&ct.child_token(), view, &book, &tera, index);
    if book.plan.layout == RenderLayout::SplitChapters {
        (0..book.plan.sections.len())
            .into_par_iter()
            .try_for_each(|index| -> Result<()> {
                let mut output = header.clone();
                output.extend(render_section(index)?);
                output.extend_from_slice(FOOTER);
                Ok(fs::write(
                    book.directory().join(&book.files[index]),
                    output,
                )?)
            })?;
    } else {
        let mut writer = CancelWriter {
            ct,
            inner: BufWriter::new(File::create(book.directory().join(&book.files[0]))?),
        };
        writer.write_all(&header)?;
        let indices: Vec<_> = (0..book.plan.sections.len()).collect();
        for batch in indices.chunks(SECTION_BATCH) {
            check(ct)?;
            let parts = batch
                .par_iter()
                .map(|&index| render_section(index))
                .collect::<Result<Vec<_>>>()?;
            for part in parts {
                writer.write_all(&part)?;
            }
        }
        writer.write_all(FOOTER)?;
        writer.flush()?;
    }
    navigation(ct, &book, &tera)?;
    check(ct)?;
    Ok(book)
}

fn section(
    ct: &CancellationToken,
    view: TextView<'_>,
    book: &RenderedBook,
    tera: &Tera,
    index: usize,
) -> Result<Vec<u8>> {
    check(ct)?;
    let section = &book.plan.sections[index];
    let mut output = Vec::new();
    let mut context = Context::new();
    context.insert("id", &section.id);
    context.insert("title", &section.title);
    context.insert("depth", &section.depth);
    context.insert("heading", &section.depth.clamp(1, 6));
    context.insert(
        "paged",
        &(book.plan.layout == RenderLayout::Paged && index > 0),
    );
    template(tera, "section.xhtml", &context, &mut output)?;
    if let Some(range) = section.body {
        for line in view.range_lines(ct, book.plan.version, range)? {
            let line = line?;
            let text = line
                .raw
                .strip_suffix("\r\n")
                .or_else(|| line.raw.strip_suffix('\n'))
                .unwrap_or(&line.raw)
                // Sources indent with full-width or ASCII spaces of varying
                // width; the stylesheet's text-indent is the only indent, or
                // pre-wrap would add the two together.
                .trim_start();
            xml_text(ct, text, &format!("source offset {}", line.range.start))?;
            let mut paragraph = Context::new();
            paragraph.insert("text", &text);
            paragraph.insert("empty", &text.is_empty());
            template(tera, "paragraph.xhtml", &paragraph, &mut output)?;
        }
    }
    output.extend_from_slice(b"</section>\n");
    Ok(output)
}

fn navigation(ct: &CancellationToken, book: &RenderedBook, tera: &Tera) -> Result<()> {
    let mut writer = CancelWriter {
        ct,
        inner: BufWriter::new(File::create(book.directory().join("nav.xhtml"))?),
    };
    let mut context = Context::new();
    context.insert("book", &book.plan.metadata);
    context.insert("navigation", &true);
    // The navigation resource is at the package root, unlike text resources.
    template(tera, "document.xhtml", &context, &mut writer)?;
    writer.write_all(b"<nav xmlns:epub=\"http://www.idpf.org/2007/ops\" epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol>\n")?;
    let mut depth = 0;
    for (index, section) in book
        .plan
        .sections
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.title.trim().is_empty())
    {
        check(ct)?;
        if section.depth > depth && depth > 0 {
            writer.write_all(b"<ol>\n")?;
        } else if depth > 0 {
            writer.write_all(b"</li>\n")?;
            for _ in section.depth..depth {
                writer.write_all(b"</ol></li>\n")?;
            }
        }
        depth = section.depth;
        context.insert("title", &section.title);
        context.insert(
            "href",
            &format!("{}#{}", book.section_file(index), section.id),
        );
        template(tera, "nav-item.xhtml", &context, &mut writer)?;
    }
    if depth == 0 {
        context.insert("title", &book.plan.metadata.title);
        context.insert(
            "href",
            &format!("{}#{}", book.section_file(0), book.plan.sections[0].id),
        );
        template(tera, "nav-item.xhtml", &context, &mut writer)?;
        depth = 1;
    }
    writer.write_all(b"</li>\n")?;
    for _ in 1..depth {
        writer.write_all(b"</ol></li>\n")?;
    }
    writer.write_all(b"</ol></nav></body></html>\n")?;
    writer.flush()?;
    Ok(())
}
