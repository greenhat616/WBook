use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Barrier};
use std::time::Duration;

use super::*;
use crate::app::{ManagerError, Params, SessionManager, Wbook};
use crate::document::{DocumentError, TextEdit};
use crate::export::{OutputFormat, RenderLayout, RenderOptions};
use crate::extractor::ExtractorError;
use crate::parser::toc::chapter_only_config;
use crate::toc::{Toc, TocSnapshot};
use crate::workspace::{DocumentStatus, ProcessingOptions};

const HANG_GUARD: Duration = Duration::from_secs(30);

async fn guard<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(HANG_GUARD, future)
        .await
        .expect("test did not finish")
}

async fn finish<T>(receipt: Receipt<T>) -> OperationResult<T> {
    guard(receipt).await.expect("receipt sender disappeared")
}

fn config() -> TocParserConfig {
    TocParserConfig::Levels(chapter_only_config())
}

fn options() -> ProcessingOptions {
    ProcessingOptions {
        filters: vec![],
        toc: config(),
    }
}

fn export_options() -> ExportOptions {
    ExportOptions {
        render: RenderOptions {
            layout: RenderLayout::SingleHtml,
        },
        format: OutputFormat::Epub,
        language: "en".into(),
        identifier: Some("urn:session-test".into()),
    }
}

fn source(directory: &tempfile::TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(directory.path().join("book.txt")).unwrap()
}

fn session(manager: &SessionManager) -> SessionHandle {
    manager
        .create("missing-session-test.txt".into(), options())
        .unwrap()
}

async fn wait_snapshot(
    handle: &SessionHandle,
    predicate: impl FnMut(&SessionSnapshot) -> bool,
) -> SessionSnapshot {
    let mut receiver = handle.subscribe();
    let snapshot = guard(receiver.wait_for(predicate)).await.unwrap().clone();
    snapshot
}

fn gate() -> (
    oneshot::Sender<()>,
    oneshot::Receiver<()>,
    mpsc::Sender<()>,
    mpsc::Receiver<()>,
) {
    let (started, observed) = oneshot::channel();
    let (release, blocked) = mpsc::channel();
    (started, observed, release, blocked)
}

#[tokio::test(flavor = "multi_thread")]
async fn current_preview_is_read_only_and_obeys_session_admission() {
    let manager = SessionManager::new(Handle::current());
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    fs::write(&path, "第一章 Start\nBody").unwrap();
    let handle = manager.create(path, options()).unwrap();
    assert_eq!(handle.current_preview().unwrap(), None);
    let revision = finish(handle.initialize().unwrap()).await.outcome.unwrap();
    let preview = finish(handle.render_preview(revision, export_options()).unwrap())
        .await
        .outcome
        .unwrap();
    let snapshot = handle.snapshot();
    assert_eq!(handle.current_preview().unwrap(), Some(preview.clone()));
    assert_eq!(handle.snapshot(), snapshot);

    let (started, observed, release, blocked) = gate();
    let receipt = handle
        .run(OpKind::ReadResults, move |_, _| {
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            Ok(())
        })
        .unwrap();
    guard(observed).await.unwrap();
    assert_eq!(handle.current_preview(), Err(Rejected::Busy));
    handle.request_close();
    assert_eq!(handle.current_preview(), Err(Rejected::Closing));
    release.send(()).unwrap();
    finish(receipt).await.outcome.unwrap();
    guard(handle.close()).await;
    assert_eq!(handle.current_preview(), Err(Rejected::Closed));
    assert!(!preview.directory.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn creation_is_lazy_config_is_validated_and_ids_are_not_reused() {
    let manager = SessionManager::new(Handle::current());
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    let invalid = ProcessingOptions {
        filters: vec![],
        toc: TocParserConfig::SplitEvenly { parts: 0 },
    };
    assert!(matches!(
        manager.create(path.clone(), invalid),
        Err(ManagerError::InvalidConfig(_))
    ));
    assert!(manager.list().is_empty());
    let handle = manager.create(path.clone(), options()).unwrap();
    assert!(!path.exists());
    assert_eq!(
        handle.snapshot().workspace_status,
        Availability::Available(Workspace::new(path.clone(), options()).unwrap().status())
    );
    let result = finish(handle.initialize().unwrap()).await;
    assert_eq!(result.revision, Revision(0));
    assert!(matches!(
        result.outcome,
        Err(OpError::Workspace(WorkspaceError::Extractor(
            ExtractorError::Io(_)
        )))
    ));
    assert_eq!(
        handle.snapshot().last.unwrap().category,
        ResultCategory::Failed
    );
    fs::write(&path, "第一章 Start\nBody").unwrap();
    assert_eq!(
        finish(handle.initialize().unwrap()).await.outcome.unwrap(),
        Revision(2)
    );
    let first = handle.id();
    guard(manager.close(first)).await.unwrap();
    assert!(matches!(manager.get(first), Err(ManagerError::NotFound)));
    let workspace = Workspace::new(path, options()).unwrap();
    let workspace_id = workspace.id();
    let reopened = manager.open(workspace).unwrap();
    assert!(reopened.id().0 > first.0);
    assert_eq!(reopened.workspace_id(), workspace_id);
    assert_eq!(manager.get(reopened.id()).unwrap().id(), reopened.id());
    assert_eq!(manager.list().len(), 1);
    assert_eq!(guard(manager.shutdown()).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_session_is_busy_while_two_sessions_and_control_operations_run_in_parallel() {
    let manager = SessionManager::new(Handle::current());
    let first = session(&manager);
    let second = session(&manager);
    let (started_a, observed_a, release_a, blocked_a) = gate();
    let (started_b, observed_b, release_b, blocked_b) = gate();
    let a = first
        .run(OpKind::ReadText, move |_, cx| {
            (cx.report)(Phase::Reading);
            started_a.send(()).unwrap();
            blocked_a.recv_timeout(HANG_GUARD).unwrap();
            assert!(cx.ct.is_cancelled());
            Ok(1)
        })
        .unwrap();
    let b = second
        .run(OpKind::ReadText, move |_, cx| {
            (cx.report)(Phase::Reading);
            started_b.send(()).unwrap();
            blocked_b.recv_timeout(HANG_GUARD).unwrap();
            assert!(!cx.ct.is_cancelled());
            Ok(2)
        })
        .unwrap();
    guard(observed_a).await.unwrap();
    guard(observed_b).await.unwrap();
    assert_eq!(first.initialize().unwrap_err(), Rejected::Busy);
    assert_eq!(second.read_results().unwrap_err(), Rejected::Busy);
    assert!(matches!(
        first.snapshot().activity,
        Activity::Running {
            phase: Some(Phase::Reading),
            ..
        }
    ));
    assert_eq!(*first.subscribe().borrow(), first.snapshot());
    assert_eq!(manager.list().len(), 2);
    assert_eq!(first.cancel(a.op), CancelReply::Requested);
    assert_eq!(first.cancel(a.op), CancelReply::Requested);
    assert!(matches!(
        first.snapshot().activity,
        Activity::Running {
            cancel_requested: true,
            ..
        }
    ));
    release_a.send(()).unwrap();
    release_b.send(()).unwrap();
    assert_eq!(finish(a).await.outcome.unwrap(), 1);
    assert_eq!(finish(b).await.outcome.unwrap(), 2);
    guard(manager.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_old_operations_cannot_cancel_new_ones_and_errors_keep_their_category() {
    let manager = SessionManager::new(Handle::current());
    let handle = session(&manager);
    let (started, observed, release, blocked) = gate();
    let first = handle
        .run::<()>(OpKind::ReadText, move |_, cx| {
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            assert!(cx.ct.is_cancelled());
            Err(DocumentError::Cancelled.into())
        })
        .unwrap();
    let old = first.op;
    guard(observed).await.unwrap();
    assert_eq!(handle.cancel(old), CancelReply::Requested);
    release.send(()).unwrap();
    assert!(matches!(
        finish(first).await.outcome,
        Err(OpError::Workspace(WorkspaceError::Document(
            DocumentError::Cancelled
        )))
    ));
    assert_eq!(
        handle.snapshot().last.unwrap().category,
        ResultCategory::Cancelled
    );
    let (started, observed, release, blocked) = gate();
    let next = handle
        .run::<()>(OpKind::ReadText, move |_, cx| {
            assert!(!cx.ct.is_cancelled());
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            assert!(cx.ct.is_cancelled());
            Err(WorkspaceError::NoDocument)
        })
        .unwrap();
    assert!(next.op.0 > old.0);
    guard(observed).await.unwrap();
    assert_eq!(handle.cancel(old), CancelReply::NotActive);
    assert!(matches!(
        handle.snapshot().activity,
        Activity::Running {
            cancel_requested: false,
            ..
        }
    ));
    assert_eq!(handle.cancel(next.op), CancelReply::Requested);
    release.send(()).unwrap();
    assert!(matches!(
        finish(next).await.outcome,
        Err(OpError::Workspace(WorkspaceError::NoDocument))
    ));
    assert_eq!(
        handle.snapshot().last.unwrap().category,
        ResultCategory::Failed
    );
    assert_eq!(handle.cancel(old), CancelReply::NotActive);
    guard(manager.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn committed_status_is_recomputed_outside_the_worker_and_snapshot_precedes_receipt() {
    let manager = SessionManager::new(Handle::current());
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    fs::write(&path, "Body").unwrap();
    let handle = manager.create(path, options()).unwrap();
    let initial = handle.snapshot();
    let (started, observed, release, blocked) = gate();
    let receipt = handle
        .run(OpKind::Initialize, move |ws, cx| {
            let revision = ws.initialize(cx)?;
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            Ok(revision)
        })
        .unwrap();
    let op = receipt.op;
    guard(observed).await.unwrap();
    let running = handle.snapshot();
    assert_eq!(running.workspace_status, initial.workspace_status);
    assert!(running.seq > initial.seq);
    release.send(()).unwrap();
    let result = finish(receipt).await;
    let completed = handle.snapshot();
    assert_eq!(completed.activity, Activity::Idle);
    assert_eq!(completed.last.as_ref().unwrap().op, op);
    assert_eq!(completed.last.as_ref().unwrap().revision, result.revision);
    assert_eq!(completed.last.unwrap().category, ResultCategory::Succeeded);
    assert!(completed.seq > running.seq);
    assert!(matches!(
        completed.workspace_status,
        Availability::Available(WorkspaceStatus {
            document: DocumentStatus::Current,
            revision: Revision(2),
            ..
        })
    ));
    let roundtrip: SessionSnapshot =
        serde_json::from_str(&serde_json::to_string(&handle.snapshot()).unwrap()).unwrap();
    assert_eq!(roundtrip, handle.snapshot());
    guard(manager.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn receipts_are_independent_and_dropping_receipts_handles_or_subscribers_does_not_cancel() {
    let manager = SessionManager::new(Handle::current());
    let handle = session(&manager);
    let first = handle.run(OpKind::ReadText, |_, _| Ok(11)).unwrap();
    let first_op = first.op;
    wait_snapshot(&handle, |snapshot| {
        snapshot.activity == Activity::Idle
            && snapshot
                .last
                .as_ref()
                .is_some_and(|last| last.op == first_op)
    })
    .await;
    let second = handle.run(OpKind::ReadText, |_, _| Ok(22)).unwrap();
    assert_eq!(finish(second).await.outcome.unwrap(), 22);
    assert_eq!(finish(first).await.outcome.unwrap(), 11);
    let (started, observed, release, blocked) = gate();
    let dropped = handle
        .run(OpKind::ReadText, move |_, cx| {
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            assert!(!cx.ct.is_cancelled());
            Ok(33)
        })
        .unwrap();
    let op = dropped.op;
    drop(dropped);
    drop(handle.subscribe());
    drop(handle.clone());
    guard(observed).await.unwrap();
    release.send(()).unwrap();
    let snapshot = wait_snapshot(&handle, |snapshot| {
        snapshot.last.as_ref().is_some_and(|last| last.op == op)
    })
    .await;
    assert_eq!(snapshot.lifecycle, LifecycleState::Open);
    assert_eq!(snapshot.last.unwrap().category, ResultCategory::Succeeded);
    let id = handle.id();
    drop(handle);
    assert_eq!(
        manager.get(id).unwrap().snapshot().lifecycle,
        LifecycleState::Open
    );
    guard(manager.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn progress_from_an_old_operation_does_not_overwrite_new_or_terminal_state() {
    let manager = SessionManager::new(Handle::current());
    let handle = session(&manager);
    let first = handle.run(OpKind::ReadText, |_, _| Ok(())).unwrap();
    let old_progress = worker::progress(Arc::downgrade(&handle.0), first.op);
    finish(first).await.outcome.unwrap();
    let before = handle.snapshot();
    old_progress(Phase::Exporting);
    assert_eq!(handle.snapshot(), before);
    let (started, observed, release, blocked) = gate();
    let next = handle
        .run(OpKind::ReadText, move |_, cx| {
            (cx.report)(Phase::Reading);
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            Ok(())
        })
        .unwrap();
    guard(observed).await.unwrap();
    let before = handle.snapshot();
    old_progress(Phase::Exporting);
    assert_eq!(handle.snapshot(), before);
    release.send(()).unwrap();
    finish(next).await.outcome.unwrap();
    guard(handle.close()).await;
    let before = handle.snapshot();
    old_progress(Phase::Extracting);
    assert_eq!(handle.snapshot(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn discarded_operation_results_are_dropped_without_holding_the_session_lock() {
    struct Reenter {
        handle: SessionHandle,
        observed: Option<oneshot::Sender<()>>,
    }
    impl Drop for Reenter {
        fn drop(&mut self) {
            assert_eq!(self.handle.cancel(OperationId(0)), CancelReply::NotActive);
            self.observed.take().unwrap().send(()).unwrap();
        }
    }
    let manager = SessionManager::new(Handle::current());
    let handle = session(&manager);
    let (started, observed, release, blocked) = gate();
    let (dropped, observed_drop) = oneshot::channel();
    let reenter = Reenter {
        handle: handle.clone(),
        observed: Some(dropped),
    };
    let receipt = handle
        .run(OpKind::ReadText, move |_, _| {
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            Ok(reenter)
        })
        .unwrap();
    guard(observed).await.unwrap();
    drop(receipt);
    release.send(()).unwrap();
    guard(observed_drop).await.unwrap();
    assert_eq!(
        handle.snapshot().last.unwrap().category,
        ResultCategory::Succeeded
    );
    guard(manager.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn panic_completes_receipt_loses_only_its_workspace_and_allows_close() {
    let manager = SessionManager::new(Handle::current());
    let failed = session(&manager);
    let healthy = session(&manager);
    let receipt = failed
        .run::<()>(OpKind::Initialize, |_, _| panic!("injected worker panic"))
        .unwrap();
    let op = receipt.op;
    let result = finish(receipt).await;
    assert!(matches!(result.outcome, Err(OpError::Panicked)));
    let snapshot = failed.snapshot();
    assert_eq!(snapshot.activity, Activity::Idle);
    assert_eq!(
        snapshot.workspace_status,
        Availability::Lost {
            last_revision: Revision(0)
        }
    );
    assert_eq!(
        snapshot.last.unwrap(),
        OperationSummary {
            op,
            kind: OpKind::Initialize,
            revision: Revision(0),
            category: ResultCategory::Panicked
        }
    );
    assert_eq!(failed.initialize().unwrap_err(), Rejected::Unavailable);
    assert_eq!(failed.current_preview(), Err(Rejected::Unavailable));
    assert_eq!(failed.cancel(op), CancelReply::NotActive);
    assert_eq!(*failed.subscribe().borrow(), failed.snapshot());
    assert_eq!(
        finish(healthy.run(OpKind::ReadText, |_, _| Ok(42)).unwrap())
            .await
            .outcome
            .unwrap(),
        42
    );
    let report = guard(failed.close()).await;
    assert!(report.lost);
    assert!(Arc::ptr_eq(&report, &guard(failed.close()).await));
    assert!(matches!(
        manager.get(failed.id()),
        Err(ManagerError::NotFound)
    ));
    assert_eq!(failed.initialize().unwrap_err(), Rejected::Closed);
    guard(manager.shutdown()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_idle_closes_share_a_report_and_old_handles_are_read_only() {
    let manager = SessionManager::new(Handle::current());
    let handle = session(&manager);
    let (first, second) = guard(async { tokio::join!(handle.close(), handle.close()) }).await;
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!first.lost);
    assert!(first.last.is_none());
    assert!(first.cleanup_failures.is_empty());
    assert_eq!(handle.snapshot().lifecycle, LifecycleState::Closed);
    assert_eq!(*handle.subscribe().borrow(), handle.snapshot());
    assert_eq!(handle.initialize().unwrap_err(), Rejected::Closed);
    assert_eq!(handle.read_results().unwrap_err(), Rejected::Closed);
    assert!(matches!(
        manager.get(handle.id()),
        Err(ManagerError::NotFound)
    ));
    assert!(matches!(
        manager.close(handle.id()).await,
        Err(ManagerError::NotFound)
    ));
    let before = handle.snapshot();
    assert!(Arc::ptr_eq(&first, &guard(handle.close()).await));
    assert_eq!(handle.snapshot(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn running_close_waits_for_worker_return_and_survives_a_dropped_close_future() {
    let manager = SessionManager::new(Handle::current());
    let handle = session(&manager);
    let returned = Arc::new(AtomicBool::new(false));
    let worker_returned = returned.clone();
    let (started, observed, release, blocked) = gate();
    let receipt = handle
        .run(OpKind::ReadText, move |_, cx| {
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            assert!(cx.ct.is_cancelled());
            worker_returned.store(true, Ordering::SeqCst);
            Ok("completed after cancellation")
        })
        .unwrap();
    guard(observed).await.unwrap();
    let closing_handle = handle.clone();
    let abandoned = tokio::spawn(async move { closing_handle.close().await });
    wait_snapshot(&handle, |snapshot| {
        snapshot.lifecycle == LifecycleState::Closing
    })
    .await;
    assert!(!returned.load(Ordering::SeqCst));
    assert!(!abandoned.is_finished());
    assert_eq!(handle.initialize().unwrap_err(), Rejected::Closing);
    abandoned.abort();
    assert!(guard(abandoned).await.unwrap_err().is_cancelled());
    let second_handle = handle.clone();
    let second = tokio::spawn(async move { second_handle.close().await });
    release.send(()).unwrap();
    let result = finish(receipt).await;
    assert_eq!(result.outcome.unwrap(), "completed after cancellation");
    let report = guard(second).await.unwrap();
    assert!(returned.load(Ordering::SeqCst));
    assert_eq!(
        report.last.as_ref().unwrap().category,
        ResultCategory::Succeeded
    );
    assert!(Arc::ptr_eq(&report, &guard(handle.close()).await));
    assert!(matches!(
        manager.get(handle.id()),
        Err(ManagerError::NotFound)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn publication_completed_during_a_running_operation_is_preserved_by_close() {
    let manager = SessionManager::new(Handle::current());
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    fs::write(&path, "第一章 Start\nBody").unwrap();
    let handle = manager.create(path.clone(), options()).unwrap();
    let revision = finish(handle.initialize().unwrap()).await.outcome.unwrap();
    let preview = finish(handle.render_preview(revision, export_options()).unwrap())
        .await
        .outcome
        .unwrap();
    let destination = directory.path().join("published.epub");
    let output = destination.clone();
    let (started, observed, release, blocked) = gate();
    let receipt = handle
        .run(OpKind::ExportEpub, move |ws, cx| {
            let artifact = ws.export_epub(cx, revision, export_options(), output)?;
            started.send(()).unwrap();
            blocked.recv_timeout(HANG_GUARD).unwrap();
            assert!(cx.ct.is_cancelled());
            Ok(artifact)
        })
        .unwrap();
    guard(observed).await.unwrap();
    let published = fs::read(&destination).unwrap();
    handle.request_close();
    assert_eq!(handle.snapshot().lifecycle, LifecycleState::Closing);
    assert!(preview.directory.exists());
    release.send(()).unwrap();
    let result = finish(receipt).await;
    assert_eq!(result.outcome.unwrap().artifact.path, destination);
    let report = guard(handle.close()).await;
    assert_eq!(
        report.last.as_ref().unwrap().category,
        ResultCategory::Succeeded
    );
    assert_eq!(fs::read(&destination).unwrap(), published);
    assert!(path.exists());
    assert!(!preview.directory.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_requests_every_close_before_waiting_for_any_worker() {
    let manager = Arc::new(SessionManager::new(Handle::current()));
    let first = session(&manager);
    let second = session(&manager);
    let (started_a, observed_a, release_a, blocked_a) = gate();
    let (started_b, observed_b, release_b, blocked_b) = gate();
    let a = first
        .run(OpKind::ReadText, move |_, cx| {
            started_a.send(()).unwrap();
            blocked_a.recv_timeout(HANG_GUARD).unwrap();
            assert!(cx.ct.is_cancelled());
            Ok(())
        })
        .unwrap();
    let b = second
        .run(OpKind::ReadText, move |_, cx| {
            started_b.send(()).unwrap();
            blocked_b.recv_timeout(HANG_GUARD).unwrap();
            assert!(cx.ct.is_cancelled());
            Ok(())
        })
        .unwrap();
    guard(observed_a).await.unwrap();
    guard(observed_b).await.unwrap();
    let stopping = manager.clone();
    let shutdown = tokio::spawn(async move { stopping.shutdown().await });
    wait_snapshot(&first, |snapshot| {
        snapshot.lifecycle == LifecycleState::Closing
    })
    .await;
    wait_snapshot(&second, |snapshot| {
        snapshot.lifecycle == LifecycleState::Closing
    })
    .await;
    assert!(!shutdown.is_finished());
    assert!(matches!(
        manager.create("unused.txt".into(), options()),
        Err(ManagerError::ShuttingDown)
    ));
    release_a.send(()).unwrap();
    release_b.send(()).unwrap();
    finish(a).await.outcome.unwrap();
    finish(b).await.outcome.unwrap();
    let reports = guard(shutdown).await.unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|(_, report)| !report.lost));
    assert!(manager.list().is_empty());
    assert!(guard(manager.shutdown()).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn create_racing_shutdown_never_leaves_an_accepted_session_open() {
    for _ in 0..32 {
        let manager = Arc::new(SessionManager::new(Handle::current()));
        let barrier = Arc::new(Barrier::new(2));
        let creating = manager.clone();
        let creator_barrier = barrier.clone();
        let creator = tokio::task::spawn_blocking(move || {
            creator_barrier.wait();
            creating.create("unread-racing-source.txt".into(), options())
        });
        let stopping = manager.clone();
        let shutdown = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || barrier.wait())
                .await
                .unwrap();
            stopping.shutdown().await
        });
        let created = guard(creator).await.unwrap();
        let reports = guard(shutdown).await.unwrap();
        match created {
            Ok(handle) => {
                assert_eq!(reports.len(), 1);
                assert_eq!(reports[0].0, handle.id());
                assert_eq!(handle.snapshot().lifecycle, LifecycleState::Closed);
                assert!(Arc::ptr_eq(&reports[0].1, &guard(handle.close()).await));
                assert!(matches!(
                    manager.get(handle.id()),
                    Err(ManagerError::NotFound)
                ));
            }
            Err(ManagerError::ShuttingDown) => assert!(reports.is_empty()),
            Err(error) => panic!("unexpected creation error: {error}"),
        }
        assert!(manager.list().is_empty());
    }
}

#[test]
fn dropping_manager_or_wbook_outside_a_runtime_context_is_nonblocking_and_breaks_no_cycles() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let manager = SessionManager::new(runtime.handle().clone());
    let handle = session(&manager);
    let registry = handle.0.registry.clone();
    drop(manager);
    runtime.block_on(guard(handle.close()));
    assert!(registry.upgrade().is_none());
    let params = Params {
        data_dir: "unused-data".into(),
        config_dir: "unused-config".into(),
    };
    let app = {
        let _entered = runtime.enter();
        Wbook::new(params.clone())
    };
    assert_eq!(app.start_params, params);
    let handle = session(app.session_manager());
    let registry = handle.0.registry.clone();
    drop(app);
    runtime.block_on(guard(handle.close()));
    assert!(registry.upgrade().is_none());
    let stopped_manager = SessionManager::new(runtime.handle().clone());
    let stopped_handle = session(&stopped_manager);
    drop(runtime);
    drop(stopped_manager);
    assert_eq!(stopped_handle.snapshot().lifecycle, LifecycleState::Closing);
}

#[tokio::test(flavor = "multi_thread")]
async fn typed_forwarders_return_workspace_results_and_wbook_shuts_down() {
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    fs::write(&path, "第一章 Start\nBody").unwrap();
    let app = Wbook::new(Params {
        data_dir: "unused-data".into(),
        config_dir: "unused-config".into(),
    });
    let handle = app.session_manager().create(path, options()).unwrap();
    let mut revision = finish(handle.initialize().unwrap()).await.outcome.unwrap();
    let results = finish(handle.read_results().unwrap())
        .await
        .outcome
        .unwrap();
    assert!(results.current);
    let version = results.results.unwrap().version;
    assert_eq!(
        finish(
            handle
                .read_text(version, TextRange { start: 0, end: 3 })
                .unwrap()
        )
        .await
        .outcome
        .unwrap(),
        "第"
    );
    revision = finish(
        handle
            .set_metadata_overrides(
                revision,
                Metadata {
                    title: Some("Manual".into()),
                    author: None,
                },
            )
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    revision = finish(
        handle
            .apply_edits(
                revision,
                EditBatch {
                    base: version,
                    edits: vec![TextEdit {
                        range: TextRange { start: 0, end: 0 },
                        insert: "Preface\n".into(),
                    }],
                },
            )
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    let parsed = finish(handle.parse(config()).unwrap())
        .await
        .outcome
        .unwrap();
    revision = finish(handle.install(revision, parsed).unwrap())
        .await
        .outcome
        .unwrap();
    let preview = finish(handle.render_preview(revision, export_options()).unwrap())
        .await
        .outcome
        .unwrap();
    let output = directory.path().join("typed.epub");
    let artifact = finish(
        handle
            .export_epub(revision, export_options(), output.clone())
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    assert_eq!(artifact.artifact.path, output);
    assert_eq!(artifact.revision, revision);
    assert_eq!(guard(app.shutdown()).await.len(), 1);
    assert!(!preview.directory.exists());
    assert!(output.exists());
    assert!(!directory.path().join("unused-data").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn public_session_flow_keeps_manual_results_and_overrides_after_export() {
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    fs::write(&path, "第一章 Start\nBody\n第二章 End\nTail").unwrap();
    let app = Wbook::new(Params {
        data_dir: "unused-data".into(),
        config_dir: "unused-config".into(),
    });
    let handle = app
        .session_manager()
        .create(path.clone(), options())
        .unwrap();
    let mut revision = finish(handle.initialize().unwrap()).await.outcome.unwrap();
    let installed = finish(handle.read_results().unwrap())
        .await
        .outcome
        .unwrap()
        .results
        .unwrap();
    revision = finish(
        handle
            .apply_edits(
                revision,
                EditBatch {
                    base: installed.version,
                    edits: vec![TextEdit {
                        range: TextRange { start: 0, end: 0 },
                        insert: "Preface\n".into(),
                    }],
                },
            )
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    let mut parsed = finish(handle.parse(config()).unwrap())
        .await
        .outcome
        .unwrap();
    let id = TocSnapshot::from(&parsed.toc)[0].id;
    parsed.toc.get_mut(id).unwrap().title = "Manual TOC".into();
    parsed.metadata.title = Some("Automatic adjusted title".into());
    revision = finish(handle.install(revision, parsed).unwrap())
        .await
        .outcome
        .unwrap();
    revision = finish(
        handle
            .set_metadata_overrides(
                revision,
                Metadata {
                    title: Some("Override title".into()),
                    author: None,
                },
            )
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    let preview = finish(handle.render_preview(revision, export_options()).unwrap())
        .await
        .outcome
        .unwrap();
    let output = directory.path().join("public-flow.epub");
    let artifact = finish(
        handle
            .export_epub(revision, export_options(), output.clone())
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    assert_eq!(artifact.revision, revision);
    let results = finish(handle.read_results().unwrap())
        .await
        .outcome
        .unwrap();
    let installed = results.results.unwrap();
    assert_eq!(TocSnapshot::from(&installed.toc)[0].title, "Manual TOC");
    assert_eq!(
        installed.metadata.title.as_deref(),
        Some("Automatic adjusted title")
    );
    assert_eq!(results.overrides.title.as_deref(), Some("Override title"));
    assert_eq!(
        finish(handle.render_preview(revision, export_options()).unwrap())
            .await
            .outcome
            .unwrap(),
        preview
    );
    revision = finish(
        handle
            .apply_edits(
                revision,
                EditBatch {
                    base: installed.version,
                    edits: vec![TextEdit {
                        range: TextRange { start: 0, end: 0 },
                        insert: "Later edit\n".into(),
                    }],
                },
            )
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    assert_eq!(revision, Revision(6));
    assert!(!preview.directory.exists());
    assert!(
        !finish(handle.read_results().unwrap())
            .await
            .outcome
            .unwrap()
            .current
    );
    assert!(guard(handle.close()).await.cleanup_failures.is_empty());
    assert!(path.exists() && output.exists());
    assert!(guard(app.shutdown()).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn workspace_allocation_is_moved_and_snapshots_contain_no_body_toc_or_history() {
    let manager = SessionManager::new(Handle::current());
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    let marker = "UNIQUE_BODY_MARKER";
    fs::write(&path, format!("第一章 Heading\n{}", marker.repeat(10000))).unwrap();
    let handle = manager.create(path, options()).unwrap();
    finish(handle.initialize().unwrap()).await.outcome.unwrap();
    let mut allocations = Vec::new();
    for _ in 0..3 {
        allocations.push(
            finish(
                handle
                    .run(OpKind::ReadText, |ws, _| Ok(ws as *mut Workspace as usize))
                    .unwrap(),
            )
            .await
            .outcome
            .unwrap(),
        );
    }
    assert!(allocations.iter().all(|address| *address == allocations[0]));
    let snapshot = serde_json::to_value(handle.snapshot()).unwrap();
    let keys: Vec<_> = snapshot
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "seq",
            "session",
            "workspace",
            "source",
            "lifecycle",
            "activity",
            "workspace_status",
            "last"
        ]
    );
    let status = snapshot["workspace_status"]["Available"]
        .as_object()
        .unwrap();
    let keys: Vec<_> = status.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "revision",
            "document",
            "document_version",
            "filters",
            "has_overrides",
            "preview"
        ]
    );
    let keys: Vec<_> = snapshot["last"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["op", "kind", "revision", "category"]);
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains(marker));
    assert!(!encoded.contains("Heading"));
    assert!(encoded.len() < 2048);
    guard(manager.shutdown()).await;
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn operation_warnings_survive_business_failure_and_close_reports_cleanup_failures() {
    use std::os::windows::fs::OpenOptionsExt;
    let manager = SessionManager::new(Handle::current());
    let directory = tempfile::tempdir().unwrap();
    let path = source(&directory);
    fs::write(&path, "Body").unwrap();
    let handle = manager.create(path, options()).unwrap();
    let revision = finish(handle.initialize().unwrap()).await.outcome.unwrap();
    let preview = finish(handle.render_preview(revision, export_options()).unwrap())
        .await
        .outcome
        .unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(preview.directory.join(&preview.files[0]))
        .unwrap();
    let result = finish(
        handle
            .run::<()>(OpKind::SetMetadataOverrides, move |ws, cx| {
                ws.set_metadata_overrides(
                    cx,
                    revision,
                    Metadata {
                        title: Some("Changed".into()),
                        author: None,
                    },
                )?;
                Err(WorkspaceError::NoDocument)
            })
            .unwrap(),
    )
    .await;
    assert_eq!(result.revision, Revision(revision.0 + 1));
    assert!(matches!(
        result.outcome,
        Err(OpError::Workspace(WorkspaceError::NoDocument))
    ));
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].path, preview.directory);
    assert!(matches!(
        handle.snapshot().workspace_status,
        Availability::Available(WorkspaceStatus {
            has_overrides: true,
            ..
        })
    ));
    assert!(finish(handle.read_results().unwrap())
        .await
        .warnings
        .is_empty());
    drop(held);
    fs::remove_dir_all(preview.directory).unwrap();
    let preview = finish(
        handle
            .render_preview(result.revision, export_options())
            .unwrap(),
    )
    .await
    .outcome
    .unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(preview.directory.join(&preview.files[0]))
        .unwrap();
    let report = guard(handle.close()).await;
    assert_eq!(report.cleanup_failures.len(), 1);
    assert_eq!(report.cleanup_failures[0].path, preview.directory);
    assert!(!report.cleanup_failures[0].message.is_empty());
    assert!(!report.lost);
    assert_eq!(handle.snapshot().lifecycle, LifecycleState::Closed);
    drop(held);
    fs::remove_dir_all(preview.directory).unwrap();
}
