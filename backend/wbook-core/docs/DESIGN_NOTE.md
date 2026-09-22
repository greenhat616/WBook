# WBook Core

WBook Core 应该暴露一个简单的接口，供外部 GUI 或是 Cli 使用。

## 接口

GUI 调用应该挂载 `Wbook` 实例上，可以考虑通过 trait 来方便外部对其进行测试。


## 核心

### Extracotr

提取器用于接受输入路径，读取解析文件内容，并将其解析为中间表示，以供后续的解析器使用。

如 Simple 提取器，应该优先读取 Bom 头，以确定文件的编码方式，如无 Bom 头，再尝试推测编码方式（最后进行 Utf-8），如都不命中，则默认以系统编码方式进行解析。


### Parser

解析器用于接受提取器生成的中间表示，并根据特定的规则对其进行处理，生成最终的文档结构或其他所需的输出。

目前应该包含的有：
* Filter - 用于过滤处理不需要的内容，如广告文本
* Toc - 用于生成文档的目录结构，方便用户快速导航文档内容
* Metadata - 用于提取文档的元信息，如标题、作者、创建日期等

#### 类型约定（TOC 与 Parser）

* `NodeId`：TOC 节点的强类型 id（`toc/id.rs`），内部是 slab key，`serde(transparent)` 零成本序列化。注意 slab 会复用被移除节点的 key，remove 后持有的旧 `NodeId` 可能命中新节点；如未来需要可换 `generational-arena`。
* `TextRange` / `ByteRange`（`types/range.rs`）：偏移区间统一使用 `u64`，分别是解码后文本的字符偏移与原始字节偏移。`TreeNodeMeta` 为 `{ words: u64, range: Option<TextRange> }`，`range` 为 `None` 表示纯容器节点（对齐 calibre 中 src 指向首个子节点的目录项）。
* 序列化 wire 格式为 `TocSnapshot = Vec<TocEntry>`（`toc/entry.rs`，对齐 calibre "TOC entry" 术语）。`TocRoot` 通过 `#[serde(try_from = "TocSnapshot", into = "TocSnapshot")]` 双向走 derive，反序列化时重建 `parent` 弱引用并校验 id 唯一性与 range 合法性（失败返回 `TocError::InvalidSnapshot`）。`parent` 字段不进入 wire 格式。
* 层级对齐 calibre 约定：`TocParser` 产出扁平的 (level, title, position) 事件流（类比 calibre `--level1-toc`/`--level2-toc`/`--level3-toc` xpath 检测的输出，level 为 1-based），由 `TocBuilder` 组装成树——level 跳级时自动补匿名容器节点，`build()` 默认用 `TextRange::merge` 自底向上回填容器节点的 range。层级不冗余存储，`TocRoot::level(id)` 通过 parent 链上溯计算。`play_order`（NCX 用）不存储，未来导出时按 DFS 序计算。
* 管线映射：Extractor → `CombinedParser`（多个 `TocParser`，`MatchConfidence` 仿 calibre 输入插件选择，`BestMatch` 策略按置信度降序尝试、失败回退）→ Tweak（TocAdjust）。`CombineStrategy::Merge` 预留未实现：未来多 parser 各自产出 `TocEvent` 流（如一级用"卷"正则、二级用"章"正则），经 `TocBuilder` 合并成一棵树。

### Tweak

Tweak 阶段介于解析器和最终输出之间，主要用于对解析结果进行微调和优化，以满足特定的需求或提高文档的可读性。例如，可以对生成的目录结构进行调整，或者对提取的元信息进行补充和修正。

目前可能包含的 Tweak 有：
* TocAdjust - 对生成的目录结构进行调整，如合并相邻的同级目录项
* MetadataEnhance - 对提取的元信息进行补充和修正，如自动填充缺失的作者信息
* ContentAdjust - 对文档内容进行调整，如修正格式错误或优化排版


### Lifecycle

目前设计上支持多 Session 并行，每个 Session 独立管理其生命周期，包括创建、使用和销毁。这有助于在同一应用中同时处理多个文档或任务，提升系统的并发处理能力和资源利用效率。

每个 Session 的生命周期管理可以通过状态机来实现，确保在不同状态下的操作合法性。例如，只有在 Session 创建后才能进行文档解析，解析完成后才能进行 Tweak 操作，最终才能销毁 Session。
