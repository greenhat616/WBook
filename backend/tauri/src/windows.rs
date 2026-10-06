use std::sync::Arc;

use snafu::ResultExt;
use tauri::{
    AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window,
    WindowEvent,
};
use tokio::sync::watch;
use wbook_core::{
    session::{LifecycleState, Rejected, SessionId, SessionSnapshot},
    Wbook,
};

use crate::{commands::dto::CommandError, errors::WindowSnafu};

pub const MAIN_WINDOW: &str = "main";
const SESSION_PREFIX: &str = "session-";

type Opener = dyn Fn(&Wbook, SessionId) -> Result<(), CommandError> + Send + Sync;

/// Commands cannot be generic over the Tauri runtime, so the runtime-specific
/// handle is erased here and managed as ordinary state.
pub struct SessionWindows(Box<Opener>);

impl SessionWindows {
    pub fn new<R: Runtime>(app: AppHandle<R>) -> Self {
        Self(Box::new(move |core, id| open(&app, core, id)))
    }

    pub fn open(&self, core: &Wbook, id: SessionId) -> Result<(), CommandError> {
        (self.0)(core, id)
    }
}

pub fn session_label(id: SessionId) -> String {
    format!("{SESSION_PREFIX}{}", id.0)
}

pub fn session_of(label: &str) -> Option<SessionId> {
    let id = SessionId(label.strip_prefix(SESSION_PREFIX)?.parse().ok()?);
    // Reject spellings such as `session-01` or `session-+1` that parse to the
    // same ID but would never be produced for a window.
    (session_label(id) == label).then_some(id)
}

pub fn open<R: Runtime>(
    app: &AppHandle<R>,
    core: &Wbook,
    id: SessionId,
) -> Result<(), CommandError> {
    let session = core.session_manager().get(id)?;
    let snapshot = session.snapshot();
    match snapshot.lifecycle {
        LifecycleState::Open => {}
        LifecycleState::Closing => return Err(Rejected::Closing.into()),
        LifecycleState::Closed => return Err(Rejected::Closed.into()),
    }
    let label = session_label(id);
    if let Some(window) = app.get_webview_window(&label) {
        return focus(&window);
    }
    let title = snapshot
        .source
        .file_name()
        .unwrap_or(snapshot.source.as_str());
    let url = WebviewUrl::App(format!("index.html#/sessions/{}", id.0).into());
    let built = WebviewWindowBuilder::new(app, &label, url)
        .title(title)
        .inner_size(1200.0, 800.0)
        // Match the main window so bridge requests keep the same Origin.
        .use_https_scheme(false)
        .build();
    match built {
        Err(
            tauri::Error::WindowLabelAlreadyExists(_) | tauri::Error::WebviewLabelAlreadyExists(_),
        ) => {
            // A concurrent call created the window and owns its watcher.
            return app
                .get_webview_window(&label)
                .map_or(Ok(()), |window| focus(&window));
        }
        built => {
            built.context(WindowSnafu {
                action: "create",
                label: &label,
            })?;
        }
    }
    let app = app.clone();
    let receiver = session.subscribe();
    tauri::async_runtime::spawn(async move {
        until_closed(receiver).await;
        destroy(&app, &label);
    });
    Ok(())
}

fn focus<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), CommandError> {
    window
        .unminimize()
        .and_then(|()| window.show())
        .and_then(|()| window.set_focus())
        .context(WindowSnafu {
            action: "focus",
            label: window.label(),
        })?;
    Ok(())
}

/// Resolves once the session is closed, including when it already was.
pub async fn until_closed(mut receiver: watch::Receiver<SessionSnapshot>) {
    // A dropped sender also means the session can no longer change.
    let _ = receiver
        .wait_for(|snapshot| snapshot.lifecycle == LifecycleState::Closed)
        .await;
}

fn destroy<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        if let Err(error) = window.destroy() {
            tracing::warn!(label, "Could not destroy window: {error}");
        }
    }
}

pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let label = window.label();
    if label == MAIN_WINDOW {
        // Exiting runs the shutdown in the exit handler before any window is
        // destroyed, so sessions never outlive the main window.
        api.prevent_close();
        window.app_handle().exit(0);
    } else if let Some(id) = session_of(label) {
        // Keep the window until the session has released its resources.
        api.prevent_close();
        let app = window.app_handle().clone();
        let core = window.state::<Arc<Wbook>>().inner().clone();
        let label = label.to_owned();
        tauri::async_runtime::spawn(async move {
            close_session(&core, id).await;
            destroy(&app, &label);
        });
    }
}

pub async fn close_session(core: &Wbook, id: SessionId) {
    // A missing session was already closed elsewhere; only the window remains.
    if let Ok(report) = core.session_manager().close(id).await {
        for warning in &report.cleanup_failures {
            tracing::warn!(session = id.0, path = ?warning.path, "{}", warning.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;
    use wbook_core::{
        parser::toc::TocParserConfig, session::SessionHandle, workspace::ProcessingOptions, Params,
    };

    use super::*;

    fn session() -> (tempfile::TempDir, Wbook, SessionHandle) {
        let directory = tempfile::tempdir().unwrap();
        let path = camino::Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
        let core = Wbook::new(Params {
            data_dir: path.clone(),
            config_dir: path.clone(),
        });
        let session = core
            .session_manager()
            .create(
                path.join("input.txt"),
                ProcessingOptions {
                    filters: vec![],
                    toc: TocParserConfig::SplitEvenly { parts: 1 },
                },
            )
            .unwrap();
        (directory, core, session)
    }

    #[test]
    fn labels_round_trip_only_in_their_canonical_form() {
        for id in [1, 42, (1 << 53) - 1, u64::MAX] {
            assert_eq!(
                session_of(&session_label(SessionId(id))),
                Some(SessionId(id))
            );
        }
        for label in [
            MAIN_WINDOW,
            "session-",
            "session-01",
            "session-+1",
            "session- 1",
            "session-1a",
            "session-18446744073709551616",
            "Session-1",
        ] {
            assert_eq!(session_of(label), None, "{label}");
        }
    }

    #[tokio::test]
    async fn waiting_for_closure_covers_earlier_and_later_closes() {
        let (_directory, _core, session) = session();
        let waiting = tokio::spawn(until_closed(session.subscribe()));
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        session.close().await;
        timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap();
        timeout(Duration::from_secs(5), until_closed(session.subscribe()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn close_requests_close_the_session_and_tolerate_missing_ones() {
        let (_directory, core, session) = session();
        close_session(&core, session.id()).await;
        assert_eq!(session.snapshot().lifecycle, LifecycleState::Closed);
        assert!(core.session_manager().get(session.id()).is_err());
        close_session(&core, session.id()).await;
        close_session(&core, SessionId(99)).await;
    }
}
