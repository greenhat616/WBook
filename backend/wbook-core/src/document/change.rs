use super::{validate_range, DocumentError, DocumentVersion};
use crate::types::TextRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Affinity {
    Before,
    After,
}

// Construct positions through a view so mappings never need an old text copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub(super) version: DocumentVersion,
    pub(super) offset: u64,
}

impl TextPosition {
    pub fn offset(self) -> u64 {
        self.offset
    }
    pub fn version(self) -> DocumentVersion {
        self.version
    }
}

#[derive(Debug)]
pub(super) struct Change {
    pub old: TextRange,
    pub new: TextRange,
}

#[derive(Debug)]
pub struct ChangeMap {
    pub(super) before: DocumentVersion,
    pub(super) after: DocumentVersion,
    pub(super) old_len: u64,
    pub(super) changes: Vec<Change>,
}

impl ChangeMap {
    pub fn before(&self) -> DocumentVersion {
        self.before
    }
    pub fn after(&self) -> DocumentVersion {
        self.after
    }

    pub fn map(
        &self,
        position: TextPosition,
        affinity: Affinity,
    ) -> Result<Option<TextPosition>, DocumentError> {
        self.before.check(position.version)?;
        let offset = position.offset;
        validate_range(
            TextRange {
                start: offset,
                end: offset,
            },
            self.old_len,
            None,
        )?;
        let mut old_end = 0;
        let mut new_end = 0;
        for change in &self.changes {
            if offset < change.old.start {
                break;
            }
            if change.old.is_empty() && offset == change.old.start {
                return Ok(Some(TextPosition {
                    version: self.after,
                    offset: match affinity {
                        Affinity::Before => change.new.start,
                        Affinity::After => change.new.end,
                    },
                }));
            }
            if offset == change.old.start {
                return Ok(Some(TextPosition {
                    version: self.after,
                    offset: change.new.start,
                }));
            }
            if offset < change.old.end {
                return Ok(None);
            }
            old_end = change.old.end;
            new_end = change.new.end;
        }
        Ok(Some(TextPosition {
            version: self.after,
            offset: new_end + (offset - old_end),
        }))
    }

    pub fn touches(
        &self,
        version: DocumentVersion,
        range: TextRange,
    ) -> Result<bool, DocumentError> {
        self.before.check(version)?;
        validate_range(range, self.old_len, None)?;
        Ok(self.changes.iter().any(|change| {
            if change.old.is_empty() {
                range.start <= change.old.start && change.old.start <= range.end
            } else {
                change.old.start < range.end && range.start < change.old.end
            }
        }))
    }
}
