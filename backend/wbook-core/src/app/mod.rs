use std::sync::Arc;

use camino::Utf8PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::session::{CloseReport, SessionId};

pub mod session_manager;

pub use session_manager::{ManagerError, SessionManager};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct Params {
    pub data_dir: Utf8PathBuf,
    pub config_dir: Utf8PathBuf,
}

pub struct Wbook {
    pub start_params: Params,
    session_manager: SessionManager,
}

impl Wbook {
    pub fn new(start_params: Params) -> Self {
        Self {
            start_params,
            session_manager: SessionManager::new(tokio::runtime::Handle::current()),
        }
    }

    pub fn session_manager(&self) -> &SessionManager {
        &self.session_manager
    }

    pub async fn shutdown(&self) -> Vec<(SessionId, Arc<CloseReport>)> {
        self.session_manager.shutdown().await
    }
}
