use std::path::PathBuf;

use camino::Utf8PathBuf;
use wbook_core::{
    document::{DocumentVersion, EditBatch, ParsedResults},
    export::ExportOptions,
    parser::{toc::TocParserConfig, Metadata},
    session::{CancelReply, OperationId, SessionId, SessionSnapshot},
    types::{Port, TextRange},
    workspace::{PreviewInfo, ProcessingOptions, Revision, WorkspaceResults},
    Wbook,
};

pub mod dto;
use dto::*;

pub use api::*;

#[wbook_command_macros::unified_commands]
mod api {
    use super::*;

    #[desktop_only]
    #[query]
    pub fn get_port(port: tauri::State<'_, Port>) -> u16 {
        port.0
    }

    pub async fn create_session(
        app: &Wbook,
        source: Utf8PathBuf,
        options: ProcessingOptions,
    ) -> Result<SessionSnapshot, CommandError> {
        Ok(app.session_manager().create(source, options)?.snapshot())
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

    pub async fn parse_session(
        app: &Wbook,
        session_id: SessionId,
        config: TocParserConfig,
    ) -> Result<OperationResponse<ParsedResults>, CommandError> {
        complete(app.session_manager().get(session_id)?.parse(config)).await
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
        options: ExportOptions,
    ) -> Result<OperationResponse<PreviewInfo>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .render_preview(expected, options),
        )
        .await
    }

    pub async fn export_epub(
        app: &Wbook,
        session_id: SessionId,
        expected: Revision,
        options: ExportOptions,
        destination: PathBuf,
    ) -> Result<OperationResponse<ExportedBook>, CommandError> {
        complete(
            app.session_manager()
                .get(session_id)?
                .export_epub(expected, options, destination),
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
