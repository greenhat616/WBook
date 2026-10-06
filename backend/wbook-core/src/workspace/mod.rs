use std::path::{Path, PathBuf};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::document::{
    check_cancelled, ChangeMap, DocumentError, DocumentVersion, EditBatch, ParsedResults,
    PipelineError, ProcessingDocument,
};
use crate::export::{
    self, CleanupFailure, ExportError, ExportFailure, ExportOptions, RenderedBook,
};
use crate::extractor::{Extractor, ExtractorError, ParsedContent, ProcessOptions, SimpleExtractor};
use crate::parser::toc::TocParserConfig;
use crate::parser::{
    AdFilterParser, FilterParser, Metadata, MetadataParser, ParserError, SimpleMetadataParser,
    TocParser,
};
use crate::settings::{Settings, SettingsError};
use crate::types::TextRange;

const READ_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct WorkspaceId([u8; 16]);

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
pub struct Revision(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum FilterConfig {
    Ad,
}

pub struct Workspace {
    state: WorkspaceState,
    preview: Option<Preview>,
    warnings: Vec<CleanupFailure>,
}

// Saving will require a versioned DTO rather than serializing this memory layout.
struct WorkspaceState {
    id: WorkspaceId,
    revision: Revision,
    source: Utf8PathBuf,
    settings: Settings,
    document: Option<ProcessingDocument>,
    filters_applied: usize,
}

struct Preview {
    id: String,
    options: ExportOptions,
    book: RenderedBook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DocumentStatus {
    Absent,
    Unparsed,
    Current,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FilterProgress {
    pub applied: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct WorkspaceStatus {
    pub revision: Revision,
    pub document: DocumentStatus,
    pub document_version: Option<DocumentVersion>,
    pub filters: FilterProgress,
    pub has_overrides: bool,
    pub preview: Option<ExportOptions>,
    pub preview_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkspaceResults {
    pub results: Option<ParsedResults>,
    pub current: bool,
    pub overrides: Metadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct PreviewInfo {
    pub id: String,
    pub revision: Revision,
    pub options: ExportOptions,
    pub directory: PathBuf,
    pub files: Vec<String>,
}

#[derive(Debug)]
pub struct ExportArtifact {
    pub revision: Revision,
    pub artifact: export::ExportArtifact,
}

pub struct OpContext<'a> {
    pub ct: &'a CancellationToken,
    pub report: &'a (dyn Fn(Phase) + Sync),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum Phase {
    Extracting,
    Filtering { index: usize, total: usize },
    Parsing,
    Installing,
    Editing,
    Reading,
    Rendering,
    Exporting,
}

#[derive(Debug, snafu::Snafu)]
pub enum WorkspaceError {
    #[snafu(context(false), display("{source}"))]
    InvalidConfig { source: SettingsError },
    #[snafu(display("stale workspace revision: expected {expected:?}, actual {actual:?}"))]
    StaleRevision {
        expected: Revision,
        actual: Revision,
    },
    #[snafu(display("no document has been extracted"))]
    NoDocument,
    #[snafu(display("results have already been installed"))]
    AlreadyInitialized,
    #[snafu(display("parsed results are not current"))]
    ResultsNotCurrent,
    #[snafu(display("read size {requested} exceeds the limit of {limit} bytes"))]
    ReadTooLarge { requested: u64, limit: u64 },
    #[snafu(context(false), display("{source}"))]
    Extractor { source: ExtractorError },
    #[snafu(context(false), display("{source}"))]
    Document { source: DocumentError },
    #[snafu(context(false), display("{source}"))]
    Pipeline { source: PipelineError },
    #[snafu(context(false), display("{source}"))]
    Export { source: ExportError },
}

impl WorkspaceError {
    pub fn is_cancelled(&self) -> bool {
        fn pipeline(error: &PipelineError) -> bool {
            matches!(
                error,
                PipelineError::Document {
                    source: DocumentError::Cancelled
                } | PipelineError::Parser {
                    source: ParserError::Cancelled
                }
            )
        }
        match self {
            Self::Extractor {
                source: ExtractorError::Shutdown,
            }
            | Self::Document {
                source: DocumentError::Cancelled,
            } => true,
            Self::Pipeline { source: error } => pipeline(error),
            Self::Export { source: error } => match &error.source {
                ExportFailure::Cancelled
                | ExportFailure::Document {
                    source: DocumentError::Cancelled,
                } => true,
                ExportFailure::Pipeline { source: error } => pipeline(error),
                _ => false,
            },
            _ => false,
        }
    }
}

impl Workspace {
    pub fn new(source: Utf8PathBuf, settings: Settings) -> Result<Self, WorkspaceError> {
        settings.validate()?;
        Ok(Self {
            state: WorkspaceState {
                id: WorkspaceId(*Uuid::new_v4().as_bytes()),
                revision: Revision::default(),
                source,
                settings,
                document: None,
                filters_applied: 0,
            },
            preview: None,
            warnings: Vec::new(),
        })
    }

    pub fn id(&self) -> WorkspaceId {
        self.state.id
    }

    pub fn source(&self) -> &Utf8Path {
        &self.state.source
    }

    pub fn revision(&self) -> Revision {
        self.state.revision
    }

    pub fn status(&self) -> WorkspaceStatus {
        let document = self.state.document.as_ref();
        WorkspaceStatus {
            revision: self.revision(),
            document: match document {
                None => DocumentStatus::Absent,
                Some(doc) => match doc.results() {
                    None => DocumentStatus::Unparsed,
                    Some(results) if results.version == doc.view().version() => {
                        DocumentStatus::Current
                    }
                    Some(_) => DocumentStatus::Stale,
                },
            },
            document_version: document.map(|doc| doc.view().version()),
            filters: FilterProgress {
                applied: self.state.filters_applied,
                total: self.state.settings.filters.len(),
            },
            has_overrides: document
                .is_some_and(|doc| doc.metadata_overrides != Metadata::default()),
            preview: self.preview.as_ref().map(|preview| preview.options.clone()),
            preview_id: self.preview.as_ref().map(|preview| preview.id.clone()),
        }
    }

    // The document APIs validate before publication; an error or no-op must leave
    // persistent state untouched so its revision and preview remain valid.
    fn commit<T>(
        &mut self,
        action: impl FnOnce(&mut WorkspaceState) -> Result<(T, bool), WorkspaceError>,
    ) -> Result<T, WorkspaceError> {
        let (value, changed) = action(&mut self.state)?;
        if changed {
            self.state.revision.0 += 1;
            self.close_preview();
        }
        Ok(value)
    }

    fn close_preview(&mut self) {
        if let Some(preview) = self.preview.take() {
            let path = preview.book.directory().to_owned();
            if let Err(error) = preview.book.close() {
                self.warnings.push(CleanupFailure {
                    path,
                    message: error.to_string(),
                });
            }
        }
    }

    fn check_revision(&self, expected: Revision) -> Result<(), WorkspaceError> {
        let actual = self.revision();
        if actual != expected {
            return Err(WorkspaceError::StaleRevision { expected, actual });
        }
        Ok(())
    }

    fn document(&self) -> Result<&ProcessingDocument, WorkspaceError> {
        self.state
            .document
            .as_ref()
            .ok_or(WorkspaceError::NoDocument)
    }

    fn current_document(&self) -> Result<&ProcessingDocument, WorkspaceError> {
        let document = self.document()?;
        if !document
            .results()
            .is_some_and(|results| results.version == document.view().version())
        {
            return Err(WorkspaceError::ResultsNotCurrent);
        }
        Ok(document)
    }

    fn extract(&mut self, cx: &OpContext<'_>) -> Result<(), WorkspaceError> {
        if self
            .state
            .document
            .as_ref()
            .is_some_and(|doc| doc.results().is_some())
        {
            return Err(WorkspaceError::AlreadyInitialized);
        }
        if self.state.document.is_none() {
            (cx.report)(Phase::Extracting);
            let content = SimpleExtractor.process(cx.ct, &ProcessOptions {}, &self.state.source)?;
            self.take_over(cx, content)?;
        }
        Ok(())
    }

    fn take_over(
        &mut self,
        cx: &OpContext<'_>,
        content: ParsedContent,
    ) -> Result<(), WorkspaceError> {
        check_cancelled(cx.ct)?;
        self.commit(|state| {
            state.document = Some(ProcessingDocument::new(content.into()));
            Ok(((), true))
        })
    }

    pub fn initialize(&mut self, cx: &OpContext<'_>) -> Result<Revision, WorkspaceError> {
        let toc = self
            .state
            .settings
            .toc
            .to_config()
            .and_then(|config| config.build())
            .map_err(SettingsError::from)?;
        let ad = AdFilterParser;
        let filters: Vec<&dyn FilterParser> = self
            .state
            .settings
            .filters
            .iter()
            .map(|config| match config {
                FilterConfig::Ad => &ad as &dyn FilterParser,
            })
            .collect();
        self.extract(cx)?;
        self.initialize_parsers(cx, &filters, toc.as_ref(), &SimpleMetadataParser)
    }

    #[cfg(test)]
    pub(crate) fn initialize_with(
        &mut self,
        cx: &OpContext<'_>,
        filters: &[&dyn FilterParser],
        toc: &dyn TocParser,
        metadata: &dyn MetadataParser,
    ) -> Result<Revision, WorkspaceError> {
        assert_eq!(filters.len(), self.state.settings.filters.len());
        self.extract(cx)?;
        self.initialize_parsers(cx, filters, toc, metadata)
    }

    fn initialize_parsers(
        &mut self,
        cx: &OpContext<'_>,
        filters: &[&dyn FilterParser],
        toc: &dyn TocParser,
        metadata: &dyn MetadataParser,
    ) -> Result<Revision, WorkspaceError> {
        for (index, filter) in filters.iter().enumerate().skip(self.state.filters_applied) {
            (cx.report)(Phase::Filtering {
                index,
                total: filters.len(),
            });
            check_cancelled(cx.ct)?;
            let view = self.document()?.view();
            let batch = EditBatch {
                base: view.version(),
                edits: filter.parse(cx.ct, view).map_err(PipelineError::from)?,
            };
            self.commit(|state| {
                state
                    .document
                    .as_mut()
                    .ok_or(WorkspaceError::NoDocument)?
                    .apply(cx.ct, batch)?;
                state.filters_applied += 1;
                Ok(((), true))
            })?;
        }
        (cx.report)(Phase::Parsing);
        let results = self.document()?.parse(cx.ct, toc, metadata)?;
        self.install(cx, self.revision(), results)
    }

    pub fn parse(
        &self,
        cx: &OpContext<'_>,
        config: TocParserConfig,
    ) -> Result<ParsedResults, WorkspaceError> {
        let document = self.document()?;
        let toc = config.build().map_err(SettingsError::from)?;
        (cx.report)(Phase::Parsing);
        Ok(document.parse(cx.ct, toc.as_ref(), &SimpleMetadataParser)?)
    }

    pub fn install(
        &mut self,
        cx: &OpContext<'_>,
        expected: Revision,
        results: ParsedResults,
    ) -> Result<Revision, WorkspaceError> {
        self.check_revision(expected)?;
        (cx.report)(Phase::Installing);
        self.commit(|state| {
            state
                .document
                .as_mut()
                .ok_or(WorkspaceError::NoDocument)?
                .install(cx.ct, results)?;
            Ok(((), true))
        })?;
        Ok(self.revision())
    }

    pub fn apply_edits(
        &mut self,
        cx: &OpContext<'_>,
        expected: Revision,
        batch: EditBatch,
    ) -> Result<(Revision, ChangeMap), WorkspaceError> {
        self.check_revision(expected)?;
        (cx.report)(Phase::Editing);
        let changes = self.commit(|state| {
            let changes = state
                .document
                .as_mut()
                .ok_or(WorkspaceError::NoDocument)?
                .apply(cx.ct, batch)?;
            let changed = changes.before() != changes.after();
            Ok((changes, changed))
        })?;
        Ok((self.revision(), changes))
    }

    pub fn set_metadata_overrides(
        &mut self,
        cx: &OpContext<'_>,
        expected: Revision,
        overrides: Metadata,
    ) -> Result<Revision, WorkspaceError> {
        self.check_revision(expected)?;
        (cx.report)(Phase::Editing);
        check_cancelled(cx.ct)?;
        self.commit(|state| {
            let document = state.document.as_mut().ok_or(WorkspaceError::NoDocument)?;
            let changed = document.metadata_overrides != overrides;
            if changed {
                document.metadata_overrides = overrides;
            }
            Ok(((), changed))
        })?;
        Ok(self.revision())
    }

    pub fn settings(&self) -> &Settings {
        &self.state.settings
    }

    /// Filters already ran during initialization, so a filter change only
    /// updates the recorded settings.
    pub fn set_settings(
        &mut self,
        cx: &OpContext<'_>,
        expected: Revision,
        settings: Settings,
    ) -> Result<Revision, WorkspaceError> {
        self.check_revision(expected)?;
        (cx.report)(Phase::Editing);
        settings.validate()?;
        check_cancelled(cx.ct)?;
        self.commit(|state| {
            let changed = state.settings != settings;
            if changed {
                state.settings = settings;
            }
            Ok(((), changed))
        })?;
        Ok(self.revision())
    }

    pub fn read_text(
        &self,
        cx: &OpContext<'_>,
        version: DocumentVersion,
        range: TextRange,
    ) -> Result<String, WorkspaceError> {
        (cx.report)(Phase::Reading);
        check_cancelled(cx.ct)?;
        let view = self.document()?.view();
        view.chunks(version, range)?;
        let requested = range.end - range.start;
        if requested > READ_LIMIT {
            return Err(WorkspaceError::ReadTooLarge {
                requested,
                limit: READ_LIMIT,
            });
        }
        Ok(view.read(cx.ct, version, range)?.into_owned())
    }

    pub fn results(&self, cx: &OpContext<'_>) -> Result<WorkspaceResults, WorkspaceError> {
        (cx.report)(Phase::Reading);
        check_cancelled(cx.ct)?;
        let document = self.document()?;
        Ok(WorkspaceResults {
            results: document.results().cloned(),
            current: document
                .results()
                .is_some_and(|results| results.version == document.view().version()),
            overrides: document.metadata_overrides.clone(),
        })
    }

    fn preview_info(&self, preview: &Preview) -> PreviewInfo {
        PreviewInfo {
            id: preview.id.clone(),
            revision: self.revision(),
            options: preview.options.clone(),
            directory: preview.book.directory().to_owned(),
            files: preview.book.content_files().to_vec(),
        }
    }

    pub fn current_preview(&self) -> Option<PreviewInfo> {
        self.preview
            .as_ref()
            .map(|preview| self.preview_info(preview))
    }

    pub fn render_preview(
        &mut self,
        cx: &OpContext<'_>,
        expected: Revision,
        options: ExportOptions,
    ) -> Result<PreviewInfo, WorkspaceError> {
        self.check_revision(expected)?;
        (cx.report)(Phase::Rendering);
        check_cancelled(cx.ct)?;
        let document = self.current_document()?;
        if let Some(preview) = &self.preview {
            if preview.options == options {
                return Ok(self.preview_info(preview));
            }
        }
        let book = export::render_book(cx.ct, document, &options)?;
        self.close_preview();
        let preview = Preview {
            id: Uuid::new_v4().to_string(),
            options,
            book,
        };
        let info = self.preview_info(&preview);
        self.preview = Some(preview);
        Ok(info)
    }

    pub fn export_epub(
        &self,
        cx: &OpContext<'_>,
        expected: Revision,
        options: ExportOptions,
        destination: impl AsRef<Path>,
    ) -> Result<ExportArtifact, WorkspaceError> {
        self.check_revision(expected)?;
        (cx.report)(Phase::Exporting);
        let document = self.current_document()?;
        let artifact = export::export_epub(cx.ct, document, &options, destination)?;
        Ok(ExportArtifact {
            revision: self.revision(),
            artifact,
        })
    }

    pub fn take_warnings(&mut self) -> Vec<CleanupFailure> {
        std::mem::take(&mut self.warnings)
    }

    pub fn close(mut self) -> Vec<CleanupFailure> {
        self.close_preview();
        self.take_warnings()
    }
}

#[cfg(test)]
mod tests;
