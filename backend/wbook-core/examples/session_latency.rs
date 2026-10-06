use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use wbook_core::export::{ExportOptions, OutputFormat, RenderLayout, RenderOptions};
use wbook_core::parser::toc::{chapter_only_config, TocParserConfig};
use wbook_core::session::{
    Activity, OpError, OperationId, OperationResult, Receipt, SessionHandle,
};
use wbook_core::workspace::{Phase, ProcessingOptions};
use wbook_core::{Params, Wbook};

const HANG_GUARD: Duration = Duration::from_secs(120);

fn generate(path: &Utf8PathBuf, bytes: usize) -> anyhow::Result<()> {
    let block = "正文 café 測量 cancellation latency.\n".repeat(1024);
    let mut file = BufWriter::new(File::create(path)?);
    for _ in 0..bytes / block.len() {
        file.write_all(block.as_bytes())?;
    }
    let mut end = bytes % block.len();
    while !block.is_char_boundary(end) {
        end -= 1;
    }
    file.write_all(&block.as_bytes()[..end])?;
    file.write_all(&vec![b' '; bytes % block.len() - end])?;
    file.flush()?;
    Ok(())
}

fn create(app: &Wbook, source: Utf8PathBuf) -> anyhow::Result<SessionHandle> {
    Ok(app.session_manager().create(
        source,
        ProcessingOptions {
            filters: vec![],
            toc: TocParserConfig::Levels(chapter_only_config()),
        },
    )?)
}

fn options() -> ExportOptions {
    ExportOptions {
        render: RenderOptions {
            layout: RenderLayout::SingleHtml,
        },
        format: OutputFormat::Epub,
        language: "zh-Hant".into(),
        identifier: Some("urn:wbook:latency".into()),
    }
}

async fn finish<T>(receipt: Receipt<T>) -> anyhow::Result<OperationResult<T>> {
    Ok(tokio::time::timeout(HANG_GUARD, receipt).await??)
}

async fn observed_phase(
    handle: &SessionHandle,
    op: OperationId,
    requested: Phase,
) -> anyhow::Result<Option<Phase>> {
    let mut receiver = handle.subscribe();
    let snapshot = tokio::time::timeout(HANG_GUARD, receiver.wait_for(|snapshot| {
        snapshot.activity == Activity::Idle || matches!(snapshot.activity,
            Activity::Running { op: active, phase: Some(phase), .. } if active == op && phase == requested)
    })).await??.clone();
    Ok(match snapshot.activity {
        Activity::Running { phase, .. } => phase,
        _ => None,
    })
}

fn outcome<T>(result: &OperationResult<T>) -> &'static str {
    match &result.outcome {
        Ok(_) => "succeeded",
        Err(OpError::Workspace { source: error }) if error.is_cancelled() => "cancelled",
        Err(OpError::Workspace { source: _ }) => "failed",
        Err(OpError::Panicked) => "panicked",
    }
}

async fn cancel<T>(
    handle: &SessionHandle,
    receipt: Receipt<T>,
    phase: Phase,
    mib: usize,
    operation: &str,
) -> anyhow::Result<()> {
    let observed = observed_phase(handle, receipt.op, phase).await?;
    let start = Instant::now();
    let reply = handle.cancel(receipt.op);
    let result = finish(receipt).await?;
    let elapsed = start.elapsed();
    println!(
        "{}",
        serde_json::json!({"mib": mib, "operation": operation, "observed_phase": observed, "reply": reply, "outcome": outcome(&result), "revision": result.revision.0, "cancel_to_receipt_ms": elapsed.as_secs_f64() * 1000.0})
    );
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    println!(
        "{}",
        serde_json::json!({"profile": if cfg!(debug_assertions) { "debug" } else { "release" }, "os": std::env::consts::OS, "logical_cpus": std::thread::available_parallelism()?.get(), "source": "exact-size UTF-8, repeated short lines", "trigger": "watch phase notification; no artificial delay"})
    );
    for mib in [10, 100] {
        let app = Wbook::new(Params {
            data_dir: Utf8PathBuf::from_path_buf(directory.path().join("unused-data")).unwrap(),
            config_dir: Utf8PathBuf::from_path_buf(directory.path().join("unused-config")).unwrap(),
        });
        let source =
            Utf8PathBuf::from_path_buf(directory.path().join(format!("latency-{mib}.txt")))
                .unwrap();
        generate(&source, mib * 1024 * 1024)?;
        let initializing = create(&app, source.clone())?;
        cancel(
            &initializing,
            initializing.initialize()?,
            Phase::Extracting,
            mib,
            "initialize",
        )
        .await?;
        anyhow::ensure!(initializing.close().await.cleanup_failures.is_empty());
        let prepared = create(&app, source.clone())?;
        let revision = finish(prepared.initialize()?).await?.outcome?;
        cancel(
            &prepared,
            prepared.render_preview(revision, options())?,
            Phase::Rendering,
            mib,
            "preview",
        )
        .await?;
        cancel(
            &prepared,
            prepared.export_epub(
                revision,
                options(),
                directory.path().join(format!("cancel-{mib}.epub")),
            )?,
            Phase::Exporting,
            mib,
            "export",
        )
        .await?;
        let closing_receipt = prepared.render_preview(revision, options())?;
        let phase = observed_phase(&prepared, closing_receipt.op, Phase::Rendering).await?;
        let start = Instant::now();
        let report = prepared.close().await;
        let elapsed = start.elapsed();
        let result = finish(closing_receipt).await?;
        println!(
            "{}",
            serde_json::json!({"mib": mib, "operation": "close-active-preview", "observed_phase": phase, "outcome": outcome(&result), "close_ms": elapsed.as_secs_f64() * 1000.0, "lost": report.lost, "cleanup_failures": report.cleanup_failures.len()})
        );
        let first = create(&app, source.clone())?;
        let second = create(&app, source)?;
        let a = first.initialize()?;
        let b = second.initialize()?;
        let (phase_a, phase_b) = tokio::join!(
            observed_phase(&first, a.op, Phase::Extracting),
            observed_phase(&second, b.op, Phase::Extracting)
        );
        let both_running = matches!(first.snapshot().activity, Activity::Running { .. })
            && matches!(second.snapshot().activity, Activity::Running { .. });
        let start_a = Instant::now();
        let reply_a = first.cancel(a.op);
        let start_b = Instant::now();
        let reply_b = second.cancel(b.op);
        let (result_a, result_b) = tokio::join!(
            async {
                let result = finish(a).await?;
                Ok::<_, anyhow::Error>((result, start_a.elapsed()))
            },
            async {
                let result = finish(b).await?;
                Ok::<_, anyhow::Error>((result, start_b.elapsed()))
            }
        );
        for (name, phase, reply, result) in [
            ("concurrent-initialize-a", phase_a?, reply_a, result_a?),
            ("concurrent-initialize-b", phase_b?, reply_b, result_b?),
        ] {
            println!(
                "{}",
                serde_json::json!({"mib": mib, "operation": name, "observed_phase": phase, "reply": reply, "outcome": outcome(&result.0), "cancel_to_receipt_ms": result.1.as_secs_f64() * 1000.0, "both_running_before_cancel": both_running})
            );
        }
        let start = Instant::now();
        let reports = app.shutdown().await;
        println!(
            "{}",
            serde_json::json!({"mib": mib, "operation": "shutdown-two-idle", "close_ms": start.elapsed().as_secs_f64() * 1000.0, "reports": reports.len()})
        );
        anyhow::ensure!(
            reports
                .iter()
                .all(|(_, report)| !report.lost && report.cleanup_failures.is_empty()),
            "shutdown cleanup failed"
        );
    }
    directory.close()?;
    Ok(())
}
