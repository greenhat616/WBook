use std::fs::File;
use std::io::{self, Seek, Write};

use tera::Context;
use tokio_util::sync::CancellationToken;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use super::{check, render, CancelWriter, RenderedBook, Result};

const CONTAINER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" version=\"1.0\"><rootfiles><rootfile full-path=\"EPUB/package.opf\" media-type=\"application/oebps-package+xml\" /></rootfiles></container>";

pub(super) fn write(
    ct: &CancellationToken,
    book: &RenderedBook,
    output: impl Write + Seek,
) -> Result<()> {
    check(ct)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(
        "mimetype",
        options.compression_method(CompressionMethod::Stored),
    )?;
    zip.write_all(b"application/epub+zip")?;
    zip.start_file("META-INF/container.xml", options)?;
    zip.write_all(CONTAINER.as_bytes())?;
    for path in book
        .files
        .iter()
        .map(String::as_str)
        .chain(["nav.xhtml", "styles/book.css"])
    {
        check(ct)?;
        zip.start_file(format!("EPUB/{path}"), options)?;
        let mut input = File::open(book.directory().join(path))?;
        io::copy(
            &mut input,
            &mut CancelWriter {
                ct,
                inner: &mut zip,
            },
        )?;
    }
    check(ct)?;
    zip.start_file("EPUB/package.opf", options)?;
    let mut context = Context::new();
    context.insert("book", &book.plan.metadata);
    context.insert("files", &book.files);
    render::builtin_template(
        "package.xml",
        &context,
        CancelWriter {
            ct,
            inner: &mut zip,
        },
    )?;
    zip.finish()?.flush()?;
    check(ct)
}
