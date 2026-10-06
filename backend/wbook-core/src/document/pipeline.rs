use std::io::Write;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

use super::{
    check_cancelled, ChangeMap, DocumentError, DocumentVersion, EditBatch, TextDocument, TextView,
};
use crate::parser::{FilterParser, Metadata, MetadataParser, ParserError, TocParser};
use crate::toc::TocRoot;
use crate::types::TextRange;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ParsedResults {
    pub version: DocumentVersion,
    pub toc: TocRoot,
    pub metadata: Metadata,
}

#[derive(Debug, snafu::Snafu)]
pub enum PipelineError {
    #[snafu(context(false), display("{source}"))]
    Document { source: DocumentError },
    #[snafu(context(false), display("{source}"))]
    Parser { source: ParserError },
    #[snafu(display("no parsed results have been installed"))]
    MissingResults,
}

#[derive(Debug)]
pub struct OutputPlan {
    version: DocumentVersion,
    ranges: Vec<TextRange>,
}

pub struct ProcessingDocument {
    document: TextDocument,
    results: Option<ParsedResults>,
    pub metadata_overrides: Metadata,
}

impl ProcessingDocument {
    pub fn new(document: TextDocument) -> Self {
        Self {
            document,
            results: None,
            metadata_overrides: Metadata::default(),
        }
    }

    pub fn view(&self) -> TextView<'_> {
        self.document.view()
    }

    // Stale results remain available so reparsing never discards manual edits.
    pub fn results(&self) -> Option<&ParsedResults> {
        self.results.as_ref()
    }

    pub fn current_results(&self) -> Result<&ParsedResults, PipelineError> {
        let results = self.results.as_ref().ok_or(PipelineError::MissingResults)?;
        self.document.version().check(results.version)?;
        Ok(results)
    }

    pub fn metadata(&self) -> Result<Metadata, PipelineError> {
        let automatic = &self.current_results()?.metadata;
        Ok(Metadata {
            title: self
                .metadata_overrides
                .title
                .clone()
                .or_else(|| automatic.title.clone()),
            author: self
                .metadata_overrides
                .author
                .clone()
                .or_else(|| automatic.author.clone()),
        })
    }

    pub fn apply(
        &mut self,
        ct: &CancellationToken,
        batch: EditBatch,
    ) -> Result<ChangeMap, DocumentError> {
        self.document.apply(ct, batch)
    }

    pub fn run(
        &mut self,
        ct: &CancellationToken,
        filters: &[&dyn FilterParser],
        toc: &dyn TocParser,
        metadata: &dyn MetadataParser,
    ) -> Result<ParsedResults, PipelineError> {
        check_cancelled(ct)?;
        for filter in filters {
            check_cancelled(ct)?;
            let view = self.view();
            let batch = EditBatch {
                base: view.version(),
                edits: filter.parse(ct, view)?,
            };
            self.apply(ct, batch)?;
        }
        self.parse(ct, toc, metadata)
    }

    pub fn parse(
        &self,
        ct: &CancellationToken,
        toc: &dyn TocParser,
        metadata: &dyn MetadataParser,
    ) -> Result<ParsedResults, PipelineError> {
        check_cancelled(ct)?;
        let view = self.view();
        let toc = toc.parse(ct, view)?;
        check_cancelled(ct)?;
        let metadata = metadata.parse(ct, view)?;
        check_cancelled(ct)?;
        Ok(ParsedResults {
            version: view.version(),
            toc,
            metadata,
        })
    }

    pub fn install(
        &mut self,
        ct: &CancellationToken,
        results: ParsedResults,
    ) -> Result<(), PipelineError> {
        check_cancelled(ct)?;
        self.document.version().check(results.version)?;
        for node in results.toc.nodes() {
            check_cancelled(ct)?;
            if let Some(range) = node.meta.range {
                self.view().chunks(results.version, range)?;
            }
        }
        check_cancelled(ct)?;
        self.results = Some(results);
        Ok(())
    }

    // Body ranges are supplied explicitly: heading and container spans are not
    // chapter bodies, and TOC display order does not reorder the source buffer.
    pub fn output_plan(
        &self,
        ct: &CancellationToken,
        ranges: Vec<TextRange>,
    ) -> Result<OutputPlan, PipelineError> {
        check_cancelled(ct)?;
        let version = self.current_results()?.version;
        for range in &ranges {
            check_cancelled(ct)?;
            self.view().chunks(version, *range)?;
        }
        check_cancelled(ct)?;
        Ok(OutputPlan { version, ranges })
    }

    pub fn write_plan(
        &self,
        ct: &CancellationToken,
        plan: &OutputPlan,
        writer: &mut impl Write,
    ) -> Result<(), PipelineError> {
        check_cancelled(ct)?;
        self.current_results()?;
        self.document.version().check(plan.version)?;
        for range in &plan.ranges {
            self.view().write_range(ct, plan.version, *range, writer)?;
        }
        check_cancelled(ct)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
