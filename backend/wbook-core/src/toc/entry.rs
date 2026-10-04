use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::{NodeId, Toc, TocError, TocNode, TocRoot, TreeNodeMeta};

/// Wire-format TOC entry (aligned with calibre's "TOC entry" terminology).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct TocEntry {
    pub id: NodeId,
    pub title: String,
    pub meta: TreeNodeMeta,
    pub children: Vec<TocEntry>,
}

impl<'de> Deserialize<'de> for TocEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct WireEntry {
            id: NodeId,
            title: String,
            meta: TreeNodeMeta,
            children: Vec<TocEntry>,
            #[serde(default)]
            patch: serde_json::Value,
        }
        let entry = WireEntry::deserialize(deserializer)?;
        // Ignoring an old patch would silently discard the user's text edits.
        if !entry.patch.is_null() {
            return Err(serde::de::Error::custom(
                "legacy text patches are not supported",
            ));
        }
        Ok(Self {
            id: entry.id,
            title: entry.title,
            meta: entry.meta,
            children: entry.children,
        })
    }
}

/// The serialized form of a `TocRoot`; deserializing it rebuilds the parent weak references.
pub type TocSnapshot = Vec<TocEntry>;

impl From<&TocRoot> for TocSnapshot {
    fn from(toc: &TocRoot) -> Self {
        let mut root = TocSnapshot::new();
        for child_id in toc.children.iter() {
            root.push(TocEntry::from_toc_node(toc, toc.get(*child_id).unwrap()));
        }
        root
    }
}

impl From<TocRoot> for TocSnapshot {
    fn from(toc: TocRoot) -> Self {
        TocSnapshot::from(&toc)
    }
}

impl TryFrom<TocSnapshot> for TocRoot {
    type Error = TocError;

    fn try_from(snapshot: TocSnapshot) -> Result<Self, Self::Error> {
        let mut seen = HashSet::new();
        let mut toc = TocRoot::new();
        for entry in snapshot {
            insert_entry(&mut toc, entry, None, &mut seen)?;
        }
        Ok(toc)
    }
}

fn insert_entry(
    toc: &mut TocRoot,
    entry: TocEntry,
    parent: Option<NodeId>,
    seen: &mut HashSet<NodeId>,
) -> Result<NodeId, TocError> {
    if !seen.insert(entry.id) {
        return Err(TocError::InvalidSnapshot(format!(
            "duplicated node id: {}",
            entry.id.0
        )));
    }
    if let Some(range) = entry.meta.range {
        if range.start > range.end {
            return Err(TocError::InvalidSnapshot(format!(
                "inverted range [{}, {}) on node `{}`",
                range.start, range.end, entry.title
            )));
        }
    }
    let id = toc
        .add_with_meta(&entry.title, Some(entry.meta), parent)?
        .id;
    for child in entry.children {
        insert_entry(toc, child, Some(id), seen)?;
    }
    Ok(id)
}

impl TocEntry {
    fn from_toc_node(toc: &TocRoot, toc_node: &TocNode) -> Self {
        let mut node = TocEntry {
            id: toc_node.id,
            title: toc_node.title.clone(),
            meta: toc_node.meta.clone(),
            children: Vec::new(),
        };
        for child_id in toc_node.children.iter() {
            node.children
                .push(TocEntry::from_toc_node(toc, toc.get(*child_id).unwrap()));
        }
        node
    }
}

impl TocRoot {
    pub fn dump(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}
