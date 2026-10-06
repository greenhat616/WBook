use std::{io, path::PathBuf};

use axum::extract::rejection::{JsonRejection, PathRejection};
use snafu::Snafu;
use tokio::sync::oneshot;
use wbook_core::session::OperationId;

use crate::commands::dto::{CommandError, ErrorKind};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum BridgeError {
    #[snafu(display("Invalid RPC request: {source}"))]
    DecodeRequest { source: JsonRejection },
    #[snafu(display("Invalid {endpoint} path: {source}"))]
    DecodePath {
        endpoint: &'static str,
        source: PathRejection,
    },
    #[snafu(display("Invalid arguments for {method}: {source}"))]
    DecodeArguments {
        method: &'static str,
        source: serde_json::Error,
    },
    #[snafu(display("Could not encode {context}: {source}"))]
    EncodeResponse {
        context: &'static str,
        source: serde_json::Error,
    },
    #[snafu(display("Could not receive the result of operation {}: {source}", op.0))]
    ReceiveReceipt {
        op: OperationId,
        source: oneshot::error::RecvError,
    },
    #[snafu(display("Could not {action} preview resource {}: {source}", path.display()))]
    PreviewFile {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[snafu(display("Could not {action} window {label}: {source}"))]
    Window {
        action: &'static str,
        label: String,
        source: tauri::Error,
    },
}

impl From<BridgeError> for CommandError {
    fn from(error: BridgeError) -> Self {
        let kind = match &error {
            BridgeError::DecodeRequest { .. }
            | BridgeError::DecodePath { .. }
            | BridgeError::DecodeArguments { .. } => ErrorKind::InvalidParams,
            BridgeError::PreviewFile { source, .. } if source.kind() == io::ErrorKind::NotFound => {
                return Self::new(ErrorKind::NotFound, "Preview resource was not found");
            }
            BridgeError::EncodeResponse { .. }
            | BridgeError::ReceiveReceipt { .. }
            | BridgeError::PreviewFile { .. }
            | BridgeError::Window { .. } => ErrorKind::InternalError,
        };
        Self::new(kind, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, error::Error};

    use axum::{body::Body, extract::FromRequest, http::Request, Json};
    use serde_json::{json, Value};
    use snafu::ResultExt;

    use super::*;
    use crate::commands::dto::{decode_arguments, encode_response};

    #[test]
    fn argument_context_retains_the_json_source_and_the_wire_shape() {
        let error = serde_json::from_value::<u64>(json!("invalid"))
            .context(DecodeArgumentsSnafu {
                method: "read_text",
            })
            .unwrap_err();
        assert!(error
            .source()
            .unwrap()
            .downcast_ref::<serde_json::Error>()
            .is_some());
        assert!(error.to_string().contains("read_text"));

        let wire = decode_arguments::<u64>("read_text", json!("invalid")).unwrap_err();
        assert_eq!(wire.kind, ErrorKind::InvalidParams);
        assert!(wire.source().is_none());
        let value = serde_json::to_value(&wire).unwrap();
        assert_eq!(
            value,
            json!({ "kind": "invalid_params", "message": error.to_string() })
        );
    }

    #[test]
    fn response_encoding_preserves_its_operation_context() {
        let unsupported = BTreeMap::from([(vec![1_u8, 2], "value")]);
        let error = serde_json::to_value(&unsupported)
            .context(EncodeResponseSnafu {
                context: "read_results",
            })
            .unwrap_err();
        assert!(error
            .source()
            .unwrap()
            .downcast_ref::<serde_json::Error>()
            .is_some());
        let wire = encode_response("read_results", &unsupported).unwrap_err();
        assert_eq!(wire.kind, ErrorKind::InternalError);
        assert!(wire.message.contains("read_results"));
        assert_eq!(wire.message, error.to_string());
    }

    #[tokio::test]
    async fn extraction_context_keeps_the_axum_rejection() {
        let request = Request::post("/bridge/rpc")
            .header("content-type", "application/json")
            .body(Body::from("{"))
            .unwrap();
        let error = Json::<Value>::from_request(request, &())
            .await
            .context(DecodeRequestSnafu)
            .unwrap_err();
        assert!(error
            .source()
            .unwrap()
            .downcast_ref::<JsonRejection>()
            .is_some());
        assert!(error.to_string().contains("Invalid RPC request"));
        assert_eq!(CommandError::from(error).kind, ErrorKind::InvalidParams);
    }

    #[tokio::test]
    async fn dropped_receipts_keep_the_operation_and_channel_source() {
        let (sender, receiver) = oneshot::channel::<()>();
        drop(sender);
        let error = receiver
            .await
            .context(ReceiveReceiptSnafu {
                op: OperationId(17),
            })
            .unwrap_err();
        assert!(error
            .source()
            .unwrap()
            .downcast_ref::<oneshot::error::RecvError>()
            .is_some());
        assert!(error.to_string().contains("operation 17"));
        assert_eq!(CommandError::from(error).kind, ErrorKind::InternalError);
    }

    #[test]
    fn preview_io_sources_keep_the_path_and_map_only_missing_files_to_not_found() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing.xhtml");
        let error = std::fs::File::open(&path)
            .with_context(|_| PreviewFileSnafu {
                action: "open",
                path: path.clone(),
            })
            .unwrap_err();
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<io::Error>()
                .unwrap()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(error.to_string().contains("missing.xhtml"));
        assert_eq!(CommandError::from(error).kind, ErrorKind::NotFound);

        let error: BridgeError = Err::<(), _>(io::Error::from(io::ErrorKind::PermissionDenied))
            .context(PreviewFileSnafu {
                action: "open",
                path,
            })
            .unwrap_err();
        assert_eq!(CommandError::from(error).kind, ErrorKind::InternalError);
    }
}
