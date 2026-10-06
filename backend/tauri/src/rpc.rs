use std::sync::Arc;

use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use snafu::ResultExt;
use wbook_core::Wbook;

use crate::commands::{
    self,
    dto::{CommandError, ErrorKind},
};
use crate::errors::DecodeRequestSnafu;

#[derive(Deserialize)]
struct Request {
    method: String,
    #[serde(default = "empty_params")]
    params: Value,
}

fn empty_params() -> Value {
    json!({})
}

pub fn router(app: Arc<Wbook>) -> Router {
    Router::new()
        .route("/bridge/rpc", post(invoke))
        .with_state(app.clone())
        .merge(crate::subscriptions::router(app.clone()))
        .merge(crate::preview::router(app))
}

async fn invoke(
    State(app): State<Arc<Wbook>>,
    request: Result<Json<Request>, JsonRejection>,
) -> Result<Json<Value>, CommandError> {
    let Json(request) = request.context(DecodeRequestSnafu)?;
    if !request.params.is_object() {
        return Err(CommandError::new(
            ErrorKind::InvalidParams,
            "params must be an object",
        ));
    }
    commands::dispatch(&app, &request.method, request.params)
        .await
        .map(Json)
}

impl IntoResponse for CommandError {
    fn into_response(self) -> Response {
        let status = match self.kind {
            ErrorKind::MethodNotFound | ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::Busy
            | ErrorKind::Closing
            | ErrorKind::Closed
            | ErrorKind::StaleRevision
            | ErrorKind::AlreadyInitialized
            | ErrorKind::ResultsNotCurrent => StatusCode::CONFLICT,
            ErrorKind::ShuttingDown | ErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorKind::InternalError
            | ErrorKind::Panicked
            | ErrorKind::Extractor
            | ErrorKind::Document
            | ErrorKind::Pipeline
            | ErrorKind::Export => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        (status, Json(self)).into_response()
    }
}
