use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use specta::Type;
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};
use tauri_specta::Event;
use tokio::sync::watch;
use wbook_core::{
    session::{LifecycleState, Rejected, SessionHandle, SessionId, SessionSnapshot},
    Wbook,
};

use crate::{commands::dto::CommandError, errors::WindowSnafu};

pub const MAIN_WINDOW: &str = "main";
pub const SETTINGS_WINDOW: &str = "settings";
const SESSION_PREFIX: &str = "session-";
const SESSION_SETTINGS_PREFIX: &str = "session-settings-";
/// How long a hidden window waits for its frontend before it is shown anyway,
/// so a page that fails to load is still visible and debuggable.
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Sent to the main window once a windowed session has closed, so its list
/// drops the session without waiting for focus.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SessionClosed {
    pub session: SessionId,
}

pub fn session_label(id: SessionId) -> String {
    format!("{SESSION_PREFIX}{}", id.0)
}

pub fn session_settings_label(id: SessionId) -> String {
    format!("{SESSION_SETTINGS_PREFIX}{}", id.0)
}

pub fn session_of(label: &str) -> Option<SessionId> {
    let id = SessionId(label.strip_prefix(SESSION_PREFIX)?.parse().ok()?);
    // Reject spellings such as `session-01` or `session-+1` that parse to the
    // same ID but would never be produced for a window.
    (session_label(id) == label).then_some(id)
}

/// Opens the window of a session, or focuses it if it is already open.
pub fn open<R: Runtime>(
    opener: &WebviewWindow<R>,
    core: &Wbook,
    id: SessionId,
) -> Result<(), CommandError> {
    let (session, snapshot) = open_session(core, id)?;
    let label = session_label(id);
    let title = snapshot
        .source
        .file_name()
        .unwrap_or(snapshot.source.as_str());
    let route = format!("sessions/{}", id.0);
    if !open_window(opener, &label, &route, title, (1200.0, 800.0))? {
        return Ok(());
    }
    let app = opener.app_handle().clone();
    let receiver = session.subscribe();
    tauri::async_runtime::spawn(async move {
        until_closed(receiver).await;
        destroy(&app, &label);
        if let Err(error) = (SessionClosed { session: id }).emit_to(&app, MAIN_WINDOW) {
            tracing::warn!(session = id.0, "Could not announce closed session: {error}");
        }
    });
    Ok(())
}

/// Opens the settings window of a session; it lives no longer than the session.
pub fn open_session_settings<R: Runtime>(
    opener: &WebviewWindow<R>,
    core: &Wbook,
    id: SessionId,
) -> Result<(), CommandError> {
    let (session, snapshot) = open_session(core, id)?;
    let label = session_settings_label(id);
    let title = format!(
        "本书设置 · {}",
        snapshot
            .source
            .file_name()
            .unwrap_or(snapshot.source.as_str())
    );
    let route = format!("sessions/{}/settings", id.0);
    if !open_window(opener, &label, &route, &title, (960.0, 800.0))? {
        return Ok(());
    }
    let app = opener.app_handle().clone();
    let receiver = session.subscribe();
    tauri::async_runtime::spawn(async move {
        until_closed(receiver).await;
        destroy(&app, &label);
    });
    Ok(())
}

/// Opens the global settings window, which is shared by every other window.
pub fn open_settings<R: Runtime>(opener: &WebviewWindow<R>) -> Result<(), CommandError> {
    open_window(opener, SETTINGS_WINDOW, "settings", "设置", (960.0, 800.0))?;
    Ok(())
}

fn open_session(
    core: &Wbook,
    id: SessionId,
) -> Result<(SessionHandle, SessionSnapshot), CommandError> {
    let session = core.session_manager().get(id)?;
    let snapshot = session.snapshot();
    match snapshot.lifecycle {
        LifecycleState::Open => Ok((session, snapshot)),
        LifecycleState::Closing => Err(Rejected::Closing.into()),
        LifecycleState::Closed => Err(Rejected::Closed.into()),
    }
}

/// Creates a hidden window for `route` on the screen of `opener`, or focuses
/// the existing one.
///
/// Returns whether this call created the window, in which case the caller
/// owns whatever must watch over it.
fn open_window<R: Runtime>(
    opener: &WebviewWindow<R>,
    label: &str,
    route: &str,
    title: &str,
    (width, height): (f64, f64),
) -> Result<bool, CommandError> {
    let app = opener.app_handle();
    if let Some(window) = app.get_webview_window(label) {
        focus(&window)?;
        return Ok(false);
    }
    let url = WebviewUrl::App(format!("index.html#/{route}").into());
    let built = WebviewWindowBuilder::new(app, label, url)
        .title(title)
        .inner_size(width, height)
        // Shown by `ready` once the frontend has rendered, avoiding a blank flash.
        .visible(false)
        // Match the main window so bridge requests keep the same Origin.
        .use_https_scheme(false)
        .build();
    match built {
        Err(
            tauri::Error::WindowLabelAlreadyExists(_) | tauri::Error::WebviewLabelAlreadyExists(_),
        ) => {
            // A concurrent call created the window and owns its watcher.
            if let Some(window) = app.get_webview_window(label) {
                focus(&window)?;
            }
            return Ok(false);
        }
        built => {
            let window = built.context(WindowSnafu {
                action: "create",
                label,
            })?;
            center(&window, opener.current_monitor().ok().flatten());
        }
    }
    reveal_after(app, label, READY_TIMEOUT);
    Ok(true)
}

/// Centers a window that is still hidden on `monitor`, or on the screen
/// under the cursor.
///
/// Tauri's own centering always picks the primary monitor, which puts the
/// window on another screen whenever the user works on a secondary one.
pub fn center<R: Runtime>(window: &WebviewWindow<R>, monitor: Option<Monitor>) {
    let monitor = monitor
        .or_else(|| {
            let cursor = window.cursor_position().ok()?;
            window.monitor_from_point(cursor.x, cursor.y).ok()?
        })
        .or_else(|| window.primary_monitor().ok()?);
    let Some(monitor) = monitor else {
        return;
    };
    let area = monitor.work_area();
    // Moving onto a screen with another scale factor resizes the window, so
    // place it once more with the size it has there.
    for _ in 0..2 {
        let size = match window.outer_size() {
            Ok(size) => size,
            Err(error) => {
                tracing::warn!(label = window.label(), "Could not measure window: {error}");
                return;
            }
        };
        let position = PhysicalPosition::new(
            area.position.x + (area.size.width as i32 - size.width as i32) / 2,
            area.position.y + (area.size.height as i32 - size.height as i32) / 2,
        );
        if let Err(error) = window.set_position(position) {
            tracing::warn!(label = window.label(), "Could not center window: {error}");
            return;
        }
    }
}

fn focus<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), CommandError> {
    let visible = window.is_visible().context(WindowSnafu {
        action: "inspect",
        label: window.label(),
    })?;
    // A window still waiting for its frontend is shown by `ready` or the
    // fallback; showing it here would bring back the blank flash.
    if !visible {
        return Ok(());
    }
    window
        .unminimize()
        .and_then(|()| window.set_focus())
        .context(WindowSnafu {
            action: "focus",
            label: window.label(),
        })?;
    Ok(())
}

/// Shows a window whose frontend reports that it has rendered.
///
/// Repeated reports, such as after a reload, leave a visible window alone so
/// they do not steal focus.
pub fn ready<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), CommandError> {
    reveal(window).context(WindowSnafu {
        action: "show",
        label: window.label(),
    })?;
    Ok(())
}

fn reveal<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<bool> {
    if window.is_visible()? {
        return Ok(false);
    }
    window.show()?;
    window.set_focus()?;
    Ok(true)
}

/// Shows the window after `delay` unless its frontend has done so already.
pub fn reveal_after<R: Runtime>(app: &AppHandle<R>, label: &str, delay: Duration) {
    let app = app.clone();
    let label = label.to_owned();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let Some(window) = app.get_webview_window(&label) else {
            return;
        };
        match reveal(&window) {
            Ok(true) => tracing::warn!(label, "Window frontend did not report ready in time"),
            Ok(false) => {}
            Err(error) => tracing::warn!(label, "Could not show window: {error}"),
        }
    });
}

/// Resolves once the session is closed, including when it already was.
pub async fn until_closed(mut receiver: watch::Receiver<SessionSnapshot>) {
    // A dropped sender also means the session can no longer change.
    let _ = receiver
        .wait_for(|snapshot| snapshot.lifecycle == LifecycleState::Closed)
        .await;
}

pub(crate) fn destroy<R: Runtime>(app: &AppHandle<R>, label: &str) {
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
    use wbook_core::{settings::Settings, Params};

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
            .create(path.join("input.txt"), Settings::default())
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
            SETTINGS_WINDOW,
            // Closing a session settings window must not close the session.
            &session_settings_label(SessionId(1)),
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
