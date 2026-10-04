use std::borrow::Cow;
use std::io::{self, Write};
use std::time::Instant;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use wbook_core::document::{EditBatch, TextDocument, TextEdit};
use wbook_core::extractor::{Content, Encoding, ParsedContent};
use wbook_core::types::TextRange;

fn edits(len: usize, workload: &str, replacement: &str) -> Vec<TextEdit> {
    if workload == "longline" {
        return vec![TextEdit {
            range: TextRange {
                start: (len / 2) as u64,
                end: (len / 2) as u64,
            },
            insert: "!".into(),
        }];
    }
    let step = if workload == "dense" { 256 } else { 256 * 256 };
    (0..len)
        .step_by(step)
        .map(|start| TextEdit {
            range: TextRange {
                start: start as u64,
                end: (start + 256) as u64,
            },
            insert: if workload == "sparse" {
                String::new()
            } else {
                replacement.into()
            },
        })
        .collect()
}

fn replace(text: &str, edits: &[TextEdit]) -> String {
    let mut output = String::with_capacity(text.len() + 1);
    let mut end = 0;
    for edit in edits {
        output.push_str(&text[end..edit.range.start as usize]);
        output.push_str(&edit.insert);
        end = edit.range.end as usize;
    }
    output.push_str(&text[end..]);
    output
}

fn digest(hash: &mut u64, text: &str) {
    for byte in text.bytes() {
        *hash = (*hash ^ byte as u64).wrapping_mul(1099511628211);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).ok_or("expected pieces or string")?;
    let workload = args
        .get(2)
        .ok_or("expected sparse, dense, repeated, or longline")?;
    let mib: usize = args.get(3).ok_or("expected size in MiB")?.parse()?;
    if !["pieces", "string"].contains(&mode.as_str())
        || !["sparse", "dense", "repeated", "longline"].contains(&workload.as_str())
        || mib == 0
    {
        return Err("invalid benchmark arguments".into());
    }
    let mut line = "正文 abc 0123456789 ".repeat(8);
    line.extend(std::iter::repeat_n('x', 255 - line.len()));
    line.push(if workload == "longline" { ' ' } else { '\n' });
    let source = line.repeat(mib * 1024 * 1024 / 256);
    let replacement = format!("{}\n", "r".repeat(255));
    let alternate = format!("{}\n", "s".repeat(255));
    let rounds = if workload == "repeated" { 10 } else { 1 };
    let ct = CancellationToken::new();
    let start = Instant::now();
    let mut hash = 14695981039346656037u64;
    let mut count = 0u64;
    let mut edit_count = 0;
    let (edit_ms, scan_ms, added_bytes, pieces) = if mode == "pieces" {
        let mut doc = TextDocument::from(ParsedContent {
            encoding: Encoding {
                name: "UTF-8".into(),
                bom: false,
            },
            content: Content::Text(source),
            source_path: None,
        });
        for round in 0..rounds {
            let replacement = if round % 2 == 0 {
                &replacement
            } else {
                &alternate
            };
            let edits = edits(doc.view().len() as usize, workload, replacement);
            edit_count += edits.len();
            doc.apply(
                &ct,
                EditBatch {
                    base: doc.version(),
                    edits,
                },
            )?;
        }
        let edit_ms = start.elapsed().as_secs_f64() * 1000.0;
        let scan = Instant::now();
        for line in doc.view().lines(&ct) {
            let line = line?;
            digest(&mut hash, &line.raw);
            count += line.raw.len() as u64;
        }
        let stats = doc.stats();
        (
            edit_ms,
            scan.elapsed().as_secs_f64() * 1000.0,
            stats.added_bytes,
            stats.pieces,
        )
    } else {
        let mut current = Cow::Borrowed(source.as_str());
        for round in 0..rounds {
            let replacement = if round % 2 == 0 {
                &replacement
            } else {
                &alternate
            };
            let edits = edits(current.len(), workload, replacement);
            edit_count += edits.len();
            current = Cow::Owned(replace(&current, &edits));
        }
        let edit_ms = start.elapsed().as_secs_f64() * 1000.0;
        let scan = Instant::now();
        for line in current.split_inclusive('\n') {
            digest(&mut hash, line);
            count += line.len() as u64;
        }
        // Retain the source for the same immutable-original contract as TextDocument.
        std::hint::black_box(&source);
        (edit_ms, scan.elapsed().as_secs_f64() * 1000.0, 0, 0)
    };
    println!(
        "{}",
        json!({"mode": mode, "workload": workload, "mib": mib, "rounds": rounds, "edits": edit_count, "edit_ms": edit_ms, "scan_ms": scan_ms, "added_bytes": added_bytes, "pieces": pieces, "output_bytes": count, "hash": format!("{hash:016x}")})
    );
    io::stdout().flush()?;
    if args.get(4).is_some_and(|arg| arg == "hold") {
        // Let an external sampler query the OS lifetime peak before exit.
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
    }
    Ok(())
}
