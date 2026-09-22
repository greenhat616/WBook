# WBook Core

WBook Core 应该暴露一个简单的接口，供外部 GUI 或是 Cli 使用。

## 接口

GUI 调用应该挂载 `Wbook` 实例上，可以考虑通过 trait 来方便外部对其进行测试。

## 文本处理大前提

* 文本可能很大，几十兆、几百兆都有可能。当前阶段**不考虑基于文件流的处理**，先按"内存中只有一份提取（解码后）的文本"来做。
* 文本本体**不可变**。一切处理——Filter、Toc、Metadata、Tweak、拆分导出——都只通过**偏移量**引用文本，不复制内容。
* 内部统一偏移域：解码后 UTF-8 文本的**字节偏移**（`u64`，半开区间 `[start, end)`）。理由：`str` 切片 O(1)、regex 天然产出字节偏移、与 Rust 生态一致。字符偏移与原始文件字节偏移都不进入内部模型（对非 UTF-8 源文件，原始字节偏移本来也无法定位解码文本）。
* 文本修改统一采用**操作算子式**：`TextOp = Replace | Delete | Insert`，每个算子只携带偏移区间与少量替换文本。算子在解析 / Tweak 阶段只是"描述"，**物化（真正应用）推迟到拆分 / 导出时按区间逐块进行**，从而全程维持单份文本的内存前提。不引入日记（Journal）/ 撤销机制。

## 核心

### Extractor

提取器用于接受输入路径，读取解析文件内容，并将其解析为中间表示，以供后续的解析器使用。

如 Simple 提取器，应该优先读取 Bom 头，以确定文件的编码方式，如无 Bom 头，再尝试推测编码方式（最后进行 Utf-8），如都不命中，则默认以系统编码方式进行解析。

### Parser

解析器用于接受提取器生成的中间表示，并根据特定的规则对其进行处理，生成最终的文档结构或其他所需的输出。

所有解析器遵循统一的 trait 形态（目标设计，代码后续跟进）：

* `name()`：解析器名称。
* `accept(content) -> MatchConfidence`：是否接受该内容，及其置信度（仿 calibre 输入插件选择，`0` 表示不接受）。
* `kind() -> ParserKind`：解析器类别（Filter / Toc / Metadata）。
* `parse(ct, content) -> Result<Self::Output, ParserError>`：关联类型 `Output` 随类别而定。

目前应该包含的有：

* Filter —— 用于过滤处理不需要的内容，如广告文本。`Output = Vec<TextOp>`（如对广告区间的 Delete 算子）。
* Toc —— 用于生成文档的目录结构，方便用户快速导航文档内容。`Output` 为扁平的 `TocEvent`（level, title, range）事件流（对齐 calibre `--level1-toc`/`--level2-toc`/`--level3-toc` xpath 检测的输出，level 为 1-based），统一由 `TocBuilder` 组装成树。这让 `CombineStrategy::Merge` 自然成立：多 parser 各自产出事件流（如一级用"卷"正则、二级用"章"正则），按位置归并后过一次 `TocBuilder` 合并成一棵树。
* Metadata —— 用于提取文档的元信息，如标题、作者、创建日期等。`Output` 为键值元信息。

管线映射：Extractor → `CombinedParser`（`BestMatch` 策略按置信度降序尝试、失败回退；`Merge` 预留未实现）→ Tweak。

#### 类型约定（TOC 与 Parser）

* `NodeId`：TOC 节点的强类型 id（`toc/id.rs`），内部是 slab key，`serde(transparent)` 零成本序列化。注意 slab 会复用被移除节点的 key，remove 后持有的旧 `NodeId` 可能命中新节点；如未来需要可换 `generational-arena`。
* `TextRange`（`types/range.rs`）：统一偏移区间，`u64`，表示解码后文本的字节偏移（见「文本处理大前提」）。`TreeNodeMeta` 为 `{ words: u64, range: Option<TextRange> }`，`range` 为 `None` 表示纯容器节点（对齐 calibre 中 src 指向首个子节点的目录项）。
* `TextOp`：文本操作算子（`Replace { range, replacement }` / `Delete { range }` / `Insert { range, insertion }`），是 Filter、ContentAdjust 以及 `TocNode.patch` 的统一表达。`TocNode.patch` 即作用于该节点内容区间的 `TextOp` 集合，在文档拆分时物化。
* 序列化 wire 格式为 `TocSnapshot = Vec<TocEntry>`（`toc/entry.rs`，对齐 calibre "TOC entry" 术语）。`TocRoot` 通过 `#[serde(try_from = "TocSnapshot", into = "TocSnapshot")]` 双向走 derive，反序列化时重建 `parent` 弱引用并校验 id 唯一性与 range 合法性（失败返回 `TocError::InvalidSnapshot`）。`parent` 字段不进入 wire 格式。
* 层级对齐 calibre 约定：`TocParser` 产出扁平的 (level, title, position) 事件流，由 `TocBuilder` 组装成树——level 跳级时自动补匿名容器节点，`build()` 默认用 `TextRange::merge` 自底向上回填容器节点的 range。层级不冗余存储，`TocRoot::level(id)` 通过 parent 链上溯计算。`play_order`（NCX 用）不存储，未来导出时按 DFS 序计算。

### Tweak

Tweak 阶段介于解析器和最终输出之间，主要用于对解析结果进行微调和优化，以满足特定的需求或提高文档的可读性。Tweak 统一描述为"对 Parser 输出的再加工"，同样只操作偏移量与结构，不触碰文本本体。

目前可能包含的 Tweak 有：
* TocAdjust - 对生成的目录结构进行调整，如合并相邻的同级目录项
* MetadataEnhance - 对提取的元信息进行补充和修正，如自动填充缺失的作者信息
* ContentAdjust - 对文档内容进行调整，如修正格式错误或优化排版，产出 / 追加 `TextOp`

### Lifecycle

目前设计上支持多 Session 并行，每个 Session 独立管理其生命周期，包括创建、使用和销毁。这有助于在同一应用中同时处理多个文档或任务，提升系统的并发处理能力和资源利用效率。

每个 Session 的生命周期管理可以通过状态机来实现，确保在不同状态下的操作合法性。例如，只有在 Session 创建后才能进行文档解析，解析完成后才能进行 Tweak 操作，最终才能销毁 Session。
