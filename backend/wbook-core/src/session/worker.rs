use super::*;

impl SessionHandle {
    pub(crate) fn run<T: Send + 'static>(
        &self,
        kind: OpKind,
        op: impl FnOnce(&mut Workspace, &OpContext<'_>) -> Result<T, WorkspaceError> + Send + 'static,
    ) -> Result<Receipt<T>, Rejected> {
        let (mut workspace, id, ct) = {
            let mut inner = self.0.inner.lock().unwrap();
            match inner.lifecycle {
                Lifecycle::Closing => return Err(Rejected::Closing),
                Lifecycle::Closed(_) => return Err(Rejected::Closed),
                Lifecycle::Open => {}
            }
            match inner.slot {
                Slot::Busy(_) => return Err(Rejected::Busy),
                Slot::Lost => return Err(Rejected::Unavailable),
                Slot::Idle(_) => {}
            }
            let Slot::Idle(workspace) = std::mem::replace(&mut inner.slot, Slot::Lost) else {
                unreachable!()
            };
            let id = OperationId(inner.next_op);
            inner.next_op += 1;
            let ct = CancellationToken::new();
            inner.slot = Slot::Busy(ActiveOp {
                id,
                kind,
                phase: None,
                ct: ct.clone(),
            });
            self.0.publish(&inner, None);
            (workspace, id, ct)
        };
        let (tx, rx) = oneshot::channel();
        let shared = self.0.clone();
        let weak = Arc::downgrade(&shared);
        tracing::debug!(
            session = shared.id.0,
            operation = id.0,
            ?kind,
            "session operation accepted"
        );
        self.0.runtime.spawn(async move {
            let report = progress(weak, id);
            let joined = shared
                .runtime
                .spawn_blocking(move || {
                    let cx = OpContext {
                        ct: &ct,
                        report: &report,
                    };
                    let outcome = op(&mut workspace, &cx);
                    (workspace, outcome)
                })
                .await;
            // Domain work stays outside the bookkeeping lock, including the
            // status calculation and warning drain after a business failure.
            let completed = joined.map(|(mut workspace, outcome)| {
                let status = workspace.status();
                let warnings = workspace.take_warnings();
                (workspace, status, warnings, outcome)
            });
            let mut inner = shared.inner.lock().unwrap();
            let (revision, outcome, warnings, availability) = match completed {
                Ok((workspace, status, warnings, outcome)) => {
                    let revision = status.revision;
                    inner.slot = Slot::Idle(workspace);
                    (
                        revision,
                        outcome.map_err(OpError::from),
                        warnings,
                        Availability::Available(status),
                    )
                }
                Err(error) => {
                    tracing::error!(
                        session = shared.id.0,
                        operation = id.0,
                        %error,
                        "session workspace lost"
                    );
                    let revision = shared.snapshot.borrow().workspace_status.revision();
                    inner.slot = Slot::Lost;
                    (
                        revision,
                        Err(OpError::Panicked),
                        Vec::new(),
                        Availability::Lost {
                            last_revision: revision,
                        },
                    )
                }
            };
            let category = match &outcome {
                Ok(_) => ResultCategory::Succeeded,
                Err(OpError::Workspace { source: error }) if error.is_cancelled() => {
                    ResultCategory::Cancelled
                }
                Err(OpError::Workspace { source: _ }) => ResultCategory::Failed,
                Err(OpError::Panicked) => ResultCategory::Panicked,
            };
            inner.last = Some(OperationSummary {
                op: id,
                kind,
                revision,
                category,
            });
            shared.publish(&inner, Some(availability));
            // A dropped receipt can run the destructor of caller-owned T.
            // Delivery must not let that destructor reenter a locked session.
            drop(inner);
            tracing::debug!(
                session = shared.id.0,
                operation = id.0,
                ?category,
                revision = revision.0,
                "session operation completed"
            );
            let _ = tx.send(OperationResult {
                op: id,
                kind,
                revision,
                outcome,
                warnings,
            });
        });
        Ok(Receipt { op: id, rx })
    }

    pub(crate) fn request_close(&self) {
        let mut inner = self.0.inner.lock().unwrap();
        if !matches!(inner.lifecycle, Lifecycle::Open) {
            return;
        }
        inner.lifecycle = Lifecycle::Closing;
        if let Slot::Busy(active) = &inner.slot {
            active.ct.cancel();
        }
        self.0.publish(&inner, None);
        tracing::debug!(session = self.id().0, "session closing");
        let shared = self.0.clone();
        self.0.runtime.spawn(async move {
            close_workspace(shared).await;
        });
    }
}

pub(super) fn progress(
    shared: Weak<Shared>,
    op: OperationId,
) -> impl Fn(Phase) + Send + Sync + 'static {
    move |phase| {
        if let Some(shared) = shared.upgrade() {
            let mut inner = shared.inner.lock().unwrap();
            if let Slot::Busy(active) = &mut inner.slot {
                if active.id == op {
                    active.phase = Some(phase);
                    shared.publish(&inner, None);
                }
            }
        }
    }
}

async fn close_workspace(shared: Arc<Shared>) {
    let mut snapshot = shared.snapshot.subscribe();
    snapshot
        .wait_for(|snapshot| snapshot.activity == Activity::Idle)
        .await
        .expect("session owns its snapshot sender");
    let (workspace, last) = {
        let mut inner = shared.inner.lock().unwrap();
        let workspace = match std::mem::replace(&mut inner.slot, Slot::Lost) {
            Slot::Idle(workspace) => Some(workspace),
            Slot::Lost => None,
            Slot::Busy(_) => unreachable!("idle snapshot must have a completed worker"),
        };
        (workspace, inner.last.clone())
    };
    let (cleanup_failures, lost) = match workspace {
        Some(workspace) => match shared
            .runtime
            .spawn_blocking(move || workspace.close())
            .await
        {
            Ok(warnings) => (warnings, false),
            Err(error) => {
                tracing::error!(session = shared.id.0, %error, "workspace cleanup task failed");
                (Vec::new(), true)
            }
        },
        None => (Vec::new(), true),
    };
    let report = Arc::new(CloseReport {
        last,
        lost,
        cleanup_failures,
    });
    let mut inner = shared.inner.lock().unwrap();
    inner.lifecycle = Lifecycle::Closed(report);
    shared.publish(&inner, None);
    // Keep close waiters behind this lock until deregistration finishes. Manager
    // methods release the registry lock before acquiring a session lock.
    if let Some(registry) = shared.registry.upgrade() {
        registry.remove(shared.id);
    }
    tracing::debug!(session = shared.id.0, lost, "session closed");
}
