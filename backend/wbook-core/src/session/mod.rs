use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::app::session_manager::ManagerShared;
use crate::document::{DocumentVersion, EditBatch, ParsedResults};
use crate::export::{CleanupFailure, ExportOptions};
use crate::parser::toc::TocParserConfig;
use crate::parser::Metadata;
use crate::types::TextRange;
use crate::workspace::{
    ExportArtifact, OpContext, Phase, PreviewInfo, Revision, Workspace, WorkspaceError,
    WorkspaceId, WorkspaceResults, WorkspaceStatus,
};

mod worker;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct OperationId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum OpKind {
    Initialize,
    Parse,
    Install,
    Edit,
    SetMetadataOverrides,
    ReadText,
    ReadResults,
    RenderPreview,
    ExportEpub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum LifecycleState {
    Open,
    Closing,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Activity {
    Idle,
    Running {
        op: OperationId,
        kind: OpKind,
        phase: Option<Phase>,
        cancel_requested: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum Availability {
    // Boxed because render options, including template overrides, make the
    // status much larger than the lost marker.
    Available(Box<WorkspaceStatus>),
    Lost { last_revision: Revision },
}

impl Availability {
    fn revision(&self) -> Revision {
        match self {
            Self::Available(status) => status.revision,
            Self::Lost { last_revision } => *last_revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ResultCategory {
    Succeeded,
    Failed,
    Cancelled,
    Panicked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationSummary {
    pub op: OperationId,
    pub kind: OpKind,
    pub revision: Revision,
    pub category: ResultCategory,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SessionSnapshot {
    pub seq: u64,
    pub session: SessionId,
    pub workspace: WorkspaceId,
    pub source: Utf8PathBuf,
    pub lifecycle: LifecycleState,
    pub activity: Activity,
    pub workspace_status: Availability,
    pub last: Option<OperationSummary>,
}

#[derive(Debug)]
pub struct Receipt<T> {
    pub op: OperationId,
    rx: oneshot::Receiver<OperationResult<T>>,
}

impl<T> Future for Receipt<T> {
    type Output = Result<OperationResult<T>, oneshot::error::RecvError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().rx).poll(cx)
    }
}

#[derive(Debug)]
pub struct OperationResult<T> {
    pub op: OperationId,
    pub kind: OpKind,
    pub revision: Revision,
    pub outcome: Result<T, OpError>,
    pub warnings: Vec<CleanupFailure>,
}

#[derive(Debug, snafu::Snafu)]
pub enum OpError {
    #[snafu(context(false), display("{source}"))]
    Workspace { source: WorkspaceError },
    #[snafu(display("workspace worker panicked or was stopped; side effects are unknown"))]
    Panicked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, snafu::Snafu)]
pub enum Rejected {
    #[snafu(display("session is busy"))]
    Busy,
    #[snafu(display("workspace is unavailable"))]
    Unavailable,
    #[snafu(display("session is closing"))]
    Closing,
    #[snafu(display("session is closed"))]
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum CancelReply {
    Requested,
    NotActive,
}

#[derive(Debug)]
pub struct CloseReport {
    pub last: Option<OperationSummary>,
    pub lost: bool,
    pub cleanup_failures: Vec<CleanupFailure>,
}

#[derive(Clone)]
pub struct SessionHandle(Arc<Shared>);

impl fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionHandle")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

struct Shared {
    id: SessionId,
    workspace_id: WorkspaceId,
    source: Utf8PathBuf,
    runtime: Handle,
    inner: Mutex<Inner>,
    snapshot: watch::Sender<SessionSnapshot>,
    registry: Weak<ManagerShared>,
}

struct Inner {
    lifecycle: Lifecycle,
    slot: Slot,
    next_op: u64,
    last: Option<OperationSummary>,
}

enum Lifecycle {
    Open,
    Closing,
    Closed(Arc<CloseReport>),
}

enum Slot {
    Idle(Box<Workspace>),
    Busy(ActiveOp),
    Lost,
}

struct ActiveOp {
    id: OperationId,
    kind: OpKind,
    phase: Option<Phase>,
    ct: CancellationToken,
}

impl SessionHandle {
    pub(crate) fn new(
        id: SessionId,
        workspace: Workspace,
        workspace_id: WorkspaceId,
        source: Utf8PathBuf,
        status: WorkspaceStatus,
        runtime: Handle,
        registry: Weak<ManagerShared>,
    ) -> Self {
        let snapshot = watch::channel(SessionSnapshot {
            seq: 0,
            session: id,
            workspace: workspace_id,
            source: source.clone(),
            lifecycle: LifecycleState::Open,
            activity: Activity::Idle,
            workspace_status: Availability::Available(Box::new(status)),
            last: None,
        })
        .0;
        Self(Arc::new(Shared {
            id,
            workspace_id,
            source,
            runtime,
            inner: Mutex::new(Inner {
                lifecycle: Lifecycle::Open,
                slot: Slot::Idle(Box::new(workspace)),
                next_op: 1,
                last: None,
            }),
            snapshot,
            registry,
        }))
    }

    pub fn id(&self) -> SessionId {
        self.0.id
    }
    pub fn workspace_id(&self) -> WorkspaceId {
        self.0.workspace_id
    }
    pub fn source(&self) -> &Utf8Path {
        &self.0.source
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        self.0.snapshot.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<SessionSnapshot> {
        self.0.snapshot.subscribe()
    }

    pub fn current_preview(&self) -> Result<Option<PreviewInfo>, Rejected> {
        let inner = self.0.inner.lock().unwrap();
        match inner.lifecycle {
            Lifecycle::Closing => return Err(Rejected::Closing),
            Lifecycle::Closed(_) => return Err(Rejected::Closed),
            Lifecycle::Open => {}
        }
        match &inner.slot {
            Slot::Idle(workspace) => Ok(workspace.current_preview()),
            Slot::Busy(_) => Err(Rejected::Busy),
            Slot::Lost => Err(Rejected::Unavailable),
        }
    }

    pub fn initialize(&self) -> Result<Receipt<Revision>, Rejected> {
        self.run(OpKind::Initialize, |ws, cx| ws.initialize(cx))
    }

    pub fn parse(&self, config: TocParserConfig) -> Result<Receipt<ParsedResults>, Rejected> {
        self.run(OpKind::Parse, move |ws, cx| ws.parse(cx, config))
    }

    pub fn install(
        &self,
        expected: Revision,
        results: ParsedResults,
    ) -> Result<Receipt<Revision>, Rejected> {
        self.run(OpKind::Install, move |ws, cx| {
            ws.install(cx, expected, results)
        })
    }

    pub fn apply_edits(
        &self,
        expected: Revision,
        batch: EditBatch,
    ) -> Result<Receipt<Revision>, Rejected> {
        self.run(OpKind::Edit, move |ws, cx| {
            ws.apply_edits(cx, expected, batch)
                .map(|(revision, _)| revision)
        })
    }

    pub fn set_metadata_overrides(
        &self,
        expected: Revision,
        overrides: Metadata,
    ) -> Result<Receipt<Revision>, Rejected> {
        self.run(OpKind::SetMetadataOverrides, move |ws, cx| {
            ws.set_metadata_overrides(cx, expected, overrides)
        })
    }

    pub fn read_text(
        &self,
        version: DocumentVersion,
        range: TextRange,
    ) -> Result<Receipt<String>, Rejected> {
        self.run(OpKind::ReadText, move |ws, cx| {
            ws.read_text(cx, version, range)
        })
    }

    pub fn read_results(&self) -> Result<Receipt<WorkspaceResults>, Rejected> {
        self.run(OpKind::ReadResults, |ws, cx| ws.results(cx))
    }

    pub fn render_preview(
        &self,
        expected: Revision,
        options: ExportOptions,
    ) -> Result<Receipt<PreviewInfo>, Rejected> {
        self.run(OpKind::RenderPreview, move |ws, cx| {
            ws.render_preview(cx, expected, options)
        })
    }

    pub fn export_epub(
        &self,
        expected: Revision,
        options: ExportOptions,
        destination: PathBuf,
    ) -> Result<Receipt<ExportArtifact>, Rejected> {
        self.run(OpKind::ExportEpub, move |ws, cx| {
            ws.export_epub(cx, expected, options, destination)
        })
    }

    pub fn cancel(&self, op: OperationId) -> CancelReply {
        let mut inner = self.0.inner.lock().unwrap();
        match &mut inner.slot {
            Slot::Busy(active) if active.id == op => {
                active.ct.cancel();
                self.0.publish(&inner, None);
                CancelReply::Requested
            }
            _ => CancelReply::NotActive,
        }
    }

    pub async fn close(&self) -> Arc<CloseReport> {
        self.request_close();
        let mut snapshot = self.subscribe();
        snapshot
            .wait_for(|snapshot| snapshot.lifecycle == LifecycleState::Closed)
            .await
            .expect("session owns its snapshot sender");
        let inner = self.0.inner.lock().unwrap();
        match &inner.lifecycle {
            Lifecycle::Closed(report) => report.clone(),
            _ => unreachable!("closed snapshot must have a close report"),
        }
    }
}

impl Shared {
    fn publish(&self, inner: &Inner, availability: Option<Availability>) {
        self.snapshot.send_modify(|snapshot| {
            snapshot.seq += 1;
            snapshot.lifecycle = match inner.lifecycle {
                Lifecycle::Open => LifecycleState::Open,
                Lifecycle::Closing => LifecycleState::Closing,
                Lifecycle::Closed(_) => LifecycleState::Closed,
            };
            snapshot.activity = match &inner.slot {
                Slot::Busy(active) => Activity::Running {
                    op: active.id,
                    kind: active.kind,
                    phase: active.phase,
                    cancel_requested: active.ct.is_cancelled(),
                },
                _ => Activity::Idle,
            };
            if let Some(availability) = availability {
                snapshot.workspace_status = availability;
            }
            snapshot.last = inner.last.clone();
        });
    }
}
