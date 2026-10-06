use std::ops::Range;

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::extractor::{Content, Encoding, ParsedContent};
use crate::types::TextRange;

mod change;
mod pipeline;
mod view;

pub use change::{Affinity, ChangeMap, TextPosition};
pub use pipeline::{OutputPlan, ParsedResults, PipelineError, ProcessingDocument};
pub use view::{LogicalLine, TextView};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DocumentVersion {
    document_id: [u8; 16],
    revision: u64,
}

impl DocumentVersion {
    pub fn revision(self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TextEdit {
    pub range: TextRange,
    pub insert: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EditBatch {
    pub base: DocumentVersion,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, snafu::Snafu)]
pub enum DocumentError {
    #[snafu(display("document operation cancelled"))]
    Cancelled,
    #[snafu(display("the version belongs to another document"))]
    WrongDocument,
    #[snafu(display("stale document revision: expected {expected}, got {actual}"))]
    StaleVersion { expected: u64, actual: u64 },
    #[snafu(display("inverted range {range:?} at edit {index:?}"))]
    Inverted {
        index: Option<usize>,
        range: TextRange,
    },
    #[snafu(display("range {range:?} exceeds document length {len} at edit {index:?}"))]
    OutOfBounds {
        index: Option<usize>,
        range: TextRange,
        len: u64,
    },
    #[snafu(display("offset {offset} is not a UTF-8 boundary at edit {index:?}"))]
    InvalidBoundary { index: Option<usize>, offset: u64 },
    #[snafu(display("edits {first} and {second} conflict"))]
    Conflict { first: usize, second: usize },
    #[snafu(display("document length or revision cannot be represented"))]
    Overflow,
    #[snafu(context(false), display("{source}"))]
    Io { source: std::io::Error },
}

pub(crate) fn check_cancelled(ct: &CancellationToken) -> Result<(), DocumentError> {
    if ct.is_cancelled() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}

impl DocumentVersion {
    pub(crate) fn check(self, actual: Self) -> Result<(), DocumentError> {
        if self.document_id != actual.document_id {
            return Err(DocumentError::WrongDocument);
        }
        if self.revision != actual.revision {
            return Err(DocumentError::StaleVersion {
                expected: self.revision,
                actual: actual.revision,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Piece {
    buffer: usize,
    range: Range<usize>,
}

#[derive(Debug)]
pub struct TextDocument {
    buffers: Vec<String>,
    pieces: Vec<Piece>,
    len: u64,
    version: DocumentVersion,
    encoding: Encoding,
    source_path: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DocumentStats {
    pub original_bytes: usize,
    pub added_bytes: usize,
    pub pieces: usize,
}

impl From<ParsedContent> for TextDocument {
    fn from(content: ParsedContent) -> Self {
        let document_id = *Uuid::new_v4().as_bytes();
        let Content::Text(text) = content.content;
        let len = text.len();
        Self {
            buffers: vec![text],
            pieces: if len == 0 {
                vec![]
            } else {
                vec![Piece {
                    buffer: 0,
                    range: 0..len,
                }]
            },
            len: len as u64,
            version: DocumentVersion {
                document_id,
                revision: 0,
            },
            encoding: content.encoding,
            source_path: content.source_path,
        }
    }
}

impl TextDocument {
    pub fn view(&self) -> TextView<'_> {
        TextView { document: self }
    }

    pub fn version(&self) -> DocumentVersion {
        self.version
    }

    pub fn stats(&self) -> DocumentStats {
        DocumentStats {
            original_bytes: self.buffers[0].len(),
            added_bytes: self.buffers[1..].iter().map(String::len).sum(),
            pieces: self.pieces.len(),
        }
    }

    pub fn apply(
        &mut self,
        ct: &CancellationToken,
        batch: EditBatch,
    ) -> Result<ChangeMap, DocumentError> {
        self.apply_checked(batch, || check_cancelled(ct))
    }

    // All fallible work precedes publication, including cancellation checks.
    // The closure also gives tests a deterministic cancellation boundary.
    fn apply_checked(
        &mut self,
        batch: EditBatch,
        mut check: impl FnMut() -> Result<(), DocumentError>,
    ) -> Result<ChangeMap, DocumentError> {
        check()?;
        self.version.check(batch.base)?;
        let mut edits = Vec::with_capacity(batch.edits.len());
        for (index, edit) in batch.edits.into_iter().enumerate() {
            check()?;
            validate_range(edit.range, self.len, Some(index))?;
            if !edit.range.is_empty() || !edit.insert.is_empty() {
                edits.push((index, edit));
            } else {
                self.view()
                    .validate_boundary(edit.range.start, Some(index))?;
            }
        }
        edits.sort_by_key(|(_, edit)| (edit.range.start, edit.range.end));
        if edits.is_empty() {
            check()?;
            return Ok(ChangeMap {
                before: self.version,
                after: self.version,
                old_len: self.len,
                changes: vec![],
            });
        }
        let mut old_end = 0;
        let mut new_end = 0u64;
        let mut changes = Vec::with_capacity(edits.len());
        for (position, (index, edit)) in edits.iter().enumerate() {
            check()?;
            if let Some((previous_index, previous)) = position.checked_sub(1).map(|i| &edits[i]) {
                if previous.range.end > edit.range.start
                    || (previous.range.end == edit.range.start
                        && (previous.range.is_empty() || edit.range.is_empty()))
                {
                    return Err(DocumentError::Conflict {
                        first: *previous_index,
                        second: *index,
                    });
                }
            }
            let start = new_end
                .checked_add(edit.range.start - old_end)
                .ok_or(DocumentError::Overflow)?;
            new_end = start
                .checked_add(edit.insert.len() as u64)
                .ok_or(DocumentError::Overflow)?;
            changes.push(change::Change {
                old: edit.range,
                new: TextRange {
                    start,
                    end: new_end,
                },
            });
            old_end = edit.range.end;
        }
        let new_len = new_end
            .checked_add(self.len - old_end)
            .ok_or(DocumentError::Overflow)?;
        usize::try_from(new_len).map_err(|_| DocumentError::Overflow)?;
        let mut new_version = self.version;
        new_version.revision = new_version
            .revision
            .checked_add(1)
            .ok_or(DocumentError::Overflow)?;
        let mut pieces = Vec::with_capacity(self.pieces.len() + edits.len());
        let mut added = Vec::new();
        let mut cursor = PieceCursor {
            document: self,
            piece: 0,
            local: 0,
            offset: 0,
        };
        for (index, edit) in edits {
            check()?;
            cursor.advance(edit.range.start, Some(index), Some(&mut pieces), &mut check)?;
            cursor.advance(edit.range.end, Some(index), None, &mut check)?;
            if !edit.insert.is_empty() {
                push_piece(
                    &mut pieces,
                    Piece {
                        buffer: self.buffers.len() + added.len(),
                        range: 0..edit.insert.len(),
                    },
                );
                added.push(edit.insert);
            }
        }
        cursor.advance(self.len, None, Some(&mut pieces), &mut check)?;
        let mapping = ChangeMap {
            before: self.version,
            after: new_version,
            old_len: self.len,
            changes,
        };
        check()?;
        self.buffers.extend(added);
        self.pieces = pieces;
        self.len = new_len;
        self.version = new_version;
        Ok(mapping)
    }
}

fn validate_range(range: TextRange, len: u64, index: Option<usize>) -> Result<(), DocumentError> {
    if range.start > range.end {
        return Err(DocumentError::Inverted { index, range });
    }
    if range.end > len {
        return Err(DocumentError::OutOfBounds { index, range, len });
    }
    Ok(())
}

fn push_piece(pieces: &mut Vec<Piece>, piece: Piece) {
    if let Some(last) = pieces.last_mut() {
        if last.buffer == piece.buffer && last.range.end == piece.range.start {
            last.range.end = piece.range.end;
            return;
        }
    }
    pieces.push(piece);
}

struct PieceCursor<'a> {
    document: &'a TextDocument,
    piece: usize,
    local: usize,
    offset: u64,
}

impl PieceCursor<'_> {
    fn advance(
        &mut self,
        end: u64,
        index: Option<usize>,
        mut output: Option<&mut Vec<Piece>>,
        check: &mut impl FnMut() -> Result<(), DocumentError>,
    ) -> Result<(), DocumentError> {
        while self.offset < end {
            check()?;
            let piece = &self.document.pieces[self.piece];
            let start = piece.range.start + self.local;
            let count = (end - self.offset).min((piece.range.end - start) as u64) as usize;
            let next = start + count;
            if !self.document.buffers[piece.buffer].is_char_boundary(next) {
                return Err(DocumentError::InvalidBoundary {
                    index,
                    offset: self.offset + count as u64,
                });
            }
            if let Some(out) = output.as_deref_mut() {
                push_piece(
                    out,
                    Piece {
                        buffer: piece.buffer,
                        range: start..next,
                    },
                );
            }
            self.offset += count as u64;
            self.local += count;
            if next == piece.range.end {
                self.piece += 1;
                self.local = 0;
            }
        }
        Ok(())
    }
}
