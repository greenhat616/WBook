use camino::Utf8PathBuf;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

mod worker;

pub use worker::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum SessionState {
    /// The session is pending and has not yet started.
    Pending,
    /// The session is currently extracting content.
    Extracting,
    /// The session is currently being parsed.
    Parsing,
    /// The session is currently being tweaked by the user.
    Tweak,
    /// The session is currently rendering.
    Rendering,
    /// The session has been completed.
    Completed(Option<Arc<SessionRunError>>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type, thiserror::Error)]
pub enum SessionError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type, thiserror::Error)]
#[error("Session run error occurred in state {prev_state:?}: {error}")]
pub struct SessionRunError {
    pub prev_state: SessionState,
    pub error: SessionError,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct SessionMetadata {
    /// The input file for the extractor and parser, represented as a UTF-8 encoded path.
    pub input_file: Utf8PathBuf,
}

#[derive(Debug, Clone)]
pub struct Timeline {
    pub timestamp: Timestamp,
    pub prev_state: SessionState,
    pub current_state: SessionState,
    pub reason: Option<String>,
}

#[derive(Debug)]
pub struct Session {
    pub id: SessionId,
    pub metadata: SessionMetadata,
    pub state: SessionState,
    pub timelines: Vec<Timeline>,
}
