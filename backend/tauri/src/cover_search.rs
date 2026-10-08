//! Finding a cover on the web: a search window whose injected script marks
//! images that can be picked, and the download of the picked image.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use specta::Type;
use tauri::{Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_specta::Event;
use url::Url;
use wbook_core::{export::cover::MAX_IMAGE_BYTES, session::SessionId, Wbook};

use crate::commands::dto::{decode_base64, CommandError, ErrorKind};
use crate::errors::{DownloadSnafu, WindowSnafu};
use crate::windows;

const PREFIX: &str = "cover-search-";
/// Picks leave the page by navigating here. The `.invalid` TLD never
/// resolves, and the navigation is cancelled before any request is sent.
const PICK_HOST: &str = "wbook-cover.invalid";
const SEARCH: &str = "https://www.bing.com/images/search";
const PICKER: &str = include_str!("cover_picker.js");
const TIMEOUT: Duration = Duration::from_secs(30);

/// Sent to the session window when an image was picked in its search window.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct CoverPicked {
    pub session: SessionId,
    pub url: String,
    /// The page the image was shown on; some hosts refuse hotlinks without it.
    pub referer: Option<String>,
}

pub fn label(id: SessionId) -> String {
    format!("{PREFIX}{}", id.0)
}

/// Opens the search window of a session, or shows `query` in the open one.
///
/// The window shows remote pages, so it is left out of every capability and
/// cannot reach the app's commands; picks reach the app only as navigations.
pub fn open<R: Runtime>(
    opener: &WebviewWindow<R>,
    core: &Wbook,
    id: SessionId,
    query: &str,
) -> Result<(), CommandError> {
    let session = core.session_manager().get(id)?;
    let mut url = Url::parse(SEARCH).expect("search URL is valid");
    url.query_pairs_mut().append_pair("q", query);
    let label = label(id);
    let app = opener.app_handle();
    if let Some(window) = app.get_webview_window(&label) {
        return window
            .navigate(url)
            .and_then(|()| window.unminimize())
            .and_then(|()| window.set_focus())
            .context(WindowSnafu {
                action: "focus",
                label,
            })
            .map_err(Into::into);
    }
    let picker = app.clone();
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title("搜索封面")
        .inner_size(1100.0, 800.0)
        .initialization_script(PICKER)
        .on_navigation(move |url| {
            if url.host_str() != Some(PICK_HOST) {
                return true;
            }
            picked(&picker, id, url);
            false
        })
        .build();
    let window = match built {
        Err(
            tauri::Error::WindowLabelAlreadyExists(_) | tauri::Error::WebviewLabelAlreadyExists(_),
        ) => return Ok(()),
        built => built.context(WindowSnafu {
            action: "create",
            label: &label,
        })?,
    };
    windows::center(&window, opener.current_monitor().ok().flatten());
    let app = app.clone();
    let receiver = session.subscribe();
    tauri::async_runtime::spawn(async move {
        windows::until_closed(receiver).await;
        windows::destroy(&app, &label);
    });
    Ok(())
}

fn picked<R: Runtime>(app: &tauri::AppHandle<R>, id: SessionId, pick: &Url) {
    let mut url = None;
    let mut referer = None;
    for (key, value) in pick.query_pairs() {
        match key.as_ref() {
            "src" => url = Some(value.into_owned()),
            "page" => referer = Some(value.into_owned()),
            _ => {}
        }
    }
    let Some(url) = url.filter(|url| !url.is_empty()) else {
        return;
    };
    let session = windows::session_label(id);
    if let Err(error) = (CoverPicked {
        session: id,
        url,
        referer,
    })
    .emit_to(app, &session)
    {
        tracing::warn!(
            session = id.0,
            "Could not hand over the picked cover: {error}"
        );
        return;
    }
    // Closing from inside the navigation handler would tear down the webview
    // that is still dispatching it.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        windows::destroy(&app, &label(id));
        if let Some(window) = app.get_webview_window(&session) {
            let _ = window.set_focus();
        }
    });
}

/// Fetches a picked image, which may also be inlined as a `data:` URL.
pub async fn download(url: &str, referer: Option<&str>) -> Result<Vec<u8>, CommandError> {
    let invalid = |message: &str| CommandError::new(ErrorKind::InvalidParams, message);
    let parsed = Url::parse(url).map_err(|_| invalid("invalid image URL"))?;
    match parsed.scheme() {
        "data" => {
            let (_, data) = url
                .split_once(";base64,")
                .ok_or_else(|| invalid("only base64 data URLs are supported"))?;
            return decode_base64(data);
        }
        "http" | "https" => {}
        _ => return Err(invalid("only web images can be downloaded")),
    }
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        // Image hosts often reject requests that do not look like a browser.
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36")
        .build()
        .context(DownloadSnafu)?;
    let mut request = client
        .get(parsed)
        .header(reqwest::header::ACCEPT, "image/*");
    if let Some(referer) = referer {
        request = request.header(reqwest::header::REFERER, referer);
    }
    let mut response = request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .context(DownloadSnafu)?;
    let too_large = || invalid("the image is larger than the cover limit");
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES as u64)
    {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.context(DownloadSnafu)? {
        if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use axum::{http::HeaderMap, routing::get, Router};

    use super::*;

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn downloads_with_the_page_as_referer() {
        let base = serve(Router::new().route(
            "/cover.png",
            get(|headers: HeaderMap| async move {
                let referer = headers[reqwest::header::REFERER]
                    .to_str()
                    .unwrap()
                    .to_owned();
                referer.into_bytes()
            }),
        ))
        .await;
        let bytes = download(
            &format!("{base}/cover.png"),
            Some("https://example.com/page"),
        )
        .await
        .unwrap();
        assert_eq!(bytes, b"https://example.com/page");
    }

    #[tokio::test]
    async fn refuses_oversized_failed_and_foreign_sources() {
        let base = serve(
            Router::new()
                .route("/large", get(|| async { vec![0u8; MAX_IMAGE_BYTES + 1] }))
                .route(
                    "/missing",
                    get(|| async { (axum::http::StatusCode::NOT_FOUND, "gone") }),
                ),
        )
        .await;
        let error = download(&format!("{base}/large"), None).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidParams);
        let error = download(&format!("{base}/missing"), None)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Network);
        for url in ["file:///C:/secret.png", "not a url", "data:image/png,raw"] {
            let error = download(url, None).await.unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidParams, "{url}");
        }
        assert_eq!(
            download("data:image/png;base64,YWJj", None).await.unwrap(),
            b"abc"
        );
    }

    #[test]
    fn search_windows_are_not_session_windows() {
        assert_eq!(windows::session_of(&label(SessionId(3))), None);
    }
}
