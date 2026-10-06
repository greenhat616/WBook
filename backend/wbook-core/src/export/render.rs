use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::LazyLock;

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
    ])?;
    Ok(tera)
});

const STYLESHEET: &str = include_str!("templates/style.css");

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
    };
    for (file_index, path) in book.files.iter().enumerate() {
        check(ct)?;
        let output = File::create(book.directory().join(path))?;
        let mut writer = CancelWriter {
            ct,
            inner: BufWriter::new(output),
        };
        let mut context = Context::new();
        context.insert("book", &book.plan.metadata);
        context.insert("navigation", &false);
        template(&tera, "document.xhtml", &context, &mut writer)?;
        let indices = if book.plan.layout == RenderLayout::SplitChapters {
            file_index..file_index + 1
        } else {
            0..book.plan.sections.len()
        };
        for index in indices {
            check(ct)?;
            let section = &book.plan.sections[index];
            context.insert("id", &section.id);
            context.insert("title", &section.title);
            context.insert("depth", &section.depth);
            context.insert("heading", &section.depth.clamp(1, 6));
            context.insert(
                "paged",
                &(book.plan.layout == RenderLayout::Paged && index > 0),
            );
            template(&tera, "section.xhtml", &context, &mut writer)?;
            if let Some(range) = section.body {
                for line in view.range_lines(ct, book.plan.version, range)? {
                    let line = line?;
                    let text = line
                        .raw
                        .strip_suffix("\r\n")
                        .or_else(|| line.raw.strip_suffix('\n'))
                        .unwrap_or(&line.raw);
                    xml_text(ct, text, &format!("source offset {}", line.range.start))?;
                    let mut paragraph = Context::new();
                    paragraph.insert("text", &text);
                    paragraph.insert("empty", &text.is_empty());
                    template(&tera, "paragraph.xhtml", &paragraph, &mut writer)?;
                }
            }
            writer.write_all(b"</section>\n")?;
        }
        writer.write_all(b"</body></html>\n")?;
        writer.flush()?;
    }
    navigation(ct, &book, &tera)?;
    check(ct)?;
    Ok(book)
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
