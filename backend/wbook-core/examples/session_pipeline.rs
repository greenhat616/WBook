use std::fs;

use camino::Utf8PathBuf;
use wbook_core::document::{EditBatch, TextEdit};
use wbook_core::parser::toc::TocMode;
use wbook_core::parser::Metadata;
use wbook_core::session::{Availability, LifecycleState, Receipt};
use wbook_core::settings::Settings;
use wbook_core::toc::{Toc, TocSnapshot};
use wbook_core::types::TextRange;
use wbook_core::workspace::{DocumentStatus, FilterConfig};
use wbook_core::{Params, Wbook};

async fn completed<T>(receipt: Receipt<T>) -> anyhow::Result<T> {
    let result = receipt.await?;
    anyhow::ensure!(
        result.warnings.is_empty(),
        "operation cleanup warnings: {:?}",
        result.warnings
    );
    Ok(result.outcome?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = Utf8PathBuf::from_path_buf(directory.path().join("book.txt")).unwrap();
    let text = "书名：Session Example\n第一章 Start\nOriginal body.\n第二章 Finish\nFinal body.\n";
    fs::write(&source, text)?;
    let app = Wbook::new(Params {
        data_dir: Utf8PathBuf::from_path_buf(directory.path().join("unused-data")).unwrap(),
        config_dir: Utf8PathBuf::from_path_buf(directory.path().join("unused-config")).unwrap(),
    });
    let mut settings = Settings::default();
    settings.toc.mode = TocMode::Chapters;
    settings.filters = vec![FilterConfig::Ad];
    settings.render.language = "en".into();
    let handle = app.session_manager().create(source.clone(), settings)?;
    let mut revision = completed(handle.initialize()?).await?;
    let initial = completed(handle.read_results()?).await?.results.unwrap();
    let start = text.find("Original").unwrap() as u64;
    revision = completed(handle.apply_edits(
        revision,
        EditBatch {
            base: initial.version,
            edits: vec![TextEdit {
                range: TextRange {
                    start,
                    end: start + "Original".len() as u64,
                },
                insert: "Edited".into(),
            }],
        },
    )?)
    .await?;
    let parsed = completed(handle.parse()?).await?;
    revision = completed(handle.install(revision, parsed)?).await?;
    let mut adjusted = completed(handle.read_results()?).await?.results.unwrap();
    let id = TocSnapshot::from(&adjusted.toc)[0].id;
    adjusted.toc.get_mut(id).unwrap().title = "Manual chapter title".into();
    adjusted.metadata.title = Some("Adjusted automatic title".into());
    revision = completed(handle.install(revision, adjusted)?).await?;
    revision = completed(handle.set_metadata_overrides(
        revision,
        Metadata {
            title: Some("Session pipeline example".into()),
            author: Some("WBook".into()),
            ..Metadata::default()
        },
    )?)
    .await?;
    let preview = completed(handle.render_preview(revision)?).await?;
    let destination = directory.path().join("book.epub");
    let artifact = completed(handle.export_epub(revision, destination.clone())?).await?;
    anyhow::ensure!(
        artifact.artifact.cleanup_failures.is_empty(),
        "export cleanup failed"
    );
    let exported_bytes = fs::metadata(&destination)?.len();
    let current = completed(handle.read_results()?).await?.results.unwrap();
    revision = completed(handle.apply_edits(
        revision,
        EditBatch {
            base: current.version,
            edits: vec![TextEdit {
                range: TextRange { start: 0, end: 0 },
                insert: "Preface\n".into(),
            }],
        },
    )?)
    .await?;
    anyhow::ensure!(matches!(handle.snapshot().workspace_status,
        Availability::Available(status) if status.document == DocumentStatus::Stale));
    anyhow::ensure!(
        !preview.directory.exists(),
        "edit did not invalidate preview"
    );
    let report = handle.close().await;
    anyhow::ensure!(
        !report.lost && report.cleanup_failures.is_empty(),
        "close failed"
    );
    anyhow::ensure!(
        source.exists() && destination.exists(),
        "close deleted input or output"
    );
    let shutdown = app.shutdown().await;
    println!("session_pipeline: exported_bytes={exported_bytes}, export_revision={}, final_revision={}, preview_invalidated=true, closed={}, shutdown_reports={}",
        artifact.revision.0, revision.0, handle.snapshot().lifecycle == LifecycleState::Closed, shutdown.len());
    directory.close()?;
    Ok(())
}
