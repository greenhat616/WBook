use crate::types::TextRange;

use super::{
    BuilderOptions, NodeId, Toc, TocBuilder, TocEntry, TocError, TocEvent, TocRoot, TocSnapshot,
    TreeNodeMeta,
};

fn range(start: u64, end: u64) -> TextRange {
    TextRange::new(start, end).unwrap()
}

fn meta() -> TreeNodeMeta {
    TreeNodeMeta {
        words: 0,
        range: Some(range(0, 0)),
    }
}

#[test]
fn test_toc_new() {
    let mut toc = TocRoot::new();
    let node = toc.add("test", range(0, 0), None).unwrap();
    assert_eq!(node.title, "test");
    assert_eq!(node.meta.range, Some(range(0, 0)));
    assert_eq!(node.parent, None);
    assert_eq!(node.children.len(), 0);
}

#[test]
fn test_toc_add() {
    let mut toc = TocRoot::new();
    let node = toc.add("test", range(0, 0), None).unwrap();
    assert_eq!(node.title, "test");
    assert_eq!(node.meta.range, Some(range(0, 0)));
    assert_eq!(node.parent, None);
    assert_eq!(node.children.len(), 0);
}

#[test]
fn test_toc_add_with_meta() {
    let mut toc = TocRoot::new();
    let node = toc.add_with_meta("test", Some(meta()), None).unwrap();
    assert_eq!(node.title, "test");
    assert_eq!(node.meta.range, Some(range(0, 0)));
    assert_eq!(node.parent, None);
    assert_eq!(node.children.len(), 0);
}

#[test]
fn test_toc_add_with_meta_defaults_to_none_range() {
    let mut toc = TocRoot::new();
    let node = toc.add_with_meta("test", None, None).unwrap();
    assert_eq!(node.meta.range, None);
}

#[test]
fn test_toc_add_with_missing_parent() {
    let mut toc = TocRoot::new();
    let result = toc.add("test", range(0, 0), Some(NodeId(42)));
    assert!(matches!(
        result,
        Err(TocError::NodeParentNotFound(NodeId(42)))
    ));
}

#[test]
fn test_toc_get() {
    let mut toc = TocRoot::new();
    let node_id = toc.add("test", range(0, 0), None).unwrap().id;
    let node = toc.get(node_id).unwrap();
    assert_eq!(node.title, "test");
    assert_eq!(node.meta.range, Some(range(0, 0)));
    assert_eq!(node.parent, None);
    assert_eq!(node.children.len(), 0);
}

#[test]
fn test_toc_add_with_meta_and_parent() {
    let mut toc = TocRoot::new();
    let node_id = toc.add_with_meta("test", Some(meta()), None).unwrap().id;
    toc.add_with_meta("test2", Some(meta()), Some(node_id))
        .unwrap();
    let node1 = toc.get(node_id).unwrap();
    let node2 = toc.get(node1.children[0]).unwrap();
    assert_eq!(node2.title, "test2");
    assert_eq!(node2.meta.range, Some(range(0, 0)));
    assert_eq!(node2.parent, Some(node_id));
    assert_eq!(node2.children.len(), 0);
}

#[test]
fn test_remove() {
    let mut toc = TocRoot::new();
    let node_id = toc.add_with_meta("test", Some(meta()), None).unwrap().id;
    toc.remove(node_id);
    assert!(!toc.contains(node_id));
    assert_eq!(toc.children.len(), 0);
}

#[test]
fn test_move_up() {
    let mut toc = TocRoot::new();
    let node_id = toc.add_with_meta("test", Some(meta()), None).unwrap().id;
    let node_id2 = toc.add_with_meta("test2", Some(meta()), None).unwrap().id;
    toc.move_up(node_id2);
    assert_eq!(toc.children[0], node_id2);
    assert_eq!(toc.children[1], node_id);
}

#[test]
fn test_move_down() {
    let mut toc = TocRoot::new();
    let node_id = toc.add_with_meta("test", Some(meta()), None).unwrap().id;
    let node_id2 = toc.add_with_meta("test2", Some(meta()), None).unwrap().id;
    toc.move_down(node_id);
    assert_eq!(toc.children[0], node_id2);
    assert_eq!(toc.children[1], node_id);
}

#[test]
fn test_move_right() {
    let mut toc = TocRoot::new();
    let node_id = toc.add_with_meta("test", Some(meta()), None).unwrap().id;
    let node_id2 = toc
        .add_with_meta("test2", Some(meta()), Some(node_id))
        .unwrap()
        .id;
    toc.move_right(node_id2);
    let node = toc.get(node_id2).unwrap();
    assert_eq!(node.parent, None);
    assert_eq!(toc.children.len(), 2);
    assert_eq!(toc.children[0], node_id);
    assert_eq!(toc.children[1], node_id2);
}

#[test]
fn test_move_left() {
    let mut toc = TocRoot::new();
    let node_id = toc.add_with_meta("test", Some(meta()), None).unwrap().id;
    let node_id2 = toc
        .add_with_meta("test2", Some(meta()), Some(node_id))
        .unwrap()
        .id;
    toc.move_left(node_id2);
    let node = toc.get(node_id2).unwrap();
    assert_eq!(node.parent, Some(node_id));
    assert_eq!(toc.children.len(), 1);
    assert_eq!(toc.children[0], node_id);
    assert_eq!(toc.get(node_id).unwrap().children[0], node_id2);
}

//
// Test move_before fn with 5 nodes,  Move node5 before node1
// Before move:
// - node1
// - node2 - node3 - node5
//         - node4
//
// After move:
// - node5
// - node1
// - node2 - node3
//         - node4
//
#[test]
fn test_move_before() {
    let mut toc = TocRoot::new();
    let node_id1 = toc.add("node1", range(0, 0), None).unwrap().id;
    let node_id2 = toc.add("node2", range(0, 0), None).unwrap().id;
    let node_id3 = toc.add("node3", range(0, 0), Some(node_id2)).unwrap().id;
    let node_id4 = toc.add("node4", range(0, 0), Some(node_id2)).unwrap().id;
    let node_id5 = toc.add("node5", range(0, 0), Some(node_id3)).unwrap().id;
    toc.move_before(node_id5, node_id1);
    assert_eq!(toc.children.len(), 3);
    assert_eq!(toc.children[0], node_id5);
    assert_eq!(toc.children[1], node_id1);
    assert_eq!(toc.children[2], node_id2);
    assert_eq!(toc.get(node_id5).unwrap().parent, None);
    assert_eq!(toc.get(node_id2).unwrap().children.len(), 2);
    assert_eq!(toc.get(node_id2).unwrap().children[0], node_id3);
    assert_eq!(toc.get(node_id2).unwrap().children[1], node_id4);
    assert_eq!(toc.get(node_id3).unwrap().children.len(), 0);
}

//
// Test move_after fn with 5 nodes,  Move node5 after node2
// Before move:
// - node1
// - node2 - node3 - node5
//         - node4
//
// After move:
// - node1
// - node2 - node3
//         - node4
// - node5
//
#[test]
fn test_move_after() {
    let mut toc = TocRoot::new();
    let node_id1 = toc.add("node1", range(0, 0), None).unwrap().id;
    let node_id2 = toc.add("node2", range(0, 0), None).unwrap().id;
    let node_id3 = toc.add("node3", range(0, 0), Some(node_id2)).unwrap().id;
    let node_id4 = toc.add("node4", range(0, 0), Some(node_id2)).unwrap().id;
    let node_id5 = toc.add("node5", range(0, 0), Some(node_id3)).unwrap().id;
    toc.move_after(node_id5, node_id2);
    assert_eq!(toc.children.len(), 3);
    assert_eq!(toc.children[0], node_id1);
    assert_eq!(toc.children[1], node_id2);
    assert_eq!(toc.children[2], node_id5);
    assert_eq!(toc.get(node_id5).unwrap().parent, None);
    assert_eq!(toc.get(node_id2).unwrap().children.len(), 2);
    assert_eq!(toc.get(node_id2).unwrap().children[0], node_id3);
    assert_eq!(toc.get(node_id2).unwrap().children[1], node_id4);
    assert_eq!(toc.get(node_id3).unwrap().children.len(), 0);
}

//
// Test move_belong_to fn with 5 nodes,  Move node2 belong to node1
// Before move:
// - node1
// - node2 - node3 - node5
//         - node4
//
// After move:
// - node1 - node2 - node3 - node5
//                 - node4
//
#[test]
fn test_move_belong_to() {
    let mut toc = TocRoot::new();
    let node_id1 = toc.add("node1", range(0, 0), None).unwrap().id;
    let node_id2 = toc.add("node2", range(0, 0), None).unwrap().id;
    let node_id3 = toc.add("node3", range(0, 0), Some(node_id2)).unwrap().id;
    let node_id4 = toc.add("node4", range(0, 0), Some(node_id2)).unwrap().id;
    let node_id5 = toc.add("node5", range(0, 0), Some(node_id3)).unwrap().id;
    toc.move_belong_to(node_id2, node_id1);
    assert_eq!(toc.children.len(), 1);
    assert_eq!(toc.children[0], node_id1);
    assert_eq!(toc.get(node_id1).unwrap().children.len(), 1);
    assert_eq!(toc.get(node_id1).unwrap().children[0], node_id2);
    assert_eq!(toc.get(node_id2).unwrap().children.len(), 2);
    assert_eq!(toc.get(node_id2).unwrap().children[0], node_id3);
    assert_eq!(toc.get(node_id2).unwrap().children[1], node_id4);
    assert_eq!(toc.get(node_id2).unwrap().parent, Some(node_id1));
    assert_eq!(toc.get(node_id5).unwrap().parent, Some(node_id3));
}

#[test]
fn test_get_mut() {
    let mut toc = TocRoot::new();
    let node_id = toc.add("test", range(0, 0), None).unwrap().id;
    let node = toc.get_mut(node_id).unwrap();
    node.title = "test2".to_string();
    assert_eq!(node.title, "test2");
}

#[test]
fn test_get_root() {
    let mut toc = TocRoot::new();
    let node_id = toc.add("test", range(0, 0), None).unwrap().id;
    let root = toc.get_root();
    assert_eq!(root.children[0], node_id);
}

#[test]
fn test_contains() {
    let mut toc = TocRoot::new();
    let node_id = toc.add("test", range(0, 0), None).unwrap().id;
    assert!(toc.contains(node_id));
}

#[test]
fn test_level() {
    let mut toc = TocRoot::new();
    let node_id1 = toc.add("node1", range(0, 0), None).unwrap().id;
    let node_id2 = toc.add("node2", range(0, 0), Some(node_id1)).unwrap().id;
    let node_id3 = toc.add("node3", range(0, 0), Some(node_id2)).unwrap().id;
    assert_eq!(toc.level(node_id1), Some(1));
    assert_eq!(toc.level(node_id2), Some(2));
    assert_eq!(toc.level(node_id3), Some(3));
    assert_eq!(toc.level(NodeId(999)), None);
}

#[test]
fn test_dump() {
    let mut toc = TocRoot::new();
    toc.add("test", range(0, 0), None).unwrap();
    let buf = toc.dump().unwrap();
    assert_eq!(
        buf,
        "[{\"id\":0,\"title\":\"test\",\"patch\":null,\"meta\":{\"words\":0,\"range\":{\"start\":0,\"end\":0}},\"children\":[]}]"
    );
}

#[test]
fn test_snapshot_roundtrip() {
    let mut toc = TocRoot::new();
    let node_id1 = toc.add("node1", range(0, 5), None).unwrap().id;
    let node_id2 = toc.add("node2", range(5, 10), Some(node_id1)).unwrap().id;
    toc.add("node3", range(10, 15), Some(node_id2)).unwrap();
    toc.add("node4", range(15, 20), None).unwrap();

    // Roundtrip through the JSON wire format.
    let json = toc.dump().unwrap();
    let restored: TocRoot = serde_json::from_str(&json).unwrap();

    // Tree equality via snapshots, and parent links rebuilt.
    assert_eq!(TocSnapshot::from(&restored), TocSnapshot::from(&toc));
    assert_eq!(restored.get(node_id2).unwrap().parent, Some(node_id1));
    assert_eq!(restored.level(node_id2), Some(2));
}

#[test]
fn test_snapshot_roundtrip_preserves_patch() {
    let mut toc = TocRoot::new();
    let node_id = toc.add("node1", range(0, 5), None).unwrap().id;
    toc.get_mut(node_id).unwrap().patch = Some("@@ patch".to_string());

    let snapshot = TocSnapshot::from(&toc);
    let restored = TocRoot::try_from(snapshot).unwrap();
    assert_eq!(
        restored.get(node_id).unwrap().patch,
        Some("@@ patch".to_string())
    );
}

#[test]
fn test_try_from_snapshot_rejects_duplicate_ids() {
    let entry = |id| TocEntry {
        id,
        title: "x".to_string(),
        patch: None,
        meta: TreeNodeMeta {
            words: 0,
            range: None,
        },
        children: vec![],
    };
    let snapshot = vec![entry(NodeId(0)), entry(NodeId(0))];
    assert!(matches!(
        TocRoot::try_from(snapshot),
        Err(TocError::InvalidSnapshot(_))
    ));
}

#[test]
fn test_try_from_snapshot_rejects_inverted_range() {
    let snapshot = vec![TocEntry {
        id: NodeId(0),
        title: "x".to_string(),
        patch: None,
        meta: TreeNodeMeta {
            words: 0,
            range: Some(TextRange { start: 10, end: 5 }),
        },
        children: vec![],
    }];
    assert!(matches!(
        TocRoot::try_from(snapshot),
        Err(TocError::InvalidSnapshot(_))
    ));
}

#[test]
fn test_builder_simple_stream() {
    let mut builder = TocBuilder::new();
    let a = builder
        .push(TocEvent {
            level: 1,
            title: "a".to_string(),
            range: Some(range(0, 10)),
        })
        .unwrap();
    let b = builder
        .push(TocEvent {
            level: 2,
            title: "b".to_string(),
            range: Some(range(0, 5)),
        })
        .unwrap();
    let root = builder.build();
    assert_eq!(root.children, vec![a]);
    assert_eq!(root.get(a).unwrap().children, vec![b]);
    assert_eq!(root.get(b).unwrap().parent, Some(a));
    assert_eq!(root.level(b), Some(2));
}

#[test]
fn test_builder_level_jump_creates_containers() {
    let mut builder = TocBuilder::new();
    let leaf = builder
        .push(TocEvent {
            level: 3,
            title: "c1".to_string(),
            range: Some(range(0, 10)),
        })
        .unwrap();
    let root = builder.build();

    // Two anonymous container nodes are auto-created for levels 1 and 2.
    let l1 = root.children[0];
    let l2 = root.get(l1).unwrap().children[0];
    assert_eq!(root.get(l1).unwrap().title, "");
    assert_eq!(root.get(l2).unwrap().title, "");
    assert_eq!(root.get(l2).unwrap().children, vec![leaf]);
    assert_eq!(root.level(leaf), Some(3));
    // Container ranges are backfilled from the leaf by default.
    assert_eq!(root.get(l1).unwrap().meta.range, Some(range(0, 10)));
    assert_eq!(root.get(l2).unwrap().meta.range, Some(range(0, 10)));
}

#[test]
fn test_builder_level_fallback_pops_stack() {
    let mut builder = TocBuilder::new();
    let a = builder
        .push(TocEvent {
            level: 1,
            title: "a".to_string(),
            range: None,
        })
        .unwrap();
    let b = builder
        .push(TocEvent {
            level: 2,
            title: "b".to_string(),
            range: None,
        })
        .unwrap();
    let c = builder
        .push(TocEvent {
            level: 1,
            title: "c".to_string(),
            range: None,
        })
        .unwrap();
    let root = builder.build();
    assert_eq!(root.children, vec![a, c]);
    assert_eq!(root.get(a).unwrap().children, vec![b]);
    assert_eq!(root.get(c).unwrap().parent, None);
    assert_eq!(root.level(c), Some(1));
}

#[test]
fn test_builder_backfills_container_range() {
    let mut builder = TocBuilder::new();
    builder
        .push(TocEvent {
            level: 1,
            title: "part".to_string(),
            range: None,
        })
        .unwrap();
    builder
        .push(TocEvent {
            level: 2,
            title: "c1".to_string(),
            range: Some(range(0, 10)),
        })
        .unwrap();
    builder
        .push(TocEvent {
            level: 2,
            title: "c2".to_string(),
            range: Some(range(10, 25)),
        })
        .unwrap();
    let root = builder.build();
    let part = root.children[0];
    assert_eq!(root.get(part).unwrap().meta.range, Some(range(0, 25)));
}

#[test]
fn test_builder_backfill_can_be_disabled() {
    let mut builder = TocBuilder::with_options(BuilderOptions {
        backfill_container_range: false,
    });
    builder
        .push(TocEvent {
            level: 1,
            title: "part".to_string(),
            range: None,
        })
        .unwrap();
    builder
        .push(TocEvent {
            level: 2,
            title: "c1".to_string(),
            range: Some(range(0, 10)),
        })
        .unwrap();
    let root = builder.build();
    let part = root.children[0];
    assert_eq!(root.get(part).unwrap().meta.range, None);
}
