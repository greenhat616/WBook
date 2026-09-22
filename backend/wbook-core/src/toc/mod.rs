// A toc is a vector of TreeNodes. Each TreeNode has a title, a range, and a vector of children.
// All node can swap places with their siblings, and can be moved up or down in the tree.

use slab::Slab;

use serde::{Deserialize, Serialize};
use specta::Type;

pub use self::builder::{BuilderOptions, TocBuilder, TocEvent};
pub use self::entry::{TocEntry, TocSnapshot};
pub use self::error::TocError;
pub use self::id::NodeId;

use crate::types::TextRange;

mod builder;
mod entry;
mod error;
mod id;

#[cfg(test)]
mod tests;

// Meta info for a node
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TreeNodeMeta {
    pub words: u64,
    /// The content range of the node itself. `None` for pure container nodes
    /// (like calibre TOC entries whose src points at their first child).
    pub range: Option<TextRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TocNode {
    pub id: NodeId, // id is the index of the node in the slab
    pub title: String,
    pub patch: Option<String>, // git-diff like patch content, to be applied while document is split.
    pub meta: TreeNodeMeta,
    #[serde(skip_serializing, default)]
    // No need to serialize parent, it's a weak reference, should rebuild on data load.
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

// Serialization goes both ways through the `TocSnapshot` wire format.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(try_from = "TocSnapshot", into = "TocSnapshot")]
#[specta(type = TocSnapshot)]
pub struct TocRoot {
    children: Vec<NodeId>,
    container: Slab<TocNode>,
}

pub trait Toc {
    fn new() -> Self;
    fn add(
        &mut self,
        title: &str,
        range: TextRange,
        parent: Option<NodeId>,
    ) -> Result<&TocNode, TocError>;
    fn add_with_meta(
        &mut self,
        title: &str,
        meta: Option<TreeNodeMeta>,
        parent: Option<NodeId>,
    ) -> Result<&TocNode, TocError>;
    fn remove(&mut self, id: NodeId);
    fn move_up(&mut self, id: NodeId);
    fn move_down(&mut self, id: NodeId);
    fn move_left(&mut self, id: NodeId);
    fn move_right(&mut self, id: NodeId);
    fn move_belong_to(&mut self, id: NodeId, parent: NodeId);
    fn move_before(&mut self, id: NodeId, target_node: NodeId);
    fn move_after(&mut self, id: NodeId, target_node: NodeId);
    fn get(&self, id: NodeId) -> Option<&TocNode>;
    fn get_mut(&mut self, id: NodeId) -> Option<&mut TocNode>;
    fn get_root(&self) -> &TocRoot;
    fn contains(&self, id: NodeId) -> bool;
    /// 1-based depth of the node, computed by walking the parent chain
    /// (aligned with calibre's level1/2/3 convention, never stored).
    fn level(&self, id: NodeId) -> Option<usize>;
}

impl Toc for TocRoot {
    fn new() -> Self {
        TocRoot {
            children: Vec::new(),
            container: Slab::new(),
        }
    }

    fn add(
        &mut self,
        title: &str,
        range: TextRange,
        parent: Option<NodeId>,
    ) -> Result<&TocNode, TocError> {
        self.add_with_meta(
            title,
            Some(TreeNodeMeta {
                words: 0,
                range: Some(range),
            }),
            parent,
        )
    }

    fn add_with_meta(
        &mut self,
        title: &str,
        meta: Option<TreeNodeMeta>,
        parent: Option<NodeId>,
    ) -> Result<&TocNode, TocError> {
        if let Some(parent_id) = parent {
            if !self.contains(parent_id) {
                return Err(TocError::NodeParentNotFound(parent_id));
            }
        }
        let entry = self.container.vacant_entry();
        let id = NodeId(entry.key());
        entry.insert(TocNode {
            id,
            title: title.to_string(),
            patch: None,
            meta: meta.unwrap_or(TreeNodeMeta {
                words: 0,
                range: None,
            }),
            parent,
            children: Vec::new(),
        });
        if let Some(parent_id) = parent {
            let parent = self.container.get_mut(parent_id.0).unwrap();
            parent.children.push(id);
        } else {
            self.children.push(id);
        }
        Ok(self.container.get(id.0).unwrap())
    }

    fn remove(&mut self, id: NodeId) {
        if !self.contains(id) {
            return;
        }
        let node = self.container.remove(id.0);
        // Remove node children
        for child_id in node.children.iter() {
            self.remove(*child_id);
        }
        if let Some(parent_id) = node.parent {
            let parent = self.get_mut(parent_id).unwrap();
            parent.children.retain(|&x| x != id);
        } else {
            self.children.retain(|&x| x != id);
        }
    }

    fn move_up(&mut self, id: NodeId) {
        if !self.contains(id) {
            return;
        }
        let node = self.get_mut(id).unwrap();
        if let Some(parent_id) = node.parent {
            let parent = self.get_mut(parent_id).unwrap();
            let index = parent.children.iter().position(|&x| x == id).unwrap();
            if index > 0 {
                parent.children.swap(index, index - 1);
            }
        } else {
            let index = self.children.iter().position(|&x| x == id).unwrap();
            if index > 0 {
                self.children.swap(index, index - 1);
            }
        }
    }

    fn move_down(&mut self, id: NodeId) {
        if !self.contains(id) {
            return;
        }
        let node = self.get_mut(id).unwrap();
        if let Some(parent_id) = node.parent {
            let parent = self.get_mut(parent_id).unwrap();
            let index = parent.children.iter().position(|&x| x == id).unwrap();
            if index < parent.children.len() - 1 {
                parent.children.swap(index, index + 1);
            }
        } else {
            let index = self.children.iter().position(|&x| x == id).unwrap();
            if index < self.children.len() - 1 {
                self.children.swap(index, index + 1);
            }
        }
    }

    // Move node to the parent level of its parent
    fn move_right(&mut self, id: NodeId) {
        if !self.contains(id) {
            return;
        }
        let node = self.get(id).unwrap().clone();
        if node.parent.is_none() {
            return; // Root node, no parent, do nothing
        }
        let parent_id = node.parent.unwrap();
        let parent = self.container.get_mut(parent_id.0).unwrap();
        parent.children.retain(|&x| x != id);
        let grand_parent_id = parent.parent;
        let new_parent_id = match grand_parent_id {
            // node's parent is a child of root node
            None => {
                let root_children: &mut Vec<NodeId> = self.children.as_mut();
                root_children.push(id);
                None
            }
            Some(grand_parent_id) => {
                let grand_parent = self.get_mut(grand_parent_id).unwrap();
                grand_parent.children.push(id);
                Some(grand_parent_id)
            }
        };
        let node = self.get_mut(id).unwrap();
        node.parent = new_parent_id;
    }

    // Move node to the first child of its previous sibling
    fn move_left(&mut self, id: NodeId) {
        if !self.contains(id) {
            return;
        }
        let node = self.get(id).unwrap().clone();
        let parent_children: &mut Vec<NodeId> = match node.parent {
            Some(parent_id) => self.get_mut(parent_id).unwrap().children.as_mut(),
            None => self.children.as_mut(),
        };
        let index = parent_children.iter().position(|&x| x == id).unwrap();
        if index == 0 {
            return; // node is the first child, do nothing
        }
        parent_children.retain(|&x| x != id);
        let prev_sibling_id = parent_children[index - 1];
        let prev_sibling = self.get_mut(prev_sibling_id).unwrap();
        prev_sibling.children.push(id);
        let node = self.get_mut(id).unwrap();
        node.parent = Some(prev_sibling_id);
    }

    // Move node before another node
    fn move_before(&mut self, id: NodeId, target_node: NodeId) {
        if !self.contains(id) || !self.contains(target_node) {
            return;
        }
        let node = self.get(id).unwrap().clone();
        let parent_children: &mut Vec<NodeId> = match node.parent {
            Some(parent_id) => self.get_mut(parent_id).unwrap().children.as_mut(),
            None => self.children.as_mut(),
        };
        parent_children.retain(|&x| x != id);

        let target_node = self.get(target_node).unwrap().clone();
        let target_parent_children: &mut Vec<NodeId> = match target_node.parent {
            Some(parent_id) => self.get_mut(parent_id).unwrap().children.as_mut(),
            None => self.children.as_mut(),
        };
        let index = target_parent_children
            .iter()
            .position(|&x| x == target_node.id)
            .unwrap();
        target_parent_children.insert(index, id);
        let node = self.get_mut(id).unwrap();
        node.parent = target_node.parent;
    }

    // Move node after another node
    fn move_after(&mut self, id: NodeId, target_node: NodeId) {
        if !self.contains(id) || !self.contains(target_node) {
            return;
        }
        let node = self.get(id).unwrap().clone();
        let parent_children: &mut Vec<NodeId> = match node.parent {
            Some(parent_id) => self.get_mut(parent_id).unwrap().children.as_mut(),
            None => self.children.as_mut(),
        };
        parent_children.retain(|&x| x != id);

        let target_node = self.get(target_node).unwrap().clone();
        let target_parent_children: &mut Vec<NodeId> = match target_node.parent {
            Some(parent_id) => self.get_mut(parent_id).unwrap().children.as_mut(),
            None => self.children.as_mut(),
        };
        let index = target_parent_children
            .iter()
            .position(|&x| x == target_node.id)
            .unwrap();
        target_parent_children.insert(index + 1, id);
        let node = self.get_mut(id).unwrap();
        node.parent = target_node.parent;
    }

    ///
    /// Move node to the children of another node
    ///
    fn move_belong_to(&mut self, id: NodeId, parent: NodeId) {
        if !self.contains(id) || !self.contains(parent) {
            return;
        }
        let node = self.get(id).unwrap().clone();
        let parent_children: &mut Vec<NodeId> = match node.parent {
            Some(parent_id) => self.get_mut(parent_id).unwrap().children.as_mut(),
            None => self.children.as_mut(),
        };
        parent_children.retain(|&x| x != id);

        let parent_node = self.get_mut(parent).unwrap();
        parent_node.children.push(id);
        let node = self.get_mut(id).unwrap();
        node.parent = Some(parent);
    }

    fn get(&self, id: NodeId) -> Option<&TocNode> {
        self.container.get(id.0)
    }

    fn get_mut(&mut self, id: NodeId) -> Option<&mut TocNode> {
        self.container.get_mut(id.0)
    }

    fn get_root(&self) -> &TocRoot {
        self
    }

    fn contains(&self, id: NodeId) -> bool {
        self.container.contains(id.0)
    }

    fn level(&self, id: NodeId) -> Option<usize> {
        let mut node = self.get(id)?;
        let mut level = 1;
        while let Some(parent_id) = node.parent {
            level += 1;
            node = self.get(parent_id)?;
        }
        Some(level)
    }
}
