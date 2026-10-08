use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use tokio_util::sync::CancellationToken;
use wbook_core::document::ProcessingDocument;
use wbook_core::export::{
    package_epub, render_book, ExportOptions, OutputFormat, RenderLayout, RenderOptions,
};
use wbook_core::extractor::{Content, Encoding, ParsedContent};
use wbook_core::parser::toc::volume_and_chapter;
use wbook_core::parser::SimpleMetadataParser;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/epub-examples"));
    let size_mib: usize = args.next().map(|s| s.parse()).transpose()?.unwrap_or(0);
    let layout = args.next().unwrap_or_else(|| "all".into());
    let text = if size_mib == 0 {
        include_str!("render_epub.txt").to_owned()
    } else {
        let mut text = String::with_capacity(size_mib * 1024 * 1024 + 64 * 1024);
        let line = "风从河面吹来，书页轻轻翻动。每一行文字都保留在自己的章节里。\n";
        let mut chapter = 0;
        while text.len() < size_mib * 1024 * 1024 {
            chapter += 1;
            text.push_str(&format!("第{chapter}章 测量\n"));
            for _ in 0..400 {
                text.push_str(line);
            }
        }
        text
    };
    let source_bytes = text.len();
    let ct = CancellationToken::new();
    let mut document = ProcessingDocument::new(
        ParsedContent {
            encoding: Encoding {
                name: "utf-8".into(),
                bom: false,
            },
            content: Content::Text(text),
            source_path: None,
        }
        .into(),
    );
    let parsed = document.parse(&ct, &volume_and_chapter(), &SimpleMetadataParser)?;
    document.install(&ct, parsed)?;
    document.metadata_overrides.title = Some("沿河书简 · WBook 渲染示例".into());
    document.metadata_overrides.author = Some("WBook 示例文本".into());
    fs::create_dir_all(&output)?;
    for (name, mode) in [
        ("split", RenderLayout::SplitChapters),
        ("paged", RenderLayout::Paged),
        ("single", RenderLayout::SingleHtml),
    ] {
        if layout != "all" && layout != name {
            continue;
        }
        let options = ExportOptions {
            render: RenderOptions {
                layout: mode,
                ..Default::default()
            },
            format: OutputFormat::Epub,
            language: "zh-Hans".into(),
            identifier: Some(format!("urn:wbook:example:along-the-river:{name}")),
            cover: Default::default(),
        };
        let start = Instant::now();
        let rendered = render_book(&ct, &document, &options)?;
        let render_ms = start.elapsed().as_millis();
        let resource_bytes: u64 = rendered
            .content_files()
            .iter()
            .map(|path| fs::metadata(rendered.directory().join(path)).map(|m| m.len()))
            .collect::<std::io::Result<Vec<_>>>()?
            .iter()
            .sum();
        let artifact = package_epub(&ct, &rendered, output.join(format!("wbook-{name}.epub")))?;
        let total_ms = start.elapsed().as_millis();
        if size_mib == 0 {
            let preview = output.join(format!("preview-{name}"));
            fs::create_dir_all(preview.join("text"))?;
            fs::create_dir_all(preview.join("styles"))?;
            for path in rendered
                .content_files()
                .iter()
                .map(String::as_str)
                .chain(["nav.xhtml", "styles/book.css"])
            {
                fs::copy(rendered.directory().join(path), preview.join(path))?;
            }
        }
        println!(
            "{}",
            serde_json::json!({"path": artifact.path, "layout": name, "source_bytes": source_bytes, "rendered_bytes": resource_bytes, "epub_bytes": fs::metadata(&artifact.path)?.len(), "render_ms": render_ms, "package_validate_publish_ms": total_ms - render_ms, "total_ms": total_ms})
        );
    }
    Ok(())
}
