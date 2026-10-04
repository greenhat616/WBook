use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use crate::document::{DocumentError, DocumentVersion, PipelineError, ProcessingDocument};

mod epub;
mod plan;
mod render;
mod validate;

pub use render::RenderedBook;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum RenderLayout {
    #[default]
    SplitChapters,
    Paged,
    SingleHtml,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct RenderOptions {
    pub layout: RenderLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum OutputFormat {
    Epub,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ExportOptions {
    pub render: RenderOptions,
    pub format: OutputFormat,
    pub language: String,
    pub identifier: Option<String>,
}

#[derive(Debug)]
pub struct ExportArtifact {
    pub format: OutputFormat,
    pub path: PathBuf,
    pub version: DocumentVersion,
    pub identifier: String,
    pub cleanup_failures: Vec<CleanupFailure>,
}

#[derive(Debug)]
pub struct CleanupFailure {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportStage {
    Planning,
    Rendering,
    Packaging,
    Validating,
    Publishing,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportFailure {
    #[error("export cancelled")]
    Cancelled,
    #[error("invalid export input: {0}")]
    InvalidInput(String),
    #[error("export validation failed: {0}")]
    Validation(String),
    #[error("destination already exists: {0}")]
    TargetExists(PathBuf),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Pipeline(#[from] PipelineError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Template(#[from] tera::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
}

#[derive(Debug, thiserror::Error)]
#[error("export failed during {stage:?}: {source}")]
pub struct ExportError {
    pub stage: ExportStage,
    #[source]
    pub source: ExportFailure,
    pub cleanup_failures: Vec<CleanupFailure>,
}

type Result<T> = std::result::Result<T, ExportFailure>;

fn check(ct: &CancellationToken) -> Result<()> {
    if ct.is_cancelled() {
        Err(ExportFailure::Cancelled)
    } else {
        Ok(())
    }
}

fn stage<T>(
    ct: &CancellationToken,
    stage: ExportStage,
    action: impl FnOnce() -> Result<T>,
) -> std::result::Result<T, ExportError> {
    check(ct)
        .and_then(|()| action())
        .map_err(|source| ExportError {
            stage,
            source: if ct.is_cancelled() {
                ExportFailure::Cancelled
            } else {
                source
            },
            cleanup_failures: Vec::new(),
        })
}

pub fn render_book(
    ct: &CancellationToken,
    document: &ProcessingDocument,
    options: &ExportOptions,
) -> std::result::Result<RenderedBook, ExportError> {
    let plan = stage(ct, ExportStage::Planning, || {
        plan::build(ct, document, options)
    })?;
    stage(ct, ExportStage::Rendering, || {
        render::render(ct, document.view(), plan)
    })
}

pub fn export_epub(
    ct: &CancellationToken,
    document: &ProcessingDocument,
    options: &ExportOptions,
    destination: impl AsRef<Path>,
) -> std::result::Result<ExportArtifact, ExportError> {
    let destination = destination.as_ref();
    stage(ct, ExportStage::Planning, || preflight(destination))?;
    let rendered = render_book(ct, document, options)?;
    let mut result = package_epub(ct, &rendered, destination);
    let path = rendered.directory().to_owned();
    if let Err(error) = rendered.close() {
        let failure = CleanupFailure {
            path,
            message: error.to_string(),
        };
        match &mut result {
            Ok(artifact) => artifact.cleanup_failures.push(failure),
            Err(error) => error.cleanup_failures.push(failure),
        }
    }
    result
}

pub fn package_epub(
    ct: &CancellationToken,
    rendered: &RenderedBook,
    destination: impl AsRef<Path>,
) -> std::result::Result<ExportArtifact, ExportError> {
    let destination = destination.as_ref();
    stage(ct, ExportStage::Packaging, || preflight(destination))?;
    let mut output = stage(ct, ExportStage::Packaging, || {
        Ok(tempfile::NamedTempFile::new_in(parent(destination))?)
    })?;
    let prepared = stage(ct, ExportStage::Packaging, || {
        epub::write(ct, rendered, output.as_file_mut())
    })
    .and_then(|()| {
        stage(ct, ExportStage::Validating, || {
            validate::epub(ct, output.path(), rendered)
        })
    })
    .and_then(|()| stage(ct, ExportStage::Publishing, || check(ct)));
    if let Err(mut error) = prepared {
        cleanup_file(output, &mut error);
        return Err(error);
    }
    if let Err(failure) = output.persist_noclobber(destination) {
        let mut error = ExportError {
            stage: ExportStage::Publishing,
            source: if failure.error.kind() == io::ErrorKind::AlreadyExists {
                ExportFailure::TargetExists(destination.to_owned())
            } else {
                ExportFailure::Io(failure.error)
            },
            cleanup_failures: Vec::new(),
        };
        cleanup_file(failure.file, &mut error);
        return Err(error);
    }
    Ok(ExportArtifact {
        format: OutputFormat::Epub,
        path: destination.to_owned(),
        version: rendered.plan.version,
        identifier: rendered.plan.metadata.identifier.clone(),
        cleanup_failures: Vec::new(),
    })
}

fn cleanup_file(file: tempfile::NamedTempFile, error: &mut ExportError) {
    let path = file.path().to_owned();
    if let Err(failure) = file.close() {
        error.cleanup_failures.push(CleanupFailure {
            path,
            message: failure.to_string(),
        });
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn preflight(path: &Path) -> Result<()> {
    match path.symlink_metadata() {
        Ok(_) => return Err(ExportFailure::TargetExists(path.to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if path.file_name().is_none() || !parent(path).is_dir() {
        return Err(ExportFailure::InvalidInput(format!(
            "invalid destination: {}",
            path.display()
        )));
    }
    Ok(())
}

struct CancelWriter<'a, W> {
    ct: &'a CancellationToken,
    inner: W,
}

impl<W: Write> Write for CancelWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.ct.is_cancelled() {
            return Err(io::Error::other("export cancelled"));
        }
        self.inner.write(&bytes[..bytes.len().min(16 * 1024)])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
