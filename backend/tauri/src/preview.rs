use std::sync::Arc;

use axum::{
    body::Body,
    extract::{rejection::PathRejection, Path, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use snafu::ResultExt;
use tokio_util::io::ReaderStream;
use wbook_core::{session::SessionId, Wbook};

use crate::commands::dto::{check_integers, CommandError, ErrorKind};
use crate::errors::{DecodePathSnafu, PreviewFileSnafu};

const CSP: &str = "default-src 'none'; style-src 'self'; base-uri 'none'; form-action 'none'; sandbox allow-same-origin";

pub fn router(app: Arc<Wbook>) -> Router {
    Router::new()
        .route(
            "/bridge/preview/{session_id}/{preview_id}/{*resource}",
            get(resource),
        )
        .with_state(app)
}

async fn resource(
    State(app): State<Arc<Wbook>>,
    path: Result<Path<(u64, String, String)>, PathRejection>,
) -> Result<Response, CommandError> {
    let Path((session, id, resource)) = path.context(DecodePathSnafu {
        endpoint: "preview resource",
    })?;
    check_integers(&session, ErrorKind::InvalidParams)?;
    if !valid_resource_path(&resource) {
        return Err(CommandError::new(
            ErrorKind::InvalidParams,
            "Invalid preview resource path",
        ));
    }
    let preview = app
        .session_manager()
        .get(SessionId(session))?
        .current_preview()?
        .filter(|preview| preview.id == id)
        .ok_or_else(not_found)?;
    let content_type = if resource == "styles/book.css" {
        "text/css; charset=utf-8"
    } else if resource == "nav.xhtml" || preview.files.contains(&resource) {
        "application/xhtml+xml; charset=utf-8"
    } else {
        return Err(not_found());
    };
    let directory = tokio::fs::canonicalize(&preview.directory)
        .await
        .with_context(|_| PreviewFileSnafu {
            action: "resolve directory for",
            path: preview.directory.clone(),
        })?;
    let requested_path = directory.join(&resource);
    let path = tokio::fs::canonicalize(&requested_path)
        .await
        .with_context(|_| PreviewFileSnafu {
            action: "resolve",
            path: requested_path,
        })?;
    if !path.starts_with(&directory) {
        return Err(not_found());
    }
    // The descriptor does not extend the preview's lifetime. Cleanup may win
    // this race, while a successfully opened resource can finish streaming.
    let file = tokio::fs::File::open(&path)
        .await
        .with_context(|_| PreviewFileSnafu {
            action: "open",
            path: path.clone(),
        })?;
    if !file
        .metadata()
        .await
        .with_context(|_| PreviewFileSnafu {
            action: "read metadata for",
            path,
        })?
        .is_file()
    {
        return Err(not_found());
    }
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CONTENT_SECURITY_POLICY, CSP),
        ],
        Body::from_stream(ReaderStream::new(file)),
    )
        .into_response())
}

fn valid_resource_path(resource: &str) -> bool {
    !resource.contains(['\\', ':', '\0'])
        && resource
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn not_found() -> CommandError {
    CommandError::new(ErrorKind::NotFound, "Preview resource was not found")
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{HeaderMap, Request, StatusCode},
    };
    use tower::ServiceExt;
    use wbook_core::{
        document::{EditBatch, TextEdit},
        export::{ExportOptions, OutputFormat, RenderLayout, RenderOptions},
        parser::toc::TocParserConfig,
        session::SessionHandle,
        types::TextRange,
        workspace::{PreviewInfo, ProcessingOptions, Revision},
        Params,
    };

    use super::*;

    struct Fixture {
        _directory: tempfile::TempDir,
        core: Arc<Wbook>,
        handle: SessionHandle,
        router: Router,
        revision: Revision,
    }

    impl Fixture {
        async fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let path = camino::Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
            let source = path.join("book.txt");
            std::fs::write(&source, "第一章 Start\nBody café\n").unwrap();
            let core = Arc::new(Wbook::new(Params {
                data_dir: path.clone(),
                config_dir: path,
            }));
            let handle = core
                .session_manager()
                .create(
                    source,
                    ProcessingOptions {
                        filters: vec![],
                        toc: TocParserConfig::SplitEvenly { parts: 1 },
                    },
                )
                .unwrap();
            let revision = handle.initialize().unwrap().await.unwrap().outcome.unwrap();
            let router =
                server::local_router(router(core.clone()), "127.0.0.1:1421".parse().unwrap());
            Self {
                _directory: directory,
                core,
                handle,
                router,
                revision,
            }
        }

        async fn preview(&self, language: &str) -> PreviewInfo {
            self.handle
                .render_preview(self.revision, options(language))
                .unwrap()
                .await
                .unwrap()
                .outcome
                .unwrap()
        }

        fn path(&self, preview: &PreviewInfo, resource: &str) -> String {
            format!(
                "/bridge/preview/{}/{}/{resource}",
                self.handle.id().0,
                preview.id
            )
        }

        async fn get(&self, path: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
            let response = self
                .router
                .clone()
                .oneshot(
                    Request::get(path)
                        .header(header::HOST, "127.0.0.1:1421")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            (status, headers, bytes.to_vec())
        }
    }

    fn options(language: &str) -> ExportOptions {
        ExportOptions {
            render: RenderOptions {
                layout: RenderLayout::SingleHtml,
            },
            format: OutputFormat::Epub,
            language: language.into(),
            identifier: Some("urn:wbook:preview-route".into()),
        }
    }

    #[tokio::test]
    async fn serves_only_generated_resources_with_restrictive_headers() {
        let f = Fixture::new().await;
        let preview = f.preview("en").await;
        for resource in [&preview.files[0], "nav.xhtml", "styles/book.css"] {
            let (status, headers, bytes) = f.get(&f.path(&preview, resource)).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[header::CACHE_CONTROL], "no-store");
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert_eq!(headers[header::CONTENT_SECURITY_POLICY], CSP);
            assert_eq!(
                headers[header::CONTENT_TYPE],
                if resource.ends_with(".css") {
                    "text/css; charset=utf-8"
                } else {
                    "application/xhtml+xml; charset=utf-8"
                }
            );
            assert_eq!(
                bytes,
                std::fs::read(preview.directory.join(resource)).unwrap()
            );
        }
        std::fs::write(preview.directory.join("private.txt"), "private").unwrap();
        for resource in ["private.txt", "missing.xhtml", "text/missing.xhtml"] {
            assert_eq!(
                f.get(&f.path(&preview, resource)).await.0,
                StatusCode::NOT_FOUND
            );
        }
        for resource in [
            "../book.txt",
            "%2e%2e/book.txt",
            "text/../../book.txt",
            "text%5cbook.xhtml",
            "C:%5cbook.txt",
            "text//book.xhtml",
            "text/./book.xhtml",
            "text/%00.xhtml",
        ] {
            assert_eq!(
                f.get(&f.path(&preview, resource)).await.0,
                StatusCode::BAD_REQUEST,
                "{resource}"
            );
        }
        assert_eq!(
            f.get("/bridge/preview/no-session/unknown/nav.xhtml")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.get("/bridge/preview/9007199254740992/unknown/nav.xhtml")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.get("/bridge/preview/999/unknown/nav.xhtml").await.0,
            StatusCode::NOT_FOUND
        );
        f.core.shutdown().await;
    }

    #[tokio::test]
    async fn replacement_edit_and_close_invalidate_urls_without_reviving_old_ids() {
        let f = Fixture::new().await;
        let first = f.preview("en").await;
        let first_url = f.path(&first, &first.files[0]);
        assert_eq!(f.preview("en").await.id, first.id);
        let second = f.preview("fr").await;
        assert_eq!(second.revision, first.revision);
        assert_ne!(second.id, first.id);
        assert_eq!(f.get(&first_url).await.0, StatusCode::NOT_FOUND);
        let third = f.preview("en").await;
        assert_ne!(third.id, first.id);
        assert_eq!(f.get(&first_url).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            f.get(&f.path(&second, &second.files[0])).await.0,
            StatusCode::NOT_FOUND
        );
        assert!(f
            .handle
            .render_preview(f.revision, options("bad_language"))
            .unwrap()
            .await
            .unwrap()
            .outcome
            .is_err());
        let third_url = f.path(&third, &third.files[0]);
        assert_eq!(f.get(&third_url).await.0, StatusCode::OK);
        let version = match f.handle.snapshot().workspace_status {
            wbook_core::session::Availability::Available(status) => {
                status.document_version.unwrap()
            }
            _ => unreachable!(),
        };
        f.handle
            .apply_edits(
                f.revision,
                EditBatch {
                    base: version,
                    edits: vec![TextEdit {
                        range: TextRange { start: 0, end: 0 },
                        insert: "Preface\n".into(),
                    }],
                },
            )
            .unwrap()
            .await
            .unwrap()
            .outcome
            .unwrap();
        assert_eq!(f.handle.current_preview().unwrap(), None);
        assert_eq!(f.get(&third_url).await.0, StatusCode::NOT_FOUND);
        f.handle.close().await;
        assert_eq!(f.get(&third_url).await.0, StatusCode::NOT_FOUND);
        f.core.shutdown().await;
    }

    #[tokio::test]
    async fn closing_a_session_revokes_its_active_preview() {
        let f = Fixture::new().await;
        let preview = f.preview("en").await;
        let url = f.path(&preview, &preview.files[0]);
        assert_eq!(f.get(&url).await.0, StatusCode::OK);
        f.handle.close().await;
        assert_eq!(f.get(&url).await.0, StatusCode::NOT_FOUND);
        assert!(!preview.directory.exists());
        f.core.shutdown().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_an_allowed_resource_symlinked_outside_the_preview() {
        let f = Fixture::new().await;
        let preview = f.preview("en").await;
        let resource = &preview.files[0];
        let target = f._directory.path().join("outside.xhtml");
        std::fs::write(&target, "outside").unwrap();
        let path = preview.directory.join(resource);
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&target, path).unwrap();
        assert_eq!(
            f.get(&f.path(&preview, resource)).await.0,
            StatusCode::NOT_FOUND
        );
        f.core.shutdown().await;
    }
}
