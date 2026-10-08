use std::{path::PathBuf, sync::Arc};

use camino::Utf8PathBuf;
use wbook_core::{
    document::{DocumentVersion, EditBatch, ParsedResults},
    export::TemplateOverrides,
    parser::Metadata,
    session::{CancelReply, OperationId, SessionId, SessionSnapshot},
    settings::{Settings, StoredSettings},
    types::{Port, TextRange},
    workspace::{PreviewInfo, Revision, WorkspaceResults},
    Wbook,
};

pub mod dto;
use dto::*;

pub use api::*;

/// Commands and events share one builder so the exported bindings and the
/// running app register the same events.
pub fn specta_builder<R: tauri::Runtime>() -> (String, tauri_specta::Builder<R>) {
    let (queries, builder) = builder::<R>();
    let events = tauri_specta::collect_events![
        crate::windows::SessionClosed,
        crate::cover_search::CoverPicked
    ];
    (queries, builder.events(events))
}

#[wbook_command_macros::unified_commands]
mod api {
    use super::*;

    #[desktop_only]
    #[query]
    pub fn get_port(port: tauri::State<'_, Port>) -> u16 {
        port.0
    }

    // Async because creating a window from a synchronous command can deadlock on Windows.
    #[desktop_only]
    pub async fn open_session_window<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
        core: tauri::State<'_, Arc<Wbook>>,
        session_id: SessionId,
    ) -> Result<(), CommandError> {
        crate::windows::open(&window, &core, session_id)
    }

    #[desktop_only]
    pub async fn open_settings_window<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
    ) -> Result<(), CommandError> {
        crate::windows::open_settings(&window)
    }

    #[desktop_only]
    pub async fn open_session_settings_window<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
        core: tauri::State<'_, Arc<Wbook>>,
        session_id: SessionId,
    ) -> Result<(), CommandError> {
        crate::windows::open_session_settings(&window, &core, session_id)
    }

    /// Opens a web image search whose picks arrive as `CoverPicked`.
    #[desktop_only]
    pub async fn open_cover_search<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
        core: tauri::State<'_, Arc<Wbook>>,
        session_id: SessionId,
        query: String,
    ) -> Result<(), CommandError> {
        crate::cover_search::open(&window, &core, session_id, &query)
    }

    /// Downloads a picked image and makes it the cover.
    #[desktop_only]
    pub async fn set_cover_from_url(
        core: tauri::State<'_, Arc<Wbook>>,
        session_id: SessionId,
        expected: Revision,
        url: String,
        referer: Option<String>,
    ) -> Result<OperationResponse<Revision>, CommandError> {
        let session = core.session_manager().get(session_id)?;
        let image = crate::cover_search::download(&url, referer.as_deref()).await?;
        complete(session.set_cover_image(expected, Some(image))).await
    }

    #[desktop_only]
    pub fn window_ready<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
    ) -> Result<(), CommandError> {
        crate::windows::ready(&window)
    }

    #[query]
    pub async fn get_settings(app: &Wbook) -> Result<StoredSettings, CommandError> {
        Ok(app.settings().get())
    }

    pub async fn save_settings(
        app: &Wbook,
        expected: u64,
        settings: Settings,
    ) -> Result<StoredSettings, CommandError> {
        Ok(app.settings().save(expected, settings)?)
    }

    #[query]
    pub async fn default_settings(_app: &Wbook) -> Result<Settings, CommandError> {
        Ok(Settings::default())
    }

    #[query]
    pub async fn builtin_templates(_app: &Wbook) -> Result<TemplateOverrides, CommandError> {
        Ok(TemplateOverrides::builtin())
    }

    /// The session starts from a copy of the current global settings.
    pub async fn create_session(
        app: &Wbook,
        source: Utf8PathBuf,
    ) -> Result<SessionSnapshot, CommandError> {
        let settings = app.settings().current();
        Ok(app.session_manager().create(source, settings)?.snapshot())
    }

    #[query]
    pub async fn list_sessions(app: &Wbook) -> Result<Vec<SessionSnapshot>, CommandError> {
        Ok(app.session_manager().list())
    }

    #[query]
    pub async fn get_session(
        app: &Wbook,
        session_id: SessionId,
    ) -> Result<SessionSnapshot, CommandError> {
        Ok(app.session_manager().get(session_id)?.snapshot())
    }

    pub async fn close_session(
        app: &Wbook,
        session_id: SessionId,
    ) -> Result<ClosedSession, CommandError> {
        Ok(app
            .session_manager()
            .close(session_id)
            .await?
            .as_ref()
            .into())
    }

    pub async fn initialize_session(
        app: &Wbook,
        session_id: SessionId,
    ) -> Result<OperationResponse<Revision>, CommandError> {
        complete(app.session_manager().get(session_id)?.initialize()).await
    }

    #[query]
    pub async fn get_session_settings(
        app: &Wbook,
        session_id: SessionId,
    ) -> Result<Settings, CommandError> {
        Ok(app.session_manager().get(session_id)?.settings()?)
    }

    pub async fn set_session_settings(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
        settings: Settings,
    ) -> Result<OperationResponse<Revision>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .set_settings(expected, settings),
        )
        .await
    }

    pub async fn parse_session(
        app: &Wbook,
        session_id: SessionId,
    ) -> Result<OperationResponse<ParsedResults>, CommandError> {
        complete(app.session_manager().get(session_id)?.parse()).await
    }

    pub async fn install_results(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
        results: ParsedResults,
    ) -> Result<OperationResponse<Revision>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .install(expected, results),
        )
        .await
    }

    pub async fn apply_edits(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
        batch: EditBatch,
    ) -> Result<OperationResponse<Revision>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .apply_edits(expected, batch),
        )
        .await
    }

    pub async fn set_metadata_overrides(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
        overrides: Metadata,
    ) -> Result<OperationResponse<Revision>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .set_metadata_overrides(expected, overrides),
        )
        .await
    }

    /// `image` is base64, so the browser bridge can carry it in JSON.
    pub async fn set_cover_image(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
        image: Option<String>,
    ) -> Result<OperationResponse<Revision>, CommandError> {
        let image = image.as_deref().map(decode_base64).transpose()?;
        complete(
            app.session_manager()
                .get(session_id)?
                .set_cover_image(expected, image),
        )
        .await
    }

    pub async fn render_cover(
        app: &Wbook,
        session_id: SessionId,
    ) -> Result<OperationResponse<RenderedCover>, CommandError> {
        complete(app.session_manager().get(session_id)?.render_cover()).await
    }

    #[query]
    pub async fn read_text(
        app: &Wbook,
        session_id: SessionId,
        version: DocumentVersion,
        range: TextRange,
    ) -> Result<OperationResponse<String>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .read_text(version, range),
        )
        .await
    }

    #[query]
    pub async fn read_results(
        app: &Wbook,
        session_id: SessionId,
    ) -> Result<OperationResponse<WorkspaceResults>, CommandError> {
        complete(app.session_manager().get(session_id)?.read_results()).await
    }

    pub async fn render_preview(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
    ) -> Result<OperationResponse<PreviewInfo>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .render_preview(expected),
        )
        .await
    }

    pub async fn export_epub(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
        destination: PathBuf,
    ) -> Result<OperationResponse<ExportedBook>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .export_epub(expected, destination),
        )
        .await
    }

    pub async fn cancel_operation(
        app: &Wbook,
        session_id: SessionId,
        operation_id: OperationId,
    ) -> Result<CancelReply, CommandError> {
        Ok(app.session_manager().get(session_id)?.cancel(operation_id))
    }
}
