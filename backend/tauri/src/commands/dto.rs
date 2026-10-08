use std::{fmt, path::PathBuf};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use snafu::ResultExt;
use specta::Type;
use wbook_core::{
    app::ManagerError,
    document::DocumentVersion,
    export::{self, OutputFormat},
    session::{self, OpError, OpKind, OperationId, OperationSummary, Receipt, Rejected},
    settings::SaveError,
    workspace::{self, Revision, WorkspaceError},
};

use crate::errors::{DecodeArgumentsSnafu, EncodeResponseSnafu, ReceiveReceiptSnafu};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidParams,
    MethodNotFound,
    PlatformUnsupported,
    NotFound,
    ShuttingDown,
    Busy,
    Unavailable,
    Closing,
    Closed,
    InvalidConfig,
    StaleRevision,
    NoDocument,
    AlreadyInitialized,
    ResultsNotCurrent,
    ReadTooLarge,
    Cancelled,
    Panicked,
    Extractor,
    Document,
    Pipeline,
    Export,
    InternalError,
}

#[derive(Debug, Serialize, Type)]
pub struct CommandError {
    pub kind: ErrorKind,
    pub message: String,
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

impl CommandError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<ManagerError> for CommandError {
    fn from(error: ManagerError) -> Self {
        let kind = match error {
            ManagerError::NotFound => ErrorKind::NotFound,
            ManagerError::ShuttingDown => ErrorKind::ShuttingDown,
            ManagerError::InvalidConfig { .. } => ErrorKind::InvalidConfig,
        };
        Self::new(kind, error.to_string())
    }
}

impl From<SaveError> for CommandError {
    fn from(error: SaveError) -> Self {
        let kind = match error {
            SaveError::Invalid { .. } => ErrorKind::InvalidConfig,
            SaveError::Stale { .. } => ErrorKind::StaleRevision,
            SaveError::Encode { .. } | SaveError::Write { .. } => ErrorKind::InternalError,
        };
        Self::new(kind, error.to_string())
    }
}

impl From<Rejected> for CommandError {
    fn from(error: Rejected) -> Self {
        let kind = match error {
            Rejected::Busy => ErrorKind::Busy,
            Rejected::Unavailable => ErrorKind::Unavailable,
            Rejected::Closing => ErrorKind::Closing,
            Rejected::Closed => ErrorKind::Closed,
        };
        Self::new(kind, error.to_string())
    }
}

impl From<OpError> for CommandError {
    fn from(error: OpError) -> Self {
        let kind = match &error {
            OpError::Panicked => ErrorKind::Panicked,
            OpError::Workspace { source } if source.is_cancelled() => ErrorKind::Cancelled,
            OpError::Workspace { source } => match source {
                WorkspaceError::InvalidConfig { .. } => ErrorKind::InvalidConfig,
                WorkspaceError::InvalidMetadata { .. } | WorkspaceError::Cover { .. } => {
                    ErrorKind::InvalidParams
                }
                WorkspaceError::StaleRevision { .. } => ErrorKind::StaleRevision,
                WorkspaceError::NoDocument => ErrorKind::NoDocument,
                WorkspaceError::AlreadyInitialized => ErrorKind::AlreadyInitialized,
                WorkspaceError::ResultsNotCurrent => ErrorKind::ResultsNotCurrent,
                WorkspaceError::ReadTooLarge { .. } => ErrorKind::ReadTooLarge,
                WorkspaceError::Extractor { .. } => ErrorKind::Extractor,
                WorkspaceError::Document { .. } => ErrorKind::Document,
                WorkspaceError::Pipeline { .. } => ErrorKind::Pipeline,
                WorkspaceError::Export { .. } => ErrorKind::Export,
            },
        };
        Self::new(kind, error.to_string())
    }
}

#[derive(Debug, Serialize, Type)]
pub struct CleanupWarning {
    pub path: PathBuf,
    pub message: String,
}

impl From<export::CleanupFailure> for CleanupWarning {
    fn from(value: export::CleanupFailure) -> Self {
        Self {
            path: value.path,
            message: value.message,
        }
    }
}

#[derive(Debug, Serialize, Type)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Outcome<T> {
    Ok { data: T },
    Error { error: CommandError },
}

#[derive(Debug, Serialize, Type)]
pub struct OperationResponse<T> {
    pub op: OperationId,
    pub kind: OpKind,
    pub revision: Revision,
    pub outcome: Outcome<T>,
    pub warnings: Vec<CleanupWarning>,
}

pub async fn complete<T, U: From<T>>(
    receipt: Result<Receipt<T>, Rejected>,
) -> Result<OperationResponse<U>, CommandError> {
    let receipt = receipt?;
    let op = receipt.op;
    let result = receipt.await.context(ReceiveReceiptSnafu { op })?;
    Ok(operation_response(result))
}

fn operation_response<T, U: From<T>>(result: session::OperationResult<T>) -> OperationResponse<U> {
    let mut warnings: Vec<CleanupWarning> = result.warnings.into_iter().map(Into::into).collect();
    // Export failures carry their own cleanup diagnostics outside the workspace warning queue.
    if let Err(OpError::Workspace {
        source: WorkspaceError::Export { source: error },
    }) = &result.outcome
    {
        warnings.extend(error.cleanup_failures.iter().map(|warning| CleanupWarning {
            path: warning.path.clone(),
            message: warning.message.clone(),
        }));
    }
    OperationResponse {
        op: result.op,
        kind: result.kind,
        revision: result.revision,
        outcome: match result.outcome {
            Ok(data) => Outcome::Ok { data: data.into() },
            Err(error) => Outcome::Error {
                error: error.into(),
            },
        },
        warnings,
    }
}

/// The cover as base64 JPEG, or `None` for a book without a cover.
#[derive(Debug, Serialize, Type)]
pub struct RenderedCover {
    pub jpeg: Option<String>,
}

impl From<Option<Vec<u8>>> for RenderedCover {
    fn from(value: Option<Vec<u8>>) -> Self {
        Self {
            jpeg: value.map(|bytes| BASE64.encode(bytes)),
        }
    }
}

pub fn decode_base64(data: &str) -> Result<Vec<u8>, CommandError> {
    BASE64.decode(data).map_err(|error| {
        CommandError::new(ErrorKind::InvalidParams, format!("invalid base64: {error}"))
    })
}

#[derive(Debug, Serialize, Type)]
pub struct ExportedBook {
    pub revision: Revision,
    pub format: OutputFormat,
    pub path: PathBuf,
    pub version: DocumentVersion,
    pub identifier: String,
    pub cleanup_failures: Vec<CleanupWarning>,
}

impl From<workspace::ExportArtifact> for ExportedBook {
    fn from(value: workspace::ExportArtifact) -> Self {
        Self {
            revision: value.revision,
            format: value.artifact.format,
            path: value.artifact.path,
            version: value.artifact.version,
            identifier: value.artifact.identifier,
            cleanup_failures: value
                .artifact
                .cleanup_failures
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, Type)]
pub struct ClosedSession {
    pub last: Option<OperationSummary>,
    pub lost: bool,
    pub cleanup_failures: Vec<CleanupWarning>,
}

impl From<&session::CloseReport> for ClosedSession {
    fn from(value: &session::CloseReport) -> Self {
        Self {
            last: value.last.clone(),
            lost: value.lost,
            cleanup_failures: value
                .cleanup_failures
                .iter()
                .map(|warning| CleanupWarning {
                    path: warning.path.clone(),
                    message: warning.message.clone(),
                })
                .collect(),
        }
    }
}

pub fn decode_arguments<T: DeserializeOwned>(
    method: &'static str,
    params: Value,
) -> Result<T, CommandError> {
    Ok(serde_json::from_value(params).context(DecodeArgumentsSnafu { method })?)
}

pub fn encode_response(
    context: &'static str,
    value: &impl Serialize,
) -> Result<Value, CommandError> {
    Ok(serde_json::to_value(value).context(EncodeResponseSnafu { context })?)
}

pub fn check_integers(value: &impl Serialize, kind: ErrorKind) -> Result<(), CommandError> {
    fn safe(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Number(number) => {
                const MAX: u64 = (1 << 53) - 1;
                number.as_u64().is_some_and(|n| n <= MAX)
                    || number.as_i64().is_some_and(|n| n.unsigned_abs() <= MAX)
            }
            serde_json::Value::Array(values) => values.iter().all(safe),
            serde_json::Value::Object(values) => values.values().all(safe),
            _ => true,
        }
    }
    let value = encode_response("JavaScript integer validation", value)?;
    if safe(&value) {
        Ok(())
    } else {
        Err(CommandError::new(
            kind,
            "Numeric values must be JavaScript safe integers",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_exports_preserve_receipt_metadata_and_both_warning_sources() {
        let result = session::OperationResult::<()> {
            op: OperationId(7),
            kind: OpKind::ExportEpub,
            revision: Revision(4),
            outcome: Err(OpError::Workspace {
                source: WorkspaceError::Export {
                    source: export::ExportError {
                        stage: export::ExportStage::Rendering,
                        source: export::ExportFailure::Cancelled,
                        cleanup_failures: vec![export::CleanupFailure {
                            path: "render-temp".into(),
                            message: "render cleanup failed".into(),
                        }],
                    },
                },
            }),
            warnings: vec![export::CleanupFailure {
                path: "preview-temp".into(),
                message: "preview cleanup failed".into(),
            }],
        };
        let response: OperationResponse<()> = operation_response(result);
        assert_eq!(response.op, OperationId(7));
        assert_eq!(response.revision, Revision(4));
        assert_eq!(response.warnings.len(), 2);
        assert_eq!(response.warnings[0].path, PathBuf::from("preview-temp"));
        assert_eq!(response.warnings[1].path, PathBuf::from("render-temp"));
        match response.outcome {
            Outcome::Error { error } => assert_eq!(error.kind, ErrorKind::Cancelled),
            Outcome::Ok { .. } => panic!("cancelled export must remain an error"),
        }
        assert_eq!(
            CommandError::from(OpError::Panicked).kind,
            ErrorKind::Panicked
        );
    }
}
