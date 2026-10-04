# WBook Core

WBook Core 应该暴露一个简单的接口，供外部 GUI 或是 Cli 使用。

## 接口

GUI 调用应该挂载 `Wbook` 实例上，可以考虑通过 trait 来方便外部对其进行测试。

## 文本处理大前提

* 文本可能很大，几十兆、几百兆都有可能。当前阶段**不考虑基于文件流的处理**，先按"内存中只有一份提取（解码后）的文本"来做。
* 解码后的原文保持**不可变**，由 `TextDocument` 接管所有权。当前文本由原文、新增缓冲区和片段表共同表示；各阶段读取当前版本的 `TextView`，不按阶段复制全文。跨片段的逻辑行、有限前缀和显式预览区间允许局部物化。
* 对外文本范围使用所属文档版本中逻辑文本的 UTF-8 **字节偏移**（`u64`，半开区间 `[start, end)`）。片段内部范围是缓冲区局部坐标，两者不混用。当前逻辑范围也不等于原文或输入文件的编码字节范围。
* 正文编辑统一为 `TextEdit { range, insert }`，通过带文档身份与版本的 `EditBatch` 原子提交。同批范围引用批次开始时的文本，下一批读取提交后的结果。提交立即更新逻辑视图，全文拼接仍可推迟。新增长文本可能显著增加内存，不承诺严格常数额外开销。不引入日记（Journal）/ 撤销机制。

## 核心

### Extractor

提取器用于接受输入路径，读取解析文件内容，并将其解析为中间表示，以供后续的解析器使用。

如 Simple 提取器，应该优先读取 Bom 头，以确定文件的编码方式，如无 Bom 头，再尝试推测编码方式（最后进行 Utf-8），如都不命中，则默认以系统编码方式进行解析。

### Parser

解析器用于接受提取器生成的中间表示，并根据特定的规则对其进行处理，生成最终的文档结构或其他所需的输出。

Parsers read a fixed current `TextView` and use separate category traits with the following shared shape:

* `name()`：解析器名称。
* `accept(content) -> MatchConfidence`：是否接受该内容，及其置信度（仿 calibre 输入插件选择，`0` 表示不接受）。
* `kind() -> ParserKind`：解析器类别（Filter / Toc / Metadata）。
* Each category's `parse(ct, content)` returns `Result<Output, ParserError>`:
  `TocRoot` for TOC, `Vec<TextEdit>` for filters, and `Metadata` for metadata.
  Cancellation is reported as `ParserError::Cancelled`.

目前应该包含的有：

* Filter —— 用于过滤处理不需要的内容，如广告文本。`Output = Vec<TextEdit>`（如对广告行区间替换为空字符串）。
* TOC parsers return a `TocRoot`. Internally they produce `TocEvent` values
  (level, title, source range) and assemble them with `TocBuilder`. Shared
  heading rules are independent of level. The level parser assigns rules to
  explicit levels; the VBook parser applies its own volume grouping policy.
  See [TOC parsing](TOC_PARSING.md) for configuration, presets and examples.
* Metadata —— 用于提取文档的元信息，如标题、作者、创建日期等。`Output` 为键值元信息。

The caller selects one `TocParser`, either directly or through
`TocParserConfig::build()`: Extractor -> TextDocument -> ordered filters -> selected parser -> Tweak.
The existing `CombinedParser` remains optional; its merge strategy is reserved
and is not required by the presets.

#### 类型约定（TOC 与 Parser）

* `NodeId`：TOC 节点的强类型 id（`toc/id.rs`），内部是 slab key，`serde(transparent)` 零成本序列化。注意 slab 会复用被移除节点的 key，remove 后持有的旧 `NodeId` 可能命中新节点；如未来需要可换 `generational-arena`。
* `TextRange`（`types/range.rs`）：统一偏移区间，`u64`，表示指定文档版本中当前文本的字节偏移（见「文本处理大前提」）。`TreeNodeMeta` 为 `{ words: u64, range: Option<TextRange> }`，`range` 为 `None` 表示纯容器节点（对齐 calibre 中 src 指向首个子节点的目录项）。
* `TextEdit` / `EditBatch`：Filter 和 ContentAdjust 的统一正文编辑表达；不再使用 `TransformOperation` 或节点字符串 `patch`。旧快照的 patch 缺失或为 null 可读取，任何其他值明确报错，避免静默丢失修改。新快照不输出该字段。
* 序列化 wire 格式为 `TocSnapshot = Vec<TocEntry>`（`toc/entry.rs`，对齐 calibre "TOC entry" 术语）。`TocRoot` 通过 `#[serde(try_from = "TocSnapshot", into = "TocSnapshot")]` 双向走 derive，反序列化时重建 `parent` 弱引用并校验 id 唯一性与 range 合法性（失败返回 `TocError::InvalidSnapshot`）。`parent` 字段不进入 wire 格式。
* TOC levels are 1-based. The level parser borrows calibre's per-level rule
  selection, but WBook's builder inserts anonymous ancestors for level gaps.
  `TocParser` returns the assembled tree, not a public event stream.
  `build()` backfills container ranges from children. Heading ranges locate
  source headings; length-split entries locate full chunks. Neither implies
  that a detected heading's range covers its entire body. Levels remain
  derived from parent links and export order from tree traversal.

### Tweak

Tweak 阶段介于解析器和最终输出之间，主要用于对解析结果进行微调和优化，以满足特定的需求或提高文档的可读性。Tweak 统一描述为"对 Parser 输出的再加工"，同样只操作偏移量与结构，不触碰文本本体。

目前可能包含的 Tweak 有：
* TocAdjust - 对生成的目录结构进行调整，如合并相邻的同级目录项
* MetadataEnhance - 对提取的元信息进行补充和修正，如自动填充缺失的作者信息
* ContentAdjust - 对文档内容进行调整，如修正格式错误或优化排版，产出并提交当前版本上的 `EditBatch`

### Current text and derived results

`ProcessingDocument` owns the document, installed parsing results and explicit metadata overrides. `run` executes the caller's ordered filters and returns a parsing candidate; `parse` returns a candidate without filtering. `install` checks the candidate's document version and ranges before replacing installed results. Reparsing never silently replaces a manually adjusted TOC.

Every committed nonempty batch changes the document revision. Previously installed TOC and automatic metadata remain accessible through `results()`, but `current_results()` rejects their stale version. Single-batch `ChangeMap` updates validated positions with explicit insertion affinity; positions inside a replaced or deleted span become invalid. Mapping positions does not certify TOC semantics.

Preview uses `TextView::read`; output uses `write_range` or a version-checked `ProcessingDocument` output plan. Body ranges must be supplied explicitly; a detected heading span is not a complete chapter. Both paths read the same current pieces and never replay chapter patches.

The Rust parser API now accepts `TextView` by value. Construct a document by moving `ParsedContent` into `TextDocument`, then pass `document.view()` to a parser. `TextOp`, `TransformOperation`, `ContentRange` and `ByteRange` have been removed. `TocNode` no longer deserializes directly; load entries through `TocSnapshot` so legacy patches are checked. `TocSnapshot` remains a structural format; a bare snapshot has no document-version validity guarantee.

The implementation and validation plan is recorded in [the transform pipeline spec](specs/2026-10-04-text-transform-pipeline/design.md). Arbitrary whole-document regex matching, automatic reconciliation of manually adjusted TOCs, piece trees and buffer garbage collection remain outside that change.

### Rendering and EPUB export

`export::export_epub` consumes an edited `ProcessingDocument` and explicit language / layout options. It plans sections from typed TOC ranges, renders embedded Tera XHTML templates to temporary resources, writes an EPUB 3 ZIP, validates its structure and links, then publishes without replacing an existing destination. `render_book` and `package_epub` can also be called separately so preview and packaging share the same resources. `RenderedBook` owns its temporary directory; callers can use `close()` to observe cleanup failures instead of relying on best-effort Drop.

`TreeNodeMeta.range_kind` distinguishes `Heading`, `Body`, `Container` and legacy `Unknown` ranges. Export rejects Unknown rather than guessing. Heading spans may be adjacent fragments of an inline VBook heading. A repeated inline volume prefix belongs to the following chapter's consumed heading span, keeping it out of the previous chapter body. Source positions determine body ownership; the edited TOC determines reading order.

Layouts are `SplitChapters` (default), `Paged` (one content file with section page-break hints), and `SingleHtml` (one continuous content file). All use EPUB-compatible XHTML and the same section anchors. Native `zip` replaces the unused `epub-builder` dependency because independent navigation and file-backed archive writing are needed. MOBI conversion remains a future stage consuming a validated EPUB artifact, with no Kindle runtime dependency for EPUB export.

See the [render/package spec](specs/2026-10-04-render-package-pipeline/design.md), [verification record](specs/2026-10-04-render-package-pipeline/verification.md), and `examples/render_epub.rs` for a runnable sample.

### Lifecycle

目前设计上支持多 Session 并行，每个 Session 独立管理其生命周期，包括创建、使用和销毁。这有助于在同一应用中同时处理多个文档或任务，提升系统的并发处理能力和资源利用效率。

每个 Session 的生命周期管理可以通过状态机来实现，确保在不同状态下的操作合法性。例如，只有在 Session 创建后才能进行文档解析，解析完成后才能进行 Tweak 操作，最终才能销毁 Session。
