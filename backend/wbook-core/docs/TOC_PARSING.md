# TOC parsing in wbook-core

Callers choose one TOC parser and pass it a decoded, immutable `ParsedContent`.
`TocParserConfig::build()` validates serializable configuration and returns
`Result<Box<dyn TocParser>, TocConfigError>`. The resulting parser returns
`Result<TocRoot, ParserError>` from `parse`. No combined-parser orchestration is
required. A caller that explicitly selects a parser can call `parse` directly;
`accept` is only a bounded confidence hint, not a prerequisite.

## Shared heading rules

`HeadingRuleConfig` is independent of TOC depth. Both the level parser and the
VBook parser reuse it:

- `Simple` matches literal prefix alternatives, a numeral, literal suffix
  alternatives, and an optional title.
- `Regex` applies a Rust `regex` expression to one line. Anchor the pattern when
  a whole-line match is needed. `title_group: None` retains the trimmed line;
  `Some(n)` explicitly selects capture group `n`. An absent or empty selected
  capture does not produce a heading. Ordinary parentheses do not change titles.

Simple affixes are lists of complete strings, not regex syntax or slash-delimited
text. `["章", "回"]` matches either suffix; `["正文第"]` matches a complete prefix.
The longest matching alternative wins. An empty list means no affix. Empty
strings and line breaks inside alternatives are invalid.

Numerals support Arabic, Chinese, or mixed forms. A contiguous numeral run is
checked before accepting the title: excess digits cannot spill into the title.
For example, a maximum of one numeral character rejects `卷十二 风起` while
accepting `卷一风起`. Bare forms with ambiguous numeral characters at the start
of a title should use a separator or a custom regex.

`min_numeral_len` must be positive; `max_numeral_len: None` means unbounded.
Equal minimum and maximum values require an exact width. Title length counts
Unicode scalar values after the suffix, excluding surrounding whitespace and
the prefix/numeral/suffix themselves. `allow_leading_space` controls indentation;
trailing whitespace is ignored.

## Level rules

`TocParserConfig::Levels(TocRulesConfig)` builds a `RuleSetTocParser`.

Each `LevelRulesConfig` has a positive, unique level and an ordered list of
heading rules. Levels are evaluated from shallowest to deepest; rules within
one level keep their configured order. The first matching rule wins. Thus a
line matching both volume and chapter rules becomes a volume.

This borrows calibre's separate rules for each TOC level, adapted to plain
text instead of XPath over HTML. It is not an implementation of calibre's TXT
heuristics. WBook retains its existing `TocBuilder` behavior: skipped levels
create anonymous ancestors. In particular, a level-2 heading before any
level-1 heading receives an anonymous parent. VBook grouping intentionally
handles that case differently.

Presets return independent, editable configurations:

| Factory | Initial behavior |
| --- | --- |
| `chapter_only_config()` | Chapters and extra headings at level 1 |
| `volume_and_chapter_config()` | Volumes/extras at level 1, chapters at level 2 |
| `easy_pub_config()` | EasyPub-style simple rules plus extra headings, all at level 1 |
| `vbook_config()` | VBook-style chapter rules and normal volume detection, fallback every 50 chapters |

The volume presets also recognize `卷一`, `卷三`, and `部一`; numbering need not
be consecutive. `easy_pub_config()` is a rule preset, not an importer for
EasyPub's XML files. Length splitting is selected separately.

A pure custom configuration contains only the rules the caller supplies:

```rust
use wbook_core::parser::toc::{
    HeadingRuleConfig, LevelRulesConfig, PatternRuleConfig,
    TocParserConfig, TocRulesConfig,
};

let config = TocParserConfig::Levels(TocRulesConfig {
    levels: vec![
        LevelRulesConfig {
            level: 1,
            rules: vec![HeadingRuleConfig::Regex(PatternRuleConfig {
                pattern: r"^卷[一二三四五六七八九十]+.*$".into(),
                title_group: None,
            })],
        },
        LevelRulesConfig {
            level: 2,
            rules: vec![HeadingRuleConfig::Regex(PatternRuleConfig {
                pattern: r"^第[0-9]+章.*$".into(),
                title_group: None,
            })],
        },
    ],
});
let parser = config.build()?;
```

No preset rules are implicitly added to custom configurations.

## VBook modes

`TocParserConfig::VBook(VBookConfig)` builds a `VBookTocParser`.

### Chapters

`ChapterMode::Rules` takes an ordered list of shared heading rules.
`chapter_rules()` provides text headings; `digit_chapter_rule()` supplies an
Arabic-number-first rule without a fixed digit width. Additional regex rules
can be appended or can replace the list entirely.

VBook's numeric-width setting is an exact width, not a maximum. Its UI value
zero maps to `min_numeral_len = 1, max_numeral_len = None`; three maps to
`min_numeral_len = 3, max_numeral_len = Some(3)`:

```rust
use wbook_core::parser::toc::{
    digit_chapter_rule, vbook_config, ChapterMode,
    HeadingRuleConfig, TocParserConfig, VolumeMode,
};

let mut config = vbook_config();
let mut digits = digit_chapter_rule();
digits.min_numeral_len = 3;
digits.max_numeral_len = Some(3);
digits.suffixes = vec!["：".into()];
config.chapters = ChapterMode::Rules(vec![HeadingRuleConfig::Simple(digits)]);
config.volumes = VolumeMode::Forced { chapters_per_volume: 50 };
let parser = TocParserConfig::VBook(config).build()?;
```

`ChapterMode::EndMarker { marker }` treats an exact, trimmed marker line as
the end of a chapter. WBook names each segment from its first nonempty line.
Leading/consecutive markers and empty segments create no chapters. The final
nonempty segment is retained even without a closing marker. The source,
including markers, is not rewritten.

### Volumes

| Mode | Behavior |
| --- | --- |
| `None` | All headings selected by the chapter rules stay at the root |
| `Normal` | Detect standalone volume headings; attach later chapters to the current volume |
| `FromChapterTitles` | Split a volume prefix and chapter title from the same line |
| `Forced` | Group every configured number of detected chapters under a generated volume |

Normal mode uses `fallback_chapters_per_volume` only if **no volume headings
exist anywhere in the text**. `None` disables the fallback. Chapters before the
first volume stay at the root; explicit empty volumes are retained. A volume
rule wins when both rule sets match the same source line.

`FromChapterTitles` uses volume rules on the prefix and chapter rules on the
remainder. It selects the first character boundary where both match, ignoring
whitespace between the two titles. Consecutive identical volume titles share
a parent:

```text
第一卷 风起 第1章 开始
第一卷 风起 第2章 继续
卷三 云动 第3章 归来
```

produces `第一卷 风起 -> [第1章 开始, 第2章 继续]` and
`卷三 云动 -> [第3章 归来]`. Standalone chapter titles continue under the current
volume. Chapter rules should be selective enough to avoid ambiguous split
points. Regex title captures affect display names and volume identity.

This mode requires chapter heading rules; combining it with an end marker is
a configuration error because a delimiter cannot identify the inline chapter
title boundary. End markers support the other volume modes.

Forced mode counts entries selected by the chapter rules, including any
additional headings those rules recognize. Standalone volume text is not
detected separately in this mode. The last group may be shorter; empty input
does not create synthetic volumes. Sizes of zero are configuration errors.

These VBook mode meanings were checked against the embedded help text in
VBook 3.5.1.1. Marker naming, whitespace handling and match precedence above
are explicit WBook contracts, not claims of byte-for-byte VBook compatibility.

## Length splitting and offsets

`TocParserConfig::SplitEvenly { parts }` builds an independent parser. It splits
the decoded text into chunks whose Unicode scalar counts differ by at most
one, with larger chunks first. At most one chunk per character is produced.
It does not align to paragraphs or infer headings. Zero parts returns a
configuration error, rather than panicking.

All source positions are half-open UTF-8 byte ranges in the original decoded
text. CRLF counts toward offsets; the line terminator is excluded from a
detected heading range.

A heading parser stores the heading's source span, not the entire chapter
body. Inline volume and chapter titles refer to their separate source
subspans. A length-split entry instead refers to its complete chunk because
its title is generated. Synthetic grouping volumes start with no source range;
`TocBuilder` backfills the union of child ranges. None of these ranges is a
promise that a volume contains all of its body text: future export code must
derive content boundaries from entry starts and document length, accounting
for container entries.

## Cancellation and errors

`ParserError::Cancelled` is distinct from a parse failure or `NoMatch`.
Scanning, grouping, tree construction, filtering, metadata parsing and
length splitting check cancellation, including empty inputs. `scan_lines`
returns `Result<Vec<TocEvent>, ParserError>` so cancellation cannot return a
successful partial event list. The existing `CombinedParser` also propagates
cancellation without trying another parser.

Invalid configuration fails when building the parser: malformed regex,
nonexistent title captures, zero/duplicate levels, invalid numeral bounds,
zero group/split sizes and empty/multiline markers are rejected.

## Compatibility with the earlier core draft

This changes the draft configuration API: the separate `simple`/`patterns`
lists become `levels -> rules`; simple rules no longer carry a level and
affixes are literal string lists. Regex title capture selection is explicit.
`SplitEvenlyParser::new` and `scan_lines` now return `Result`. There are no
application call sites or persisted configuration migrations in this change.

## References

- calibre's per-level TOC construction:
  <https://github.com/kovidgoyal/calibre/blob/master/src/calibre/ebooks/oeb/transforms/structure.py>
- VBook 3.5.1.1 embedded help: volume modes, chapter modes, numeric width,
  literal prefixes and the no-volume fallback.
- EasyPub's chapter settings supplied with the requirement: simple rules,
  extra regex, pure regex and equal-length splitting.
