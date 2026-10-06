use std::{path::PathBuf, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tauri::{
    test::{mock_builder, mock_context, noop_assets, MockRuntime},
    Manager, WebviewWindow,
};
use tower::ServiceExt;
use wbook_core::{session::Rejected, types::Port, Params, Wbook};

use crate::commands::dto::{check_integers, CommandError, ErrorKind};
use crate::windows::SessionClosed;
use tauri_specta::Event;

struct Harness {
    _app: tauri::App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    core: Arc<Wbook>,
    router: Router,
    directory: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = camino::Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
        let core = Arc::new(Wbook::new(Params {
            data_dir: path.clone(),
            config_dir: path,
        }));
        let (_, specta) = crate::commands::specta_builder();
        let app = mock_builder()
            .manage(core.clone())
            .manage(Port(1421))
            .invoke_handler(specta.invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        specta.mount_events(&app);
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let router = server::local_router(
            crate::rpc::router(core.clone()),
            "127.0.0.1:1421".parse().unwrap(),
        );
        Self {
            _app: app,
            window,
            core,
            router,
            directory,
        }
    }

    async fn ipc(&self, method: &str, params: Value) -> Result<Value, Value> {
        let window = self.window.clone();
        let method = method.to_owned();
        tokio::task::spawn_blocking(move || {
            tauri::test::get_ipc_response(
                &window,
                tauri::webview::InvokeRequest {
                    cmd: method,
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "http://tauri.localhost".parse().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(params),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .map(|response| response.deserialize().unwrap())
        })
        .await
        .unwrap()
    }

    async fn raw(&self, body: String, host: &str, origin: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::post("/bridge/rpc")
            .header("host", host)
            .header("content-type", "application/json");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn rpc(&self, method: &str, params: Value) -> (StatusCode, Value) {
        self.raw(
            json!({ "method": method, "params": params }).to_string(),
            "127.0.0.1:1421",
            None,
        )
        .await
    }

    async fn rpc_ok(&self, method: &str, params: Value) -> Value {
        let (status, body) = self.rpc(method, params).await;
        assert_eq!(status, StatusCode::OK, "{method}: {body}");
        body
    }

    fn source(&self) -> PathBuf {
        self.directory.path().join("input.txt")
    }

    fn create_params(&self) -> Value {
        json!({ "source": self.source(), "options": { "filters": [], "toc": { "SplitEvenly": { "parts": 1 } } } })
    }
}

fn data(result: &Value) -> Value {
    assert_eq!(result["outcome"]["status"], "ok", "{result}");
    result["outcome"]["data"].clone()
}

#[tokio::test]
async fn ipc_and_http_share_the_complete_session_pipeline() {
    let h = Harness::new();
    let text = "第一章 开始\n正文 café\n第二段正文\n";
    std::fs::write(h.source(), text).unwrap();
    assert_eq!(h.ipc("get_port", json!({})).await.unwrap(), 1421);
    let created = h.ipc("create_session", h.create_params()).await.unwrap();
    let id = created["session"].clone();
    assert_eq!(h.rpc_ok("list_sessions", json!({})).await, json!([created]));
    let args = json!({ "sessionId": id });
    assert_eq!(
        h.ipc("get_session", args.clone()).await.unwrap(),
        h.rpc_ok("get_session", args.clone()).await
    );

    let initialized = h.rpc_ok("initialize_session", args.clone()).await;
    let revision = data(&initialized);
    assert_eq!(initialized["revision"], revision);
    assert_eq!(initialized["warnings"], json!([]));
    let results = data(&h.ipc("read_results", args.clone()).await.unwrap());
    let parsed = results["results"].clone();
    let version = parsed["version"].clone();
    let read = h.rpc_ok("read_text", json!({ "sessionId": id, "version": version, "range": { "start": 0, "end": text.len() } })).await;
    assert_eq!(data(&read), text);

    let edited = h
        .ipc(
            "apply_edits",
            json!({ "sessionId": id, "expected": revision, "batch": {
        "base": version, "edits": [{ "range": { "start": 0, "end": 0 }, "insert": "前言\n" }]
    } }),
        )
        .await
        .unwrap();
    let edited_revision = data(&edited);
    let stale_args = json!({ "sessionId": id, "expected": revision, "results": parsed });
    let stale = h.rpc_ok("install_results", stale_args.clone()).await;
    let ipc_stale = h.ipc("install_results", stale_args).await.unwrap();
    assert_eq!(stale["outcome"], ipc_stale["outcome"]);
    assert_eq!(stale["outcome"]["error"]["kind"], "stale_revision");
    assert_eq!(stale["revision"], edited_revision);
    assert!(ipc_stale["op"].as_u64().unwrap() > stale["op"].as_u64().unwrap());

    let parsed = data(
        &h.rpc_ok(
            "parse_session",
            json!({ "sessionId": id, "config": { "SplitEvenly": { "parts": 1 } } }),
        )
        .await,
    );
    let installed = h
        .ipc(
            "install_results",
            json!({ "sessionId": id, "expected": edited_revision, "results": parsed }),
        )
        .await
        .unwrap();
    let updated = h.rpc_ok("set_metadata_overrides", json!({ "sessionId": id, "expected": data(&installed), "overrides": { "title": "RPC Book", "author": "Test Author" } })).await;
    let revision = data(&updated);
    let options = json!({ "render": { "layout": "SingleHtml", "templates": {} }, "format": "Epub", "language": "zh-CN", "identifier": "urn:wbook:rpc-test" });
    let preview = data(
        &h.ipc(
            "render_preview",
            json!({ "sessionId": id, "expected": revision, "options": options }),
        )
        .await
        .unwrap(),
    );
    let preview_dir = PathBuf::from(preview["directory"].as_str().unwrap());
    assert!(preview_dir.is_dir());
    let destination = h.directory.path().join("book.epub");
    let exported = h.rpc_ok("export_epub", json!({ "sessionId": id, "expected": revision, "options": options, "destination": destination })).await;
    assert_eq!(data(&exported)["revision"], revision);
    assert!(destination.is_file());

    let cancel = json!({ "sessionId": id, "operationId": exported["op"] });
    assert_eq!(
        h.ipc("cancel_operation", cancel.clone()).await.unwrap(),
        "NotActive"
    );
    assert_eq!(h.rpc_ok("cancel_operation", cancel).await, "NotActive");
    let closed = h.rpc_ok("close_session", args.clone()).await;
    assert_eq!(closed["lost"], false);
    assert!(!preview_dir.exists());
    assert!(destination.exists() && h.source().exists());
    let (status, error) = h.rpc("get_session", args.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(h.ipc("get_session", args).await.unwrap_err(), error);
    assert_eq!(error["kind"], "not_found");
    assert!(h.core.shutdown().await.is_empty());
}

#[tokio::test]
async fn rejected_requests_and_failed_operations_preserve_their_contract() {
    let h = Harness::new();
    let mut invalid = h.create_params();
    invalid["options"]["toc"]["SplitEvenly"]["parts"] = json!(0);
    let (status, error) = h.rpc("create_session", invalid.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["kind"], "invalid_config");
    assert_eq!(h.ipc("create_session", invalid).await.unwrap_err(), error);
    assert_eq!(h.rpc_ok("list_sessions", json!({})).await, json!([]));

    for (method, params, status, kind) in [
        (
            "get_port",
            json!({}),
            StatusCode::BAD_REQUEST,
            "platform_unsupported",
        ),
        (
            "open_session_window",
            json!({ "sessionId": 1 }),
            StatusCode::BAD_REQUEST,
            "platform_unsupported",
        ),
        (
            "window_ready",
            json!({}),
            StatusCode::BAD_REQUEST,
            "platform_unsupported",
        ),
        (
            "missing",
            json!({}),
            StatusCode::NOT_FOUND,
            "method_not_found",
        ),
        (
            "get_session",
            json!({ "session_id": 1 }),
            StatusCode::BAD_REQUEST,
            "invalid_params",
        ),
        (
            "get_session",
            json!({ "sessionId": "1" }),
            StatusCode::BAD_REQUEST,
            "invalid_params",
        ),
        (
            "get_session",
            json!({ "sessionId": 1_u64 << 53 }),
            StatusCode::BAD_REQUEST,
            "invalid_params",
        ),
        (
            "list_sessions",
            Value::Null,
            StatusCode::BAD_REQUEST,
            "invalid_params",
        ),
    ] {
        let (actual, body) = h.rpc(method, params).await;
        assert_eq!(actual, status, "{body}");
        assert_eq!(body["kind"], kind);
    }
    let (status, body) = h.raw("{".into(), "127.0.0.1:1421", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["kind"], "invalid_params");
    assert_eq!(
        h.raw(
            json!({ "method": "list_sessions" }).to_string(),
            "127.0.0.1:1421",
            None
        )
        .await
        .1,
        json!([])
    );
    assert_eq!(
        h.ipc("get_session", json!({ "sessionId": 1_u64 << 53 }))
            .await
            .unwrap_err()["kind"],
        "invalid_params"
    );

    let session = h.rpc_ok("create_session", h.create_params()).await;
    let args = json!({ "sessionId": session["session"] });
    let failed = h.ipc("initialize_session", args.clone()).await.unwrap();
    assert_eq!(failed["outcome"]["error"]["kind"], "extractor");
    assert_eq!(failed["revision"], 0);
    assert_eq!(failed["op"], 1);
    assert_eq!(failed["warnings"], json!([]));
    let http_failed = h.rpc_ok("initialize_session", args.clone()).await;
    assert_eq!(http_failed["outcome"], failed["outcome"]);
    h.ipc("close_session", args).await.unwrap();
    h.core.shutdown().await;
    assert_eq!(
        h.rpc("create_session", h.create_params()).await.1["kind"],
        "shutting_down"
    );
}

#[tokio::test]
async fn host_and_full_origin_are_checked_before_dispatch() {
    let h = Harness::new();
    let body = json!({ "method": "create_session", "params": h.create_params() }).to_string();
    for (host, origin) in [
        ("attacker.example:1421", None),
        ("127.0.0.1:1422", None),
        ("127.0.0.1:1421", Some("http://localhost:1422")),
        ("127.0.0.1:1421", Some("https://localhost:1420")),
        ("127.0.0.1:1421", Some("null")),
        ("127.0.0.1:1421", Some("https://attacker.example")),
    ] {
        let (status, error) = h.raw(body.clone(), host, origin).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error["kind"], "forbidden");
    }
    assert!(h.core.session_manager().list().is_empty());
    let mut origins = vec!["http://127.0.0.1:1421", "https://tauri.localhost"];
    if cfg!(debug_assertions) {
        origins.push("http://localhost:1420");
    }
    for origin in origins {
        let response = h
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/bridge/rpc")
                    .header("host", "127.0.0.1:1421")
                    .header("origin", origin)
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "content-type")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(response.headers()["access-control-allow-origin"], origin);
    }
    h.core.shutdown().await;
}

#[test]
fn integer_limits_and_admission_error_categories_are_explicit() {
    assert!(check_integers(
        &json!({ "values": [(1_u64 << 53) - 1, 0] }),
        ErrorKind::InvalidParams
    )
    .is_ok());
    for value in [
        json!(1_u64 << 53),
        json!({ "nested": [u64::MAX] }),
        json!(-9_007_199_254_740_992_i64),
    ] {
        assert_eq!(
            check_integers(&value, ErrorKind::InvalidParams)
                .unwrap_err()
                .kind,
            ErrorKind::InvalidParams
        );
        assert_eq!(
            check_integers(&value, ErrorKind::InternalError)
                .unwrap_err()
                .kind,
            ErrorKind::InternalError
        );
    }
    for (rejected, kind) in [
        (Rejected::Busy, ErrorKind::Busy),
        (Rejected::Closing, ErrorKind::Closing),
        (Rejected::Closed, ErrorKind::Closed),
        (Rejected::Unavailable, ErrorKind::Unavailable),
    ] {
        assert_eq!(CommandError::from(rejected).kind, kind);
    }
}

#[tokio::test]
async fn runtime_publishes_the_bound_port_and_shuts_down_sessions_and_http() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let directory = tempfile::tempdir().unwrap();
    let path = camino::Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
    let runtime = crate::runtime::AppRuntime::start(
        Params {
            data_dir: path.clone(),
            config_dir: path.clone(),
        },
        0,
    )
    .await
    .unwrap();
    assert_ne!(runtime.port, 0);
    let session = runtime
        .core
        .session_manager()
        .create(
            path.join("missing.txt"),
            wbook_core::workspace::ProcessingOptions {
                filters: vec![],
                toc: wbook_core::parser::toc::TocParserConfig::SplitEvenly { parts: 1 },
            },
        )
        .unwrap();
    let mut stream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, runtime.port))
        .await
        .unwrap();
    let body = r#"{"method":"list_sessions"}"#;
    stream.write_all(format!("POST /bridge/rpc HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", runtime.port, body.len()).as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("missing.txt"));
    tokio::time::timeout(std::time::Duration::from_secs(10), runtime.shutdown())
        .await
        .unwrap();
    assert_eq!(
        session.snapshot().lifecycle,
        wbook_core::session::LifecycleState::Closed
    );
    assert!(runtime.finished.load(std::sync::atomic::Ordering::Acquire));
    assert!(
        tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, runtime.port))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn session_windows_open_once_and_only_for_open_sessions() {
    let h = Harness::new();
    std::fs::write(h.source(), "Text\n").unwrap();
    let created = h.ipc("create_session", h.create_params()).await.unwrap();
    let args = json!({ "sessionId": created["session"] });
    for _ in 0..2 {
        assert_eq!(
            h.ipc("open_session_window", args.clone()).await.unwrap(),
            Value::Null
        );
    }
    // Reports are idempotent, e.g. after a reload of an already shown window.
    for _ in 0..2 {
        assert_eq!(h.ipc("window_ready", json!({})).await.unwrap(), Value::Null);
    }
    let windows = h._app.webview_windows();
    assert_eq!(windows.len(), 2);
    let url = windows["session-1"].url().unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.fragment(), Some("/sessions/1"));

    let (closed, announced) = tokio::sync::oneshot::channel();
    let closed = std::sync::Mutex::new(Some(closed));
    SessionClosed::listen(&h.window, move |event| {
        if let Some(closed) = closed.lock().unwrap().take() {
            let _ = closed.send(event.payload.session);
        }
    });
    h.rpc_ok("close_session", args.clone()).await;
    let payload = tokio::time::timeout(std::time::Duration::from_secs(5), announced)
        .await
        .expect("the main window hears about the closed session")
        .unwrap();
    assert_eq!(json!(payload.0), created["session"]);
    for args in [args, json!({ "sessionId": 99 })] {
        assert_eq!(
            h.ipc("open_session_window", args).await.unwrap_err()["kind"],
            "not_found"
        );
    }
    assert_eq!(h._app.webview_windows().len(), 2);
    h.core.shutdown().await;
}
