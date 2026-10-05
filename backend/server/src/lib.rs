use std::{future::Future, io, net::SocketAddr};

use axum::{
    extract::Request,
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::info;

mod router;

pub fn local_router(app: Router, addr: SocketAddr) -> Router {
    let hosts = [addr.to_string(), format!("localhost:{}", addr.port())];
    let mut origins = vec![
        format!("http://{}", hosts[0]),
        format!("http://{}", hosts[1]),
        "tauri://localhost".into(),
        "http://tauri.localhost".into(),
        "https://tauri.localhost".into(),
    ];
    if cfg!(debug_assertions) {
        origins.extend([
            "http://localhost:1420".into(),
            "http://127.0.0.1:1420".into(),
        ]);
    }
    let origins: Vec<HeaderValue> = origins
        .into_iter()
        .map(|value| value.parse().unwrap())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(origins.clone())
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE]);

    router::register(app).layer(cors).layer(middleware::from_fn(
        move |request: Request, next: Next| {
            let allowed = request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|host| hosts.iter().any(|allowed| allowed == host))
                && request
                    .headers()
                    .get(header::ORIGIN)
                    .is_none_or(|origin| origins.contains(origin));
            async move {
                if allowed {
                    next.run(request).await
                } else {
                    forbidden()
                }
            }
        },
    ))
}

fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "kind": "forbidden",
            "message": "Host or Origin is not allowed",
        })),
    )
        .into_response()
}

pub async fn serve(
    listener: TcpListener,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let addr = listener.local_addr()?;
    info!("Backend is listening on http://{}", addr);
    axum::serve(listener, local_router(app, addr))
        .with_graceful_shutdown(shutdown)
        .await
}
