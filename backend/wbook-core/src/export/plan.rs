use std::collections::{HashMap, HashSet};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::{check, ExportFailure, ExportOptions, RenderLayout, Result};
use crate::document::{DocumentVersion, ProcessingDocument, TextView};
use crate::toc::{Toc, TocRangeKind};
use crate::types::TextRange;

#[derive(Debug, Serialize)]
pub(super) struct PublicationMetadata {
    pub title: String,
    pub author: Option<String>,
    pub language: String,
    pub identifier: String,
    pub modified: String,
}

#[derive(Debug)]
pub(super) struct Section {
    pub id: String,
    pub title: String,
    pub depth: usize,
    pub body: Option<TextRange>,
}

#[derive(Debug)]
pub(super) struct BookPlan {
    pub version: DocumentVersion,
    pub metadata: PublicationMetadata,
    pub sections: Vec<Section>,
    pub layout: RenderLayout,
}

pub(super) fn invalid(message: impl Into<String>) -> ExportFailure {
    ExportFailure::InvalidInput(message.into())
}

pub(super) fn xml_text(ct: &CancellationToken, text: &str, location: &str) -> Result<()> {
    let mut checked = 0;
    for (offset, ch) in text.char_indices() {
        if offset - checked >= 16384 {
            check(ct)?;
            checked = offset;
        }
        if !matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
        {
            return Err(invalid(format!(
                "XML character U+{:04X} at {location}, byte {offset}",
                ch as u32
            )));
        }
    }
    check(ct)
}

pub(super) fn build(
    ct: &CancellationToken,
    document: &ProcessingDocument,
    options: &ExportOptions,
) -> Result<BookPlan> {
    check(ct)?;
    let results = document.current_results()?;
    let view = document.view();
    if view.is_empty() {
        return Err(invalid("empty document"));
    }
    let meta = document.metadata()?;
    let title = meta
        .title
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("missing book title"))?;
    let language = language_tags::LanguageTag::parse(&options.language)
        .map_err(|e| invalid(format!("invalid language: {e}")))?;
    language
        .validate()
        .map_err(|e| invalid(format!("invalid language: {e}")))?;
    let identifier = options
        .identifier
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().urn().to_string());
    if identifier.trim().is_empty() {
        return Err(invalid("empty identifier"));
    }
    let metadata = PublicationMetadata {
        title,
        author: meta.author.filter(|a| !a.trim().is_empty()),
        language: options.language.clone(),
        identifier,
        modified: jiff::Timestamp::now()
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
    };
    for (field, value) in [
        ("title", metadata.title.as_str()),
        ("author", metadata.author.as_deref().unwrap_or("")),
        ("identifier", metadata.identifier.as_str()),
    ] {
        xml_text(ct, value, field)?;
    }

    // Traverse defensively because get_mut allows callers to change tree links.
    let toc = &results.toc;
    let mut stack: Vec<_> = toc
        .root_ids()
        .iter()
        .rev()
        .map(|id| (*id, 0, None))
        .collect();
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    while let Some((id, parent_depth, parent)) = stack.pop() {
        check(ct)?;
        if !seen.insert(id) {
            return Err(invalid(format!("repeated TOC node {id:?}")));
        }
        let node = toc
            .get(id)
            .ok_or_else(|| invalid(format!("missing TOC node {id:?}")))?;
        if node.parent != parent || node.id != id {
            return Err(invalid(format!("inconsistent TOC links at {id:?}")));
        }
        xml_text(ct, &node.title, &format!("TOC node {id:?}"))?;
        let depth = parent_depth + usize::from(!node.title.trim().is_empty());
        ordered.push((node, depth));
        stack.extend(
            node.children
                .iter()
                .rev()
                .map(|child| (*child, depth, Some(id))),
        );
    }
    if seen.len() != toc.nodes().count() {
        return Err(invalid("unreachable TOC nodes"));
    }
    let mut source = Vec::new();
    let mut kind = None;
    for (node, _) in &ordered {
        check(ct)?;
        if let Some(range) = node.meta.range {
            view.chunks(view.version(), range)?;
        }
        match node.meta.range_kind {
            TocRangeKind::Unknown => {
                return Err(invalid(format!(
                    "unknown range semantics at {:?}; reparse or confirm the TOC",
                    node.id
                )))
            }
            TocRangeKind::Container => {}
            current => {
                if kind.is_some_and(|kind| kind != current) {
                    return Err(invalid("mixed heading and body ranges"));
                }
                kind = Some(current);
                let range =
                    node.meta.range.filter(|r| r.start < r.end).ok_or_else(|| {
                        invalid(format!("missing or empty range at {:?}", node.id))
                    })?;
                source.push((node.id, range));
            }
        }
    }
    source.sort_by_key(|(_, range)| range.start);
    let mut bodies = HashMap::new();
    let mut cursor = 0;
    for (index, (id, range)) in source.iter().enumerate() {
        check(ct)?;
        if range.start < cursor {
            return Err(invalid(format!("overlapping source ranges at {id:?}")));
        }
        if kind == Some(TocRangeKind::Body) {
            if range.start != cursor {
                return Err(invalid(format!("body range gap before {id:?}")));
            }
            bodies.insert(*id, *range);
            cursor = range.end;
        } else {
            for part in view.chunks(view.version(), *range)? {
                if part.contains(['\r', '\n']) {
                    return Err(invalid(format!(
                        "heading range contains a newline at {id:?}"
                    )));
                }
            }
            let start = heading_end(view, range.end)?;
            let end = source
                .get(index + 1)
                .map_or(view.len(), |(_, next)| next.start);
            if start > end {
                return Err(invalid(format!(
                    "overlapping heading consumption at {id:?}"
                )));
            }
            bodies.insert(*id, TextRange { start, end });
            cursor = start;
        }
    }
    if kind == Some(TocRangeKind::Body) && cursor != view.len() {
        return Err(invalid("body ranges do not cover the end of the document"));
    }
    let prefix_end = source.first().map_or(view.len(), |(_, r)| r.start);
    let mut sections = Vec::new();
    if prefix_end > 0 {
        sections.push(Section {
            id: String::new(),
            title: String::new(),
            depth: 0,
            body: Some(TextRange {
                start: 0,
                end: prefix_end,
            }),
        });
    }
    for (node, depth) in ordered {
        check(ct)?;
        let body = bodies.remove(&node.id);
        if body.is_some() || !node.title.trim().is_empty() {
            sections.push(Section {
                id: String::new(),
                title: node.title.clone(),
                depth,
                body,
            });
        }
    }
    for (index, section) in sections.iter_mut().enumerate() {
        section.id = format!("section-{:04}", index + 1);
    }
    check(ct)?;
    Ok(BookPlan {
        version: view.version(),
        metadata,
        sections,
        layout: options.render.layout,
    })
}

fn heading_end(view: TextView<'_>, end: u64) -> Result<u64> {
    let next: Vec<u8> = view
        .chunks(
            view.version(),
            TextRange {
                start: end,
                end: view.len(),
            },
        )?
        .flat_map(str::bytes)
        .take(2)
        .collect();
    Ok(end
        + if next.starts_with(b"\r\n") {
            2
        } else if next.starts_with(b"\n") {
            1
        } else {
            0
        })
}
