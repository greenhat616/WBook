# WBook Core

WBook Core 应该暴露一个简单的接口，供外部 GUI 或是 Cli 使用。

## 接口

GUI 调用应该挂载 `Wbook` 实例上，可以考虑通过 trait 来方便外部对其进行测试。

## 文本处理大前提

- 文本可能很大，几十兆、几百兆都有可能。当前阶段**不考虑基于文件流的处理**，先按"内存中只有一份提取（解码后）的文本"来做。
- 解码后的原文保持**不可变**，由 `TextDocument` 接管所有权。当前文本由原文、新增缓冲区和片段表共同表示；各阶段读取当前版本的 `TextView`，不按阶段复制全文。跨片段的逻辑行、有限前缀和显式预览区间允许局部物化。
- 对外文本范围使用所属文档版本中逻辑文本的 UTF-8 **字节偏移**（`u64`，半开区间 `[start, end)`）。片段内部范围是缓冲区局部坐标，两者不混用。当前逻辑范围也不等于原文或输入文件的编码字节范围。
- 正文编辑统一为 `TextEdit { range, insert }`，通过带文档身份与版本的 `EditBatch` 原子提交。同批范围引用批次开始时的文本，下一批读取提交后的结果。提交立即更新逻辑视图，全文拼接仍可推迟。新增长文本可能显著增加内存，不承诺严格常数额外开销。不引入日记（Journal）/ 撤销机制。

## 核心

### Extractor

提取器用于接受输入路径，读取解析文件内容，并将其解析为中间表示，以供后续的解析器使用。

如 Simple 提取器，应该优先读取 Bom 头，以确定文件的编码方式，如无 Bom 头，再尝试推测编码方式（最后进行 Utf-8），如都不命中，则默认以系统编码方式进行解析。

### Parser

解析器用于接受提取器生成的中间表示，并根据特定的规则对其进行处理，生成最终的文档结构或其他所需的输出。

Parsers read a fixed current `TextView` and use separate category traits with the following shared shape:

- `name()`：解析器名称。
- `accept(content) -> MatchConfidence`：是否接受该内容，及其置信度（仿 calibre 输入插件选择，`0` 表示不接受）。
- `kind() -> ParserKind`：解析器类别（Filter / Toc / Metadata）。
- Each category's `parse(ct, content)` returns `Result<Output, ParserError>`:
  `TocRoot` for TOC, `Vec<TextEdit>` for filters, and `Metadata` for metadata.
  Cancellation is reported as `ParserError::Cancelled`.

目前应该包含的有：

- Filter —— 用于过滤处理不需要的内容，如广告文本。`Output = Vec<TextEdit>`（如对广告行区间替换为空字符串）。
- TOC parsers return a `TocRoot`. Internally they produce `TocEvent` values
  (level, title, source range) and assemble them with `TocBuilder`. Shared
  heading rules are independent of level. The level parser assigns rules to
  explicit levels; the VBook parser applies its own volume grouping policy.
  See [TOC parsing](TOC_PARSING.md) for configuration, presets and examples.
- Metadata —— 用于提取文档的元信息，如标题、作者、创建日期等。`Output` 为键值元信息。

The caller selects one `TocParser`, either directly or through
`TocParserConfig::build()`: Extractor -> TextDocument -> ordered filters -> selected parser -> Tweak.
The existing `CombinedParser` remains optional; its merge strategy is reserved
and is not required by the presets.

#### 类型约定（TOC 与 Parser）

- `NodeId`：TOC 节点的强类型 id（`toc/id.rs`），内部是 slab key，`serde(transparent)` 零成本序列化。注意 slab 会复用被移除节点的 key，remove 后持有的旧 `NodeId` 可能命中新节点；如未来需要可换 `generational-arena`。
- `TextRange`（`types/range.rs`）：统一偏移区间，`u64`，表示指定文档版本中当前文本的字节偏移（见「文本处理大前提」）。`TreeNodeMeta` 为 `{ words: u64, range: Option<TextRange> }`，`range` 为 `None` 表示纯容器节点（对齐 calibre 中 src 指向首个子节点的目录项）。
- `TextEdit` / `EditBatch`：Filter 和 ContentAdjust 的统一正文编辑表达；不再使用 `TransformOperation` 或节点字符串 `patch`。旧快照的 patch 缺失或为 null 可读取，任何其他值明确报错，避免静默丢失修改。新快照不输出该字段。
- 序列化 wire 格式为 `TocSnapshot = Vec<TocEntry>`（`toc/entry.rs`，对齐 calibre "TOC entry" 术语）。`TocRoot` 通过 `#[serde(try_from = "TocSnapshot", into = "TocSnapshot")]` 双向走 derive，反序列化时重建 `parent` 弱引用并校验 id 唯一性与 range 合法性（失败返回 `TocError::InvalidSnapshot`）。`parent` 字段不进入 wire 格式。
- TOC levels are 1-based. The level parser borrows calibre's per-level rule
  selection, but WBook's builder inserts anonymous ancestors for level gaps.
  `TocParser` returns the assembled tree, not a public event stream.
  `build()` backfills container ranges from children. Heading ranges locate
  source headings; length-split entries locate full chunks. Neither implies
  that a detected heading's range covers its entire body. Levels remain
  derived from parent links and export order from tree traversal.

### Tweak

Tweak 阶段介于解析器和最终输出之间，主要用于对解析结果进行微调和优化，以满足特定的需求或提高文档的可读性。Tweak 统一描述为"对 Parser 输出的再加工"，同样只操作偏移量与结构，不触碰文本本体。

目前可能包含的 Tweak 有：

- TocAdjust - 对生成的目录结构进行调整，如合并相邻的同级目录项
- MetadataEnhance - 对提取的元信息进行补充和修正，如自动填充缺失的作者信息
- ContentAdjust - 对文档内容进行调整，如修正格式错误或优化排版，产出并提交当前版本上的 `EditBatch`

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

Session 管线的契约见 [Session spec](specs/2026-10-04-session-pipeline/requirements.md)：同步 Workspace 持有领域状态与规则，Session 运行时只负责准入、取消、执行、快照与关闭；持久化保存与重启恢复仍是 TODO。

2026-10-04 实施事实：`Wbook::new(Params)` 从当前 tokio runtime 取得 Handle，持有 `SessionManager`；通过 `session_manager()` 调用 `create / open / get / list / close / shutdown`。`create` 只验证配置，不读取输入；`open(Workspace)` 是注册的基本入口。同步 Workspace 集中持有正文、已安装结果、overrides、过滤进度与 Revision；持久字段只在提交点修改，预览与清理告警属于临时状态。WorkspaceState 未派生 Serialize，尚无保存格式。

`SessionHandle` 的 initialize、parse、install、apply_edits、set_metadata_overrides、read_text、read_results、render_preview、export_epub 同步准入，成功返回可 await 的独立 `Receipt<T>`。同一 Session 一次只接受一个操作，冲突返回 Busy；多个 Session 可并行。阻塞工作线程独占移入的 Workspace，完成后归还，包括业务失败；正常返回的操作结果携带实际 Revision、保留类别的错误和清理告警。panic 使工作区成为 Lost，完成凭据并拒绝后续数据操作，查询与关闭仍可用；其副作用未知，Revision 只能报告最后已知值。快照通过 `snapshot / subscribe` 访问，只含状态、阶段、版本与最近一次操作摘要，不含全文、目录或时间线。

生命周期为 Open → Closing → Closed，解析或导出完成不关闭 Session。`cancel(OperationId)` 只取消匹配的活动操作，最终是否取消由底层错误决定；已提交的前缀与已发布 EPUB 不回滚。正文编辑使旧结果 Stale 并关闭预览，parse 不安装结果，install 验证版本和范围。相同 ExportOptions 复用预览，导出独立渲染且不覆盖既有目标。

`close().await` 禁止新操作、请求取消、等待真实工作线程返回后清理 Workspace；并发关闭共享同一个 `Arc<CloseReport>`，丢弃等待不终止关闭。清理告警不会覆写成功结果。关闭后 Manager 移除注册项，旧 Handle 可查询终态。`Wbook::shutdown().await` 先禁止创建并请求全部关闭，再等待并汇总；Drop 只尽力请求停止，不能替代有序 shutdown。关闭只清理所拥有的临时资源，不删除输入或已发布产物。

公开 API 全链示例与取消测量只通过 Wbook / Handle 操作，使用生成的临时文件并在结束时清理：

```powershell
cargo run --manifest-path backend/Cargo.toml -p wbook-core --example session_pipeline
cargo run --manifest-path backend/Cargo.toml -p wbook-core --release --example session_latency
```

测量、自动化场景和已知限制见 [Session 验证记录](specs/2026-10-04-session-pipeline/verification.md)。文件读取、解码及不可中断库调用会推迟取消响应，不承诺固定上限。打开文档数量决定总正文内存，Session 不额外累积全文或操作历史。GUI、HTTP/WebSocket、Tauri 退出事件与持久化保存、自动保存、重启恢复仍未接入；`Params.data_dir` 本期不使用。
