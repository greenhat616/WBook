use super::*;

pub(crate) fn document(text: &str) -> TextDocument {
    ParsedContent {
        encoding: Encoding {
            name: "utf-8".into(),
            bom: false,
        },
        content: Content::Text(text.into()),
        source_path: None,
    }
    .into()
}

fn edit(start: u64, end: u64, insert: &str) -> TextEdit {
    TextEdit {
        range: TextRange { start, end },
        insert: insert.into(),
    }
}

fn text(doc: &TextDocument) -> String {
    let view = doc.view();
    view.read(
        &CancellationToken::new(),
        view.version(),
        TextRange {
            start: 0,
            end: view.len(),
        },
    )
    .unwrap()
    .into_owned()
}

fn apply(doc: &mut TextDocument, edits: Vec<TextEdit>) -> ChangeMap {
    doc.apply(
        &CancellationToken::new(),
        EditBatch {
            base: doc.version(),
            edits,
        },
    )
    .unwrap()
}

fn reference(text: &mut String, edits: &[TextEdit]) {
    let mut edits: Vec<_> = edits.iter().collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
    for edit in edits {
        text.replace_range(
            edit.range.start as usize..edit.range.end as usize,
            &edit.insert,
        );
    }
}

#[test]
fn edits_read_current_text_and_share_original_storage() {
    let mut doc = document("abc");
    let pointer = doc.buffers[0].as_ptr();
    apply(&mut doc, vec![edit(1, 1, "xy")]);
    assert_eq!(text(&doc), "axybc");
    apply(&mut doc, vec![edit(2, 3, "Z")]);
    assert_eq!(text(&doc), "axZbc");
    apply(&mut doc, vec![edit(0, 4, "中🙂")]);
    assert_eq!(text(&doc), "中🙂c");
    assert_eq!(doc.buffers[0], "abc");
    assert_eq!(doc.buffers[0].as_ptr(), pointer);
    assert_eq!(doc.version().revision(), 3);
}

#[test]
fn batch_offsets_are_all_relative_to_its_start() {
    let mut doc = document("abcdef");
    apply(&mut doc, vec![edit(4, 5, "XY"), edit(1, 3, "")]);
    assert_eq!(text(&doc), "adXYf");
    apply(&mut doc, vec![edit(1, 2, "D"), edit(2, 4, "!")]);
    assert_eq!(text(&doc), "aD!f");
}

#[test]
fn invalid_batches_are_atomic_even_after_candidate_pieces_exist() {
    let cases = [
        vec![edit(4, 3, "")],
        vec![edit(0, 20, "")],
        vec![edit(1, 1, "x")],
        vec![edit(0, 3, "a"), edit(4, 4, "b")],
        vec![edit(0, 3, "a"), edit(0, 3, "a")],
        vec![edit(0, 0, "a"), edit(0, 0, "b")],
        vec![edit(0, 3, "a"), edit(3, 3, "b")],
        vec![edit(3, 3, "a"), edit(3, 7, "b")],
        vec![edit(0, 7, "a"), edit(3, 3, "b")],
        vec![edit(1, 1, "")],
    ];
    for edits in cases {
        let mut doc = document("中🙂z");
        let batch = EditBatch {
            base: doc.version(),
            edits,
        };
        let wire = serde_json::to_string(&batch).unwrap();
        for batch in [batch, serde_json::from_str(&wire).unwrap()] {
            let version = doc.version();
            let pieces = doc.pieces.clone();
            assert!(doc.apply(&CancellationToken::new(), batch).is_err());
            assert_eq!(doc.version(), version);
            assert_eq!(doc.pieces, pieces);
            assert_eq!(doc.buffers, vec!["中🙂z"]);
        }
    }
}

#[test]
fn versions_noops_and_document_identity_are_enforced() {
    let mut doc = document("");
    let base = doc.version();
    apply(&mut doc, vec![]);
    apply(&mut doc, vec![edit(0, 0, "")]);
    assert_eq!(doc.version(), base);
    let wrong = document("").version();
    assert!(matches!(
        doc.apply(
            &CancellationToken::new(),
            EditBatch {
                base: wrong,
                edits: vec![]
            }
        ),
        Err(DocumentError::WrongDocument)
    ));
    apply(&mut doc, vec![edit(0, 0, "x")]);
    assert!(matches!(
        doc.apply(
            &CancellationToken::new(),
            EditBatch {
                base,
                edits: vec![]
            }
        ),
        Err(DocumentError::StaleVersion { .. })
    ));
    apply(&mut doc, vec![edit(0, 1, "x")]);
    assert_eq!(doc.version().revision(), 2);
    apply(&mut doc, vec![edit(0, 1, "")]);
    assert!(doc.view().is_empty());
}

#[test]
fn every_prepublication_cancellation_preserves_all_document_state() {
    let mut checks = 0;
    let mut probe = document("abcdef");
    let edits = vec![edit(1, 2, "中"), edit(4, 5, "🙂")];
    probe
        .apply_checked(
            EditBatch {
                base: probe.version(),
                edits: edits.clone(),
            },
            || {
                checks += 1;
                Ok(())
            },
        )
        .unwrap();
    for cancel_at in 1..=checks {
        let mut doc = document("abcdef");
        let before = doc.version();
        let pieces = doc.pieces.clone();
        let mut count = 0;
        let result = doc.apply_checked(
            EditBatch {
                base: before,
                edits: edits.clone(),
            },
            || {
                count += 1;
                if count == cancel_at {
                    Err(DocumentError::Cancelled)
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(result, Err(DocumentError::Cancelled)));
        assert_eq!(doc.version(), before);
        assert_eq!(doc.pieces, pieces);
        assert_eq!(doc.buffers, vec!["abcdef"]);
    }
}

#[test]
fn lines_prefix_and_output_ignore_piece_boundaries() {
    let mut doc = document("第章\r\n\n末尾");
    apply(&mut doc, vec![edit(3, 3, "一"), edit(7, 7, "\n新行\r")]);
    let expected = "第一章\r\n新行\r\n\n末尾";
    assert_eq!(text(&doc), expected);
    let ct = CancellationToken::new();
    let view = doc.view();
    let lines: Vec<_> = view
        .lines(&ct)
        .map(|line| {
            let line = line.unwrap();
            (line.range, line.text().to_string(), line.raw.into_owned())
        })
        .collect();
    assert_eq!(
        lines
            .iter()
            .map(|(_, text, _)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["第一章", "新行", "", "末尾"]
    );
    let mut output = Vec::new();
    for (range, _, raw) in lines {
        assert_eq!(view.read(&ct, view.version(), range).unwrap(), raw);
        view.write_range(&ct, view.version(), range, &mut output)
            .unwrap();
    }
    assert_eq!(output, expected.as_bytes());
    assert_eq!(view.prefix(&ct, 2).unwrap(), "第一");
    let chars = view
        .char_indices(&ct)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        chars,
        expected
            .char_indices()
            .map(|(p, c)| (p as u64, c))
            .collect::<Vec<_>>()
    );
}

#[test]
fn positions_map_with_affinity_and_report_removed_interiors() {
    let mut doc = document("abcdef");
    let positions: Vec<_> = (0..=6).map(|p| doc.view().position(p).unwrap()).collect();
    let map = apply(&mut doc, vec![edit(1, 1, "XY"), edit(3, 5, "!")]);
    let expected = [Some(0), Some(1), Some(4), Some(5), None, Some(6), Some(7)];
    for (position, expected) in positions.iter().zip(expected) {
        assert_eq!(
            map.map(*position, Affinity::Before)
                .unwrap()
                .map(TextPosition::offset),
            expected
        );
    }
    let inserted_end = map.map(positions[1], Affinity::After).unwrap().unwrap();
    assert_eq!(inserted_end.offset(), 3);
    assert!(map
        .touches(map.before(), TextRange { start: 3, end: 5 })
        .unwrap());
    let next = apply(&mut doc, vec![edit(0, 1, "")]);
    assert_eq!(
        next.map(inserted_end, Affinity::After)
            .unwrap()
            .unwrap()
            .offset(),
        2
    );
    assert!(matches!(
        next.map(positions[0], Affinity::Before),
        Err(DocumentError::StaleVersion { .. })
    ));
}

#[test]
fn deterministic_unicode_edits_match_independent_string_reference() {
    let mut doc = document("甲🙂abc\r\n乙");
    let mut expected = text(&doc);
    let mut random = 47u64;
    for _ in 0..1000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let boundaries: Vec<_> = expected
            .char_indices()
            .map(|(p, _)| p)
            .chain(std::iter::once(expected.len()))
            .collect();
        let a = random as usize % boundaries.len();
        let b = (random >> 32) as usize % boundaries.len();
        let replacement = ["", "x", "中文", "🙂", "\r\n"][random as usize % 5];
        let edits = vec![edit(
            boundaries[a.min(b)] as u64,
            boundaries[a.max(b)] as u64,
            replacement,
        )];
        reference(&mut expected, &edits);
        apply(&mut doc, edits);
        assert_eq!(text(&doc), expected);
        for pair in doc.pieces.windows(2) {
            assert!(pair[0].buffer != pair[1].buffer || pair[0].range.end != pair[1].range.start);
        }
    }
}

#[test]
fn empty_and_long_reads_honor_cancellation_and_writer_failure() {
    use std::io::{self, Write};

    struct InterruptedWriter<'a> {
        ct: &'a CancellationToken,
        output: Vec<u8>,
        fail: bool,
    }
    impl Write for InterruptedWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.output.is_empty() && self.fail {
                return Err(io::Error::other("intentional writer failure"));
            }
            self.output.extend_from_slice(bytes);
            if !self.fail {
                self.ct.cancel();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    for fail in [false, true] {
        let doc = document(&"中🙂".repeat(10000));
        let version = doc.version();
        let ct = CancellationToken::new();
        let mut writer = InterruptedWriter {
            ct: &ct,
            output: vec![],
            fail,
        };
        let result = doc.view().write_range(
            &ct,
            version,
            TextRange {
                start: 0,
                end: doc.view().len(),
            },
            &mut writer,
        );
        if fail {
            assert!(matches!(result, Err(DocumentError::Io { source: _ })));
        } else {
            assert!(matches!(result, Err(DocumentError::Cancelled)));
        }
        assert!(!writer.output.is_empty());
        assert!(writer.output.len() < doc.view().len() as usize);
        assert!(std::str::from_utf8(&writer.output).is_ok());
        assert_eq!(doc.version(), version);
        assert_eq!(doc.buffers.len(), 1);
    }
    for source in ["", "text"] {
        let mut doc = document(source);
        let ct = CancellationToken::new();
        ct.cancel();
        let view = doc.view();
        assert!(matches!(
            view.lines(&ct).next(),
            Some(Err(DocumentError::Cancelled))
        ));
        assert!(matches!(
            view.char_indices(&ct).next(),
            Some(Err(DocumentError::Cancelled))
        ));
        assert!(matches!(view.prefix(&ct, 0), Err(DocumentError::Cancelled)));
        assert!(matches!(
            view.read(&ct, view.version(), TextRange { start: 0, end: 0 }),
            Err(DocumentError::Cancelled)
        ));
        assert!(matches!(
            doc.apply(
                &ct,
                EditBatch {
                    base: doc.version(),
                    edits: vec![]
                }
            ),
            Err(DocumentError::Cancelled)
        ));
    }
}

#[test]
fn mapping_handles_adjacent_replacements_eof_and_collapsed_deletions() {
    let mut doc = document("abcd");
    let positions: Vec<_> = (0..=4).map(|p| doc.view().position(p).unwrap()).collect();
    let map = apply(&mut doc, vec![edit(0, 2, ""), edit(2, 4, "🙂")]);
    for (position, expected) in positions
        .into_iter()
        .zip([Some(0), None, Some(0), None, Some(4)])
    {
        assert_eq!(
            map.map(position, Affinity::After)
                .unwrap()
                .map(TextPosition::offset),
            expected
        );
    }
    assert!(doc.view().position(1).is_err());
    assert!(doc.view().position(5).is_err());
    let eof = doc.view().position(4).unwrap();
    let map = apply(&mut doc, vec![edit(4, 4, "中")]);
    assert_eq!(map.map(eof, Affinity::Before).unwrap().unwrap().offset(), 4);
    assert_eq!(map.map(eof, Affinity::After).unwrap().unwrap().offset(), 7);
}

#[test]
fn read_ranges_validate_version_order_bounds_and_character_boundaries() {
    let mut doc = document("中abc");
    let ct = CancellationToken::new();
    let version = doc.version();
    for range in [
        TextRange { start: 4, end: 3 },
        TextRange { start: 0, end: 99 },
        TextRange { start: 1, end: 3 },
    ] {
        assert!(doc.view().read(&ct, version, range).is_err());
    }
    apply(&mut doc, vec![edit(3, 4, "!")]);
    assert!(matches!(
        doc.view()
            .read(&ct, version, TextRange { start: 0, end: 3 }),
        Err(DocumentError::StaleVersion { .. })
    ));
    assert!(matches!(
        doc.view()
            .read(&ct, document("").version(), TextRange { start: 0, end: 3 }),
        Err(DocumentError::WrongDocument)
    ));
}
