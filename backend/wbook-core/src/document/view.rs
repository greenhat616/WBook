use std::borrow::Cow;
use std::io::Write;

use camino::Utf8Path;
use tokio_util::sync::CancellationToken;

use super::{
    check_cancelled, validate_range, DocumentError, DocumentVersion, Encoding, TextDocument,
    TextPosition,
};
use crate::types::TextRange;

const CHECK_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct TextView<'a> {
    pub(super) document: &'a TextDocument,
}

impl<'a> TextView<'a> {
    pub fn version(self) -> DocumentVersion {
        self.document.version
    }
    pub fn len(self) -> u64 {
        self.document.len
    }
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
    pub fn encoding(self) -> &'a Encoding {
        &self.document.encoding
    }
    pub fn source_path(self) -> Option<&'a Utf8Path> {
        self.document.source_path.as_deref()
    }

    pub fn position(self, offset: u64) -> Result<TextPosition, DocumentError> {
        validate_range(
            TextRange {
                start: offset,
                end: offset,
            },
            self.len(),
            None,
        )?;
        self.validate_boundary(offset, None)?;
        Ok(TextPosition {
            version: self.version(),
            offset,
        })
    }

    pub(super) fn validate_boundary(
        self,
        offset: u64,
        index: Option<usize>,
    ) -> Result<(), DocumentError> {
        let mut start = 0;
        for piece in &self.document.pieces {
            let end = start + piece.range.len() as u64;
            if offset <= end {
                return if self.document.buffers[piece.buffer]
                    .is_char_boundary(piece.range.start + (offset - start) as usize)
                {
                    Ok(())
                } else {
                    Err(DocumentError::InvalidBoundary { index, offset })
                };
            }
            start = end;
        }
        Ok(())
    }

    pub fn chunks(
        self,
        version: DocumentVersion,
        range: TextRange,
    ) -> Result<Chunks<'a>, DocumentError> {
        self.version().check(version)?;
        validate_range(range, self.len(), None)?;
        self.validate_boundary(range.start, None)?;
        self.validate_boundary(range.end, None)?;
        Ok(Chunks {
            view: self,
            range,
            piece: 0,
            offset: 0,
        })
    }

    pub fn read(
        self,
        ct: &CancellationToken,
        version: DocumentVersion,
        range: TextRange,
    ) -> Result<Cow<'a, str>, DocumentError> {
        check_cancelled(ct)?;
        let mut result = Cow::Borrowed("");
        for chunk in self.chunks(version, range)? {
            append_text(&mut result, chunk, ct)?;
        }
        check_cancelled(ct)?;
        Ok(result)
    }

    pub fn write_range(
        self,
        ct: &CancellationToken,
        version: DocumentVersion,
        range: TextRange,
        writer: &mut impl Write,
    ) -> Result<(), DocumentError> {
        check_cancelled(ct)?;
        for chunk in self.chunks(version, range)? {
            for block in blocks(chunk) {
                check_cancelled(ct)?;
                writer.write_all(block.as_bytes())?;
            }
        }
        check_cancelled(ct)
    }

    pub fn lines<'c>(self, ct: &'c CancellationToken) -> Lines<'a, 'c> {
        Lines {
            chunks: self.all_chunks(),
            remaining: "",
            offset: 0,
            ct,
            finished: false,
        }
    }

    pub fn range_lines<'c>(
        self,
        ct: &'c CancellationToken,
        version: DocumentVersion,
        range: TextRange,
    ) -> Result<Lines<'a, 'c>, DocumentError> {
        check_cancelled(ct)?;
        Ok(Lines {
            chunks: self.chunks(version, range)?,
            remaining: "",
            offset: range.start,
            ct,
            finished: false,
        })
    }

    pub fn char_indices<'c>(self, ct: &'c CancellationToken) -> CharIndices<'a, 'c> {
        CharIndices {
            chunks: self.all_chunks(),
            chars: "".char_indices(),
            start: 0,
            next_start: 0,
            checked_at: 0,
            ct,
            finished: false,
        }
    }

    pub fn prefix(
        self,
        ct: &CancellationToken,
        chars: usize,
    ) -> Result<Cow<'a, str>, DocumentError> {
        check_cancelled(ct)?;
        let end = match self.char_indices(ct).nth(chars) {
            Some(value) => value?.0,
            None => self.len(),
        };
        self.read(ct, self.version(), TextRange { start: 0, end })
    }

    fn all_chunks(self) -> Chunks<'a> {
        Chunks {
            view: self,
            range: TextRange {
                start: 0,
                end: self.len(),
            },
            piece: 0,
            offset: 0,
        }
    }
}

pub struct Chunks<'a> {
    view: TextView<'a>,
    range: TextRange,
    piece: usize,
    offset: u64,
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        while self.offset < self.range.end {
            let piece = self.view.document.pieces.get(self.piece)?;
            let end = self.offset + piece.range.len() as u64;
            let start = self.range.start.max(self.offset);
            let stop = self.range.end.min(end);
            let base = self.offset;
            self.offset = end;
            self.piece += 1;
            if start < stop {
                return Some(
                    &self.view.document.buffers[piece.buffer][piece.range.start
                        + (start - base) as usize
                        ..piece.range.start + (stop - base) as usize],
                );
            }
        }
        None
    }
}

pub struct LogicalLine<'a> {
    pub range: TextRange,
    pub raw: Cow<'a, str>,
}

impl LogicalLine<'_> {
    pub fn text(&self) -> &str {
        let line = self.raw.strip_suffix('\n').unwrap_or(&self.raw);
        line.strip_suffix('\r').unwrap_or(line)
    }
}

pub struct Lines<'a, 'c> {
    chunks: Chunks<'a>,
    remaining: &'a str,
    offset: u64,
    ct: &'c CancellationToken,
    finished: bool,
}

impl<'a> Lines<'a, '_> {
    fn read_line(&mut self) -> Result<Option<LogicalLine<'a>>, DocumentError> {
        check_cancelled(self.ct)?;
        let start = self.offset;
        let mut raw = Cow::Borrowed("");
        loop {
            if self.remaining.is_empty() {
                self.remaining = self.chunks.next().unwrap_or("");
                if self.remaining.is_empty() {
                    break;
                }
            }
            let mut count = 0;
            let mut newline = false;
            // Search long contiguous lines in bounded pieces without copying them.
            for block in self.remaining.as_bytes().chunks(CHECK_BYTES) {
                check_cancelled(self.ct)?;
                if let Some(at) = block.iter().position(|byte| *byte == b'\n') {
                    count += at + 1;
                    newline = true;
                    break;
                }
                count += block.len();
            }
            let (part, rest) = self.remaining.split_at(count);
            append_text(&mut raw, part, self.ct)?;
            self.remaining = rest;
            self.offset += count as u64;
            if newline {
                break;
            }
        }
        check_cancelled(self.ct)?;
        Ok((self.offset != start).then_some(LogicalLine {
            range: TextRange {
                start,
                end: self.offset,
            },
            raw,
        }))
    }
}

impl<'a> Iterator for Lines<'a, '_> {
    type Item = Result<LogicalLine<'a>, DocumentError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.read_line() {
            Ok(Some(line)) => Some(Ok(line)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

pub struct CharIndices<'a, 'c> {
    chunks: Chunks<'a>,
    chars: std::str::CharIndices<'a>,
    start: u64,
    next_start: u64,
    checked_at: u64,
    ct: &'c CancellationToken,
    finished: bool,
}

impl Iterator for CharIndices<'_, '_> {
    type Item = Result<(u64, char), DocumentError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        loop {
            if let Some((offset, ch)) = self.chars.next() {
                let offset = self.start + offset as u64;
                if offset - self.checked_at >= CHECK_BYTES as u64 {
                    self.checked_at = offset;
                    if let Err(error) = check_cancelled(self.ct) {
                        self.finished = true;
                        return Some(Err(error));
                    }
                }
                return Some(Ok((offset, ch)));
            }
            if let Err(error) = check_cancelled(self.ct) {
                self.finished = true;
                return Some(Err(error));
            }
            let Some(chunk) = self.chunks.next() else {
                self.finished = true;
                return None;
            };
            self.start = self.next_start;
            self.next_start += chunk.len() as u64;
            self.chars = chunk.char_indices();
        }
    }
}

fn blocks(mut text: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let mut end = text.len().min(CHECK_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let (block, rest) = text.split_at(end);
        text = rest;
        Some(block)
    })
}

fn append_text<'a>(
    output: &mut Cow<'a, str>,
    part: &'a str,
    ct: &CancellationToken,
) -> Result<(), DocumentError> {
    check_cancelled(ct)?;
    if output.is_empty() {
        *output = Cow::Borrowed(part);
        return Ok(());
    }
    if let Cow::Borrowed(prefix) = output {
        let mut owned = String::with_capacity(prefix.len() + part.len());
        for block in blocks(prefix) {
            check_cancelled(ct)?;
            owned.push_str(block);
        }
        *output = Cow::Owned(owned);
    }
    for block in blocks(part) {
        check_cancelled(ct)?;
        output.to_mut().push_str(block);
    }
    Ok(())
}
