use super::{NodeId, Toc, TocError, TocRoot, TreeNodeMeta};
use crate::types::TextRange;

/// A single flattened TOC event, aligned with calibre's level-based TOC detection:
/// `--level1-toc` / `--level2-toc` / `--level3-toc` xpath rules produce a flat
/// (level, title, position) stream that is then assembled into a tree.
#[derive(Debug, Clone)]
pub struct TocEvent {
    /// 1-based level, aligned with calibre's level1/2/3 convention.
    pub level: usize,
    pub title: String,
    /// `None` for pure container entries.
    pub range: Option<TextRange>,
}

#[derive(Debug, Clone, Copy)]
pub struct BuilderOptions {
    /// Whether `build()` backfills container-node ranges bottom-up as the
    /// union (`TextRange::merge`) of their children's ranges. Defaults to true.
    pub backfill_container_range: bool,
}

impl Default for BuilderOptions {
    fn default() -> Self {
        Self {
            backfill_container_range: true,
        }
    }
}

/// Streaming builder turning a flat `TocEvent` stream into a `TocRoot`.
///
/// Note: `play_order` (for NCX export) is not stored; compute it later as a
/// DFS order over the finished tree.
pub struct TocBuilder {
    root: TocRoot,
    /// stack[i] is the most recent node at level i+1 on the current path.
    stack: Vec<NodeId>,
    options: BuilderOptions,
}

impl Default for TocBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TocBuilder {
    pub fn new() -> Self {
        Self::with_options(BuilderOptions::default())
    }

    pub fn with_options(options: BuilderOptions) -> Self {
        Self {
            root: TocRoot::new(),
            stack: Vec::new(),
            options,
        }
    }

    pub fn push(&mut self, event: TocEvent) -> Result<NodeId, TocError> {
        let TocEvent {
            level,
            title,
            range,
        } = event;
        let level = level.max(1);
        // Level jump: auto-insert anonymous container nodes for the missing levels.
        while level > self.stack.len() + 1 {
            let parent = self.stack.last().copied();
            let id = self.root.add_with_meta("", None, parent)?.id;
            self.stack.push(id);
        }
        // Level fallback: pop the stack so the top is the parent level.
        self.stack.truncate(level - 1);
        let parent = self.stack.last().copied();
        let id = self
            .root
            .add_with_meta(&title, Some(TreeNodeMeta { words: 0, range }), parent)?
            .id;
        self.stack.push(id);
        Ok(id)
    }

    pub fn build(mut self) -> TocRoot {
        if self.options.backfill_container_range {
            self.backfill_ranges();
        }
        self.root
    }

    fn backfill_ranges(&mut self) {
        let mut by_depth: Vec<(NodeId, usize)> = self
            .root
            .container
            .iter()
            .map(|(key, _)| {
                let id = NodeId(key);
                (id, self.root.level(id).unwrap_or(0))
            })
            .collect();
        // Deepest nodes first, so children are backfilled before their parents.
        by_depth.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
        for (id, _) in by_depth {
            let node = self.root.get(id).unwrap();
            if node.meta.range.is_some() || node.children.is_empty() {
                continue;
            }
            let merged = node
                .children
                .iter()
                .filter_map(|child| self.root.get(*child).and_then(|n| n.meta.range))
                .reduce(TextRange::merge);
            if let Some(range) = merged {
                self.root.get_mut(id).unwrap().meta.range = Some(range);
            }
        }
    }
}
