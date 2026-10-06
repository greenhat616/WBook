use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use tokio::runtime::Handle;

use crate::parser::toc::TocConfigError;
use crate::session::{CloseReport, SessionHandle, SessionId, SessionSnapshot};
use crate::workspace::{ProcessingOptions, Workspace, WorkspaceError};

pub struct SessionManager(Arc<ManagerShared>);

pub(crate) struct ManagerShared {
    runtime: Handle,
    registry: Mutex<Registry>,
}

struct Registry {
    accepting: bool,
    next_id: u64,
    sessions: HashMap<SessionId, SessionHandle>,
}

#[derive(Debug, snafu::Snafu)]
pub enum ManagerError {
    #[snafu(display("session not found"))]
    NotFound,
    #[snafu(display("session manager is shutting down"))]
    ShuttingDown,
    #[snafu(context(false), display("{source}"))]
    InvalidConfig { source: TocConfigError },
}

impl SessionManager {
    pub fn new(runtime: Handle) -> Self {
        Self(Arc::new(ManagerShared {
            runtime,
            registry: Mutex::new(Registry {
                accepting: true,
                next_id: 1,
                sessions: HashMap::new(),
            }),
        }))
    }

    pub fn open(&self, workspace: Workspace) -> Result<SessionHandle, ManagerError> {
        let workspace_id = workspace.id();
        let source = workspace.source().to_owned();
        let status = workspace.status();
        let mut registry = self.0.registry.lock().unwrap();
        if !registry.accepting {
            return Err(ManagerError::ShuttingDown);
        }
        let id = SessionId(registry.next_id);
        registry.next_id += 1;
        let handle = SessionHandle::new(
            id,
            workspace,
            workspace_id,
            source,
            status,
            self.0.runtime.clone(),
            Arc::downgrade(&self.0),
        );
        registry.sessions.insert(id, handle.clone());
        Ok(handle)
    }

    pub fn create(
        &self,
        source: Utf8PathBuf,
        options: ProcessingOptions,
    ) -> Result<SessionHandle, ManagerError> {
        let workspace = Workspace::new(source, options).map_err(|error| match error {
            WorkspaceError::InvalidConfig { source: error } => {
                ManagerError::InvalidConfig { source: error }
            }
            _ => unreachable!("workspace creation only validates configuration"),
        })?;
        self.open(workspace)
    }

    pub fn get(&self, id: SessionId) -> Result<SessionHandle, ManagerError> {
        self.0
            .registry
            .lock()
            .unwrap()
            .sessions
            .get(&id)
            .cloned()
            .ok_or(ManagerError::NotFound)
    }

    pub fn list(&self) -> Vec<SessionSnapshot> {
        let handles: Vec<_> = self
            .0
            .registry
            .lock()
            .unwrap()
            .sessions
            .values()
            .cloned()
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.snapshot())
            .collect()
    }

    pub async fn close(&self, id: SessionId) -> Result<Arc<CloseReport>, ManagerError> {
        Ok(self.get(id)?.close().await)
    }

    pub async fn shutdown(&self) -> Vec<(SessionId, Arc<CloseReport>)> {
        let handles = self.0.stop();
        let mut reports = Vec::with_capacity(handles.len());
        for handle in handles {
            reports.push((handle.id(), handle.close().await));
        }
        reports
    }
}

impl ManagerShared {
    pub(crate) fn remove(&self, id: SessionId) {
        self.registry.lock().unwrap().sessions.remove(&id);
    }

    fn stop(&self) -> Vec<SessionHandle> {
        let handles: Vec<_> = {
            let mut registry = self.registry.lock().unwrap();
            registry.accepting = false;
            registry.sessions.values().cloned().collect()
        };
        // Request every close before awaiting any worker, without nesting the
        // registry and session locks in the reverse order of deregistration.
        for handle in &handles {
            handle.request_close();
        }
        handles
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        self.0.stop();
    }
}
