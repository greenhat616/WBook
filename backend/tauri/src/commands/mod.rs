use std::{path::PathBuf, sync::Arc};

use camino::Utf8PathBuf;
use serde_json::Value;
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

pub const DESKTOP_ONLY_COMMANDS: &[&str] = &["get_port"];

#[tauri::command]
#[specta::specta]
pub fn get_port(port: tauri::State<'_, Port>) -> u16 {
    port.0
}

// All commands currently depend on Wbook, so a fixed declaration avoids a second
// registry and keeps service injection out of the wire contract.
macro_rules! commands {
    ($(async fn $name:ident($app:ident: &Wbook $(, $arg:ident: $ty:ty)*) -> $output:ty $body:block)*) => {
        $(
            pub mod $name {
                use super::*;

                pub async fn call($app: &Wbook, $($arg: $ty),*) -> Result<$output, CommandError> {
                    check_integers(&($(&$arg,)*), ErrorKind::InvalidParams)?;
                    let result: Result<$output, CommandError> = async move { $body }.await;
                    let result = result?;
                    check_integers(&result, ErrorKind::InternalError)?;
                    Ok(result)
                }

                pub async fn rpc(app: &Wbook, params: Value) -> Result<Value, CommandError> {
                    #[derive(serde::Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Args { $($arg: $ty),* }

                    let args: Args = serde_json::from_value(params).map_err(|error| {
                        CommandError::new(ErrorKind::InvalidParams, error.to_string())
                    })?;
                    let _ = &args;
                    let result = call(app, $(args.$arg),*).await?;
                    serde_json::to_value(result).map_err(|error| {
                        CommandError::new(ErrorKind::InternalError, error.to_string())
                    })
                }
            }

            #[tauri::command]
            #[specta::specta]
            pub async fn $name(app: tauri::State<'_, Arc<Wbook>>, $($arg: $ty),*)
                -> Result<$output, CommandError>
            {
                $name::call(&app, $($arg),*).await
            }
        )*

        pub fn builder<R: tauri::Runtime>() -> tauri_specta::Builder<R> {
            tauri_specta::Builder::new()
                .commands(tauri_specta::collect_commands![get_port, $($name),*])
                .constant("DESKTOP_ONLY_COMMANDS", DESKTOP_ONLY_COMMANDS)
                .error_handling(tauri_specta::ErrorHandlingMode::Result)
                // Shared calls reject values outside JSON/JavaScript's exact integer range.
                .dangerously_cast_bigints_to_number()
        }

        pub async fn dispatch(app: &Wbook, method: &str, params: Value) -> Result<Value, CommandError> {
            match method {
                $(stringify!($name) => $name::rpc(app, params).await,)*
                name if DESKTOP_ONLY_COMMANDS.contains(&name) => Err(CommandError::new(
                    ErrorKind::PlatformUnsupported, "This operation requires the desktop application",
                )),
                _ => Err(CommandError::new(ErrorKind::MethodNotFound, "Unknown command")),
            }
        }
    };
}

commands! {
    async fn create_session(app: &Wbook, source: Utf8PathBuf, options: ProcessingOptions) -> SessionSnapshot {
        Ok(app.session_manager().create(source, options)?.snapshot())
    }

    async fn list_sessions(app: &Wbook) -> Vec<SessionSnapshot> {
        Ok(app.session_manager().list())
    }

    async fn get_session(app: &Wbook, session_id: SessionId) -> SessionSnapshot {
        Ok(app.session_manager().get(session_id)?.snapshot())
    }

    async fn close_session(app: &Wbook, session_id: SessionId) -> ClosedSession {
        Ok(app.session_manager().close(session_id).await?.as_ref().into())
    }

    async fn initialize_session(app: &Wbook, session_id: SessionId) -> OperationResponse<Revision> {
        complete(app.session_manager().get(session_id)?.initialize()).await
    }

    async fn parse_session(app: &Wbook, session_id: SessionId, config: TocParserConfig) -> OperationResponse<ParsedResults> {
        complete(app.session_manager().get(session_id)?.parse(config)).await
    }

    async fn install_results(app: &Wbook, session_id: SessionId, expected: Revision, results: ParsedResults) -> OperationResponse<Revision> {
        complete(app.session_manager().get(session_id)?.install(expected, results)).await
    }

    async fn apply_edits(app: &Wbook, session_id: SessionId, expected: Revision, batch: EditBatch) -> OperationResponse<Revision> {
        complete(app.session_manager().get(session_id)?.apply_edits(expected, batch)).await
    }

    async fn set_metadata_overrides(app: &Wbook, session_id: SessionId, expected: Revision, overrides: Metadata) -> OperationResponse<Revision> {
        complete(app.session_manager().get(session_id)?.set_metadata_overrides(expected, overrides)).await
    }

    async fn read_text(app: &Wbook, session_id: SessionId, version: DocumentVersion, range: TextRange) -> OperationResponse<String> {
        complete(app.session_manager().get(session_id)?.read_text(version, range)).await
    }

    async fn read_results(app: &Wbook, session_id: SessionId) -> OperationResponse<WorkspaceResults> {
        complete(app.session_manager().get(session_id)?.read_results()).await
    }

    async fn render_preview(app: &Wbook, session_id: SessionId, expected: Revision, options: ExportOptions) -> OperationResponse<PreviewInfo> {
        complete(app.session_manager().get(session_id)?.render_preview(expected, options)).await
    }

    async fn export_epub(app: &Wbook, session_id: SessionId, expected: Revision, options: ExportOptions, destination: PathBuf) -> OperationResponse<ExportedBook> {
        complete(app.session_manager().get(session_id)?.export_epub(expected, options, destination)).await
    }

    async fn cancel_operation(app: &Wbook, session_id: SessionId, operation_id: OperationId) -> CancelReply {
        Ok(app.session_manager().get(session_id)?.cancel(operation_id))
    }
}
