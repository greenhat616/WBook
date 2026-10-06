use std::{convert::Infallible, sync::Arc};

use axum::{
    extract::{rejection::PathRejection, Path, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    Router,
};
use futures_util::{stream, Stream};
use snafu::ResultExt;
use tokio::sync::watch;
use wbook_core::{
    session::{LifecycleState, SessionId, SessionSnapshot},
    Wbook,
};

use crate::commands::dto::{check_integers, CommandError, ErrorKind};
use crate::errors::{DecodePathSnafu, EncodeResponseSnafu};

pub fn router(core: Arc<Wbook>) -> Router {
    Router::new()
        .route("/bridge/sessions/{session_id}/events", get(subscribe))
        .with_state(core)
}

async fn subscribe(
    State(core): State<Arc<Wbook>>,
    session_id: Result<Path<u64>, PathRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, CommandError> {
    let Path(session_id) = session_id.context(DecodePathSnafu {
        endpoint: "session subscription",
    })?;
    check_integers(&session_id, ErrorKind::InvalidParams)?;
    let receiver = core
        .session_manager()
        .get(SessionId(session_id))?
        .subscribe();
    Ok(Sse::new(snapshots(receiver)).keep_alive(KeepAlive::default()))
}

fn snapshots(
    receiver: watch::Receiver<SessionSnapshot>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(Some((receiver, true)), |state| async move {
        let (mut receiver, initial) = state?;
        if !initial && receiver.changed().await.is_err() {
            return None;
        }
        // Do not hold the watch read lock while a slow client consumes the body.
        let snapshot = receiver.borrow_and_update().clone();
        let next = (snapshot.lifecycle != LifecycleState::Closed).then_some((receiver, false));
        let event = check_integers(&snapshot, ErrorKind::InternalError).and_then(|()| {
            let data = serde_json::to_string(&snapshot).context(EncodeResponseSnafu {
                context: "session snapshot event",
            })?;
            Ok(Event::default()
                .event("session")
                .id(snapshot.seq.to_string())
                .data(data))
        });
        match event {
            Ok(event) => Some((Ok(event), next)),
            Err(error) => Some((
                Ok(Event::default()
                    .event("bridge-error")
                    .json_data(error)
                    .expect("command errors contain only strings")),
                None,
            )),
        }
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::{
        body::{to_bytes, Body, BodyDataStream},
        http::{Request, StatusCode},
        response::IntoResponse,
    };
    use futures_util::StreamExt;
    use serde_json::Value;
    use tokio::time::timeout;
    use tower::ServiceExt;
    use wbook_core::{
        session::{Activity, SessionHandle},
        settings::Settings,
        Params,
    };

    use super::*;

    struct Harness {
        core: Arc<Wbook>,
        session: SessionHandle,
        router: Router,
        _directory: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let path = camino::Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
            let core = Arc::new(Wbook::new(Params {
                data_dir: path.clone(),
                config_dir: path.clone(),
            }));
            let source = path.join("input.txt");
            std::fs::write(&source, "Chapter one\nText to preview.\n").unwrap();
            let session = core
                .session_manager()
                .create(source, Settings::default())
                .unwrap();
            let router = server::local_router(
                super::router(core.clone()),
                "127.0.0.1:1421".parse().unwrap(),
            );
            Self {
                core,
                session,
                router,
                _directory: directory,
            }
        }

        async fn subscribe(&self) -> BodyDataStream {
            let response = self
                .router
                .clone()
                .oneshot(
                    Request::get(format!("/bridge/sessions/{}/events", self.session.id().0))
                        .header("host", "127.0.0.1:1421")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["content-type"], "text/event-stream");
            response.into_body().into_data_stream()
        }
    }

    async fn next_snapshot(body: &mut BodyDataStream) -> SessionSnapshot {
        let bytes = timeout(Duration::from_secs(5), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let event = std::str::from_utf8(&bytes).unwrap();
        assert!(event.contains("event: session\n"), "{event}");
        let data = event
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap();
        let snapshot: SessionSnapshot = serde_json::from_str(data).unwrap();
        assert!(event.contains(&format!("id: {}\n", snapshot.seq)));
        snapshot
    }

    #[tokio::test]
    async fn subscription_sends_latest_snapshots_and_ends_after_closed() {
        let h = Harness::new();
        let mut body = h.subscribe().await;
        let initial = next_snapshot(&mut body).await;
        assert_eq!(initial, h.session.snapshot());
        h.session
            .initialize()
            .unwrap()
            .await
            .unwrap()
            .outcome
            .unwrap();
        let updated = next_snapshot(&mut body).await;
        assert_eq!(updated, h.session.snapshot());
        assert!(updated.seq > initial.seq);
        assert_eq!(updated.activity, Activity::Idle);

        let mut reconnected = h.subscribe().await;
        assert_eq!(next_snapshot(&mut reconnected).await, updated);
        h.session.close().await;
        for body in [&mut body, &mut reconnected] {
            let closed = next_snapshot(body).await;
            assert_eq!(closed.lifecycle, LifecycleState::Closed);
            assert!(closed.seq > updated.seq);
            assert!(timeout(Duration::from_secs(5), body.next())
                .await
                .unwrap()
                .is_none());
        }
    }

    #[tokio::test]
    async fn disconnecting_a_subscriber_does_not_cancel_the_operation() {
        let h = Harness::new();
        let mut body = h.subscribe().await;
        next_snapshot(&mut body).await;
        let receipt = h.session.initialize().unwrap();
        drop(body);
        receipt.await.unwrap().outcome.unwrap();
        assert_eq!(h.session.snapshot().activity, Activity::Idle);
        assert_eq!(h.session.snapshot().lifecycle, LifecycleState::Open);
        h.core.shutdown().await;
    }

    #[tokio::test]
    async fn subscription_errors_and_host_origin_restrictions_are_preserved() {
        let h = Harness::new();
        for (id, host, origin, status, kind) in [
            (
                "missing",
                "127.0.0.1:1421",
                None,
                StatusCode::BAD_REQUEST,
                "invalid_params",
            ),
            (
                "9007199254740992",
                "127.0.0.1:1421",
                None,
                StatusCode::BAD_REQUEST,
                "invalid_params",
            ),
            (
                "2",
                "127.0.0.1:1421",
                None,
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                "1",
                "attacker.example:1421",
                None,
                StatusCode::FORBIDDEN,
                "forbidden",
            ),
            (
                "1",
                "127.0.0.1:1421",
                Some("https://attacker.example"),
                StatusCode::FORBIDDEN,
                "forbidden",
            ),
        ] {
            let mut request =
                Request::get(format!("/bridge/sessions/{id}/events")).header("host", host);
            if let Some(origin) = origin {
                request = request.header("origin", origin);
            }
            let response = h
                .router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            assert_eq!(body["kind"], kind);
        }
        h.core.shutdown().await;
        let response = h
            .router
            .oneshot(
                Request::get("/bridge/sessions/1/events")
                    .header("host", "127.0.0.1:1421")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn unsafe_snapshot_values_end_the_stream_with_a_typed_error() {
        let h = Harness::new();
        let mut snapshot = h.session.snapshot();
        snapshot.seq = 1_u64 << 53;
        let (_sender, receiver) = watch::channel(snapshot);
        let body = Sse::new(snapshots(receiver)).into_response().into_body();
        let bytes = timeout(Duration::from_secs(5), to_bytes(body, usize::MAX))
            .await
            .unwrap()
            .unwrap();
        let event = std::str::from_utf8(&bytes).unwrap();
        assert!(event.contains("event: bridge-error\n"), "{event}");
        assert!(event.contains("\"kind\":\"internal_error\""), "{event}");
        assert!(!event.contains("event: session\n"));
        h.core.shutdown().await;
    }

    #[tokio::test]
    async fn runtime_shutdown_finishes_with_an_active_sse_connection() {
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
        let session = runtime
            .core
            .session_manager()
            .create(path.join("input.txt"), Settings::default())
            .unwrap();
        let mut connection =
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, runtime.port))
                .await
                .unwrap();
        connection.write_all(format!("GET /bridge/sessions/{}/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", session.id().0, runtime.port).as_bytes()).await.unwrap();
        let mut response = Vec::new();
        timeout(Duration::from_secs(5), async {
            while !String::from_utf8_lossy(&response).contains("event: session\n") {
                let mut buffer = [0; 4096];
                let read = connection.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "SSE connection ended without an initial snapshot");
                response.extend_from_slice(&buffer[..read]);
            }
        })
        .await
        .unwrap();
        let (shutdown, drained) = tokio::join!(
            timeout(Duration::from_secs(5), runtime.shutdown()),
            timeout(
                Duration::from_secs(5),
                connection.read_to_end(&mut response)
            ),
        );
        shutdown.unwrap();
        drained.unwrap().unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"lifecycle\":\"Closed\""), "{response}");
        assert_eq!(session.snapshot().lifecycle, LifecycleState::Closed);
        assert!(
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, runtime.port))
                .await
                .is_err()
        );
    }
}
