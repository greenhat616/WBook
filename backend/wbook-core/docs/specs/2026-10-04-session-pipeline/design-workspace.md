# Session 管线：Workspace 层

- 日期：2026-10-04
- 状态：已实施
- 总览：[design.md](design.md)；运行时：[design-session.md](design-session.md)

## 1. 定位

Workspace 是一份可编辑的电子书工作区。它是同步的，不知道 Session、线程或通道的存在，可以不启动 tokio 运行时直接测试。它是本 spec 中唯一持有领域规则的地方，也是以后唯一的持久化对象。

## 2. 结构

```rust
pub struct Workspace {
    state: WorkspaceState,
    // Ephemeral: never part of saved state.
    preview: Option<Preview>,
    warnings: Vec<CleanupFailure>,
}

// The complete set of data a future save format must cover.
struct WorkspaceState {
    id: WorkspaceId,
    revision: Revision,
    source: Utf8PathBuf,
    options: ProcessingOptions,
    document: Option<ProcessingDocument>,
    filters_applied: usize,
}

pub struct ProcessingOptions {
    pub filters: Vec<FilterConfig>,
    pub toc: TocParserConfig,
}

pub enum FilterConfig {
    Ad,
}

struct Preview {
    options: ExportOptions,
    book: RenderedBook,
}
```

- ProcessingOptions 与 FilterConfig 派生 serde / specta：它们既是接口 DTO，以后也会进入保存格式。
- SimpleExtractor 与 SimpleMetadataParser 固定使用，不进入配置；以后可配置时以带默认值的新字段加入。
- 已安装结果与 overrides 由 ProcessingDocument 持有，Workspace 不另存一份。
- 新增持久字段只能加在 WorkspaceState 中，且只能在提交时修改。

`Workspace::new(source, options)` 生成 WorkspaceId，编译验证全部配置（`TocParserConfig::build`），不做 I/O；失败返回 InvalidConfig。

## 3. 提交

所有持久状态修改都经过同一个私有提交函数：Revision 加一；若存在预览则关闭它，关闭失败记入 warnings。由此得到两个不变量：

- 预览存在时一定对应当前 Revision，预览有效性只需比较 ExportOptions。
- Revision 的变化与持久状态的变化一一对应。

| 操作                                     | 提交次数                                  |
| ---------------------------------------- | ----------------------------------------- |
| initialize 接管提取结果                  | 1                                         |
| initialize 每完成一个过滤器              | 每个 1 次；空修改也提交，因为过滤进度变化 |
| install（含 initialize 末尾的安装）      | 1                                         |
| apply_edits                              | 正文实际变化时 1 次；空批次不提交         |
| set_metadata_overrides                   | 值变化时 1 次；相同值不提交               |
| parse、读取、render_preview、export_epub | 0                                         |

单个过滤器的正文修改与 `filters_applied` 加一在同一次提交中完成：既有 `apply` 成功后，两者都是纯内存赋值，中间没有可失败的步骤，因此不会出现"正文已改但进度未记"的状态。

## 4. 状态（推导）

`status()` 由当前数据计算，不单独存储：

```rust
pub struct WorkspaceStatus {
    pub revision: Revision,
    pub document: DocumentStatus,          // Absent | Unparsed | Current | Stale
    pub document_version: Option<DocumentVersion>,
    pub filters: FilterProgress,           // applied, total
    pub has_overrides: bool,
    pub preview: Option<ExportOptions>,
}
```

Current 与 Stale 由 `results.version` 是否等于当前 DocumentVersion 决定，与 `ProcessingDocument::current_results` 同一判据。

## 5. 操作

操作统一签名为 `fn op(&mut self | &self, cx: &OpContext, ...) -> Result<T, WorkspaceError>`：

```rust
pub struct OpContext<'a> {
    pub ct: &'a CancellationToken,
    pub report: &'a (dyn Fn(Phase) + Sync),
}

pub enum Phase {
    Extracting,
    Filtering { index: usize, total: usize },
    Parsing,
    Installing,
    Editing,
    Reading,
    Rendering,
    Exporting,
}
```

`report` 只传递轻量阶段值，不等待消费方。

| 操作                                              | 接收者      | 前置条件                                    | 返回                                |
| ------------------------------------------------- | ----------- | ------------------------------------------- | ----------------------------------- |
| initialize                                        | `&mut self` | 尚未安装过结果，否则 AlreadyInitialized     | Revision                            |
| parse(config: TocParserConfig)                    | `&self`     | 有文档；配置可构建                          | ParsedResults                       |
| install(expected, ParsedResults)                  | `&mut self` | Revision 匹配；既有 install 校验通过        | Revision                            |
| apply_edits(expected, EditBatch)                  | `&mut self` | 有文档；Revision 匹配；既有 apply 校验 base | Revision 与 ChangeMap               |
| set_metadata_overrides(expected, Metadata)        | `&mut self` | 有文档；Revision 匹配                       | Revision                            |
| read_text(DocumentVersion, TextRange)             | `&self`     | 版本匹配；范围不超过 1 MiB UTF-8 字节       | String                              |
| results()                                         | `&self`     | 有文档                                      | 已安装结果、是否 Current、overrides |
| render_preview(expected, ExportOptions)           | `&mut self` | 结果 Current；Revision 匹配                 | PreviewInfo                         |
| export_epub(expected, ExportOptions, destination) | `&self`     | 结果 Current；Revision 匹配                 | ExportArtifact                      |
| take_warnings()                                   | `&mut self` | 无                                          | `Vec<CleanupFailure>`               |
| close(self)                                       | `self`      | 无                                          | `Vec<CleanupFailure>`               |

### 5.1 initialize

1. 无文档时用 SimpleExtractor 提取；返回后检查取消，已取消则丢弃内容并保持 Absent。随后移动进 ProcessingDocument，提交。
2. 从 `filters_applied` 开始逐个运行剩余过滤器：`FilterParser::parse` 产生批次，`ProcessingDocument::apply` 提交到正文，提交。
3. 用 `options.toc` 解析当前正文，安装结果，提交。

失败或取消时停止后续步骤，已提交的前缀保留。只要尚未安装过结果，再次调用 initialize 会从已提交的进度继续：已有文档则跳过提取，已完成的过滤器不再执行，因此非幂等过滤器只会执行一次。首次安装成功后 initialize 返回 AlreadyInitialized，之后的重解析一律走 parse 加 install。

过滤未完成时，调用方也可以直接对当前文本 parse 并 install。这是显式选择：安装后 initialize 不再可用，剩余过滤器不会再自动执行。快照中的过滤进度足以让调用方做出这一选择，因此不设额外的"接受部分处理"参数。

Workspace 自己逐个调用过滤器以记录进度，而不调用 `ProcessingDocument::run`；`run` 保留给独立调用方。解析器在操作开始时按配置构造；crate 内部的 `initialize_with` 变体接受注入的解析器，仅供测试，不作为产品配置暴露。

### 5.2 重解析与目录编辑

parse 不修改工作区，返回带 DocumentVersion 的完整 ParsedResults；调用方可显式传入新的 TOC 配置，该配置不写回 `options`。

install 同时承担"接受解析结果"与"替换编辑后的目录"两种用途：调用方可以先修改 ParsedResults 中的目录与自动元数据再安装。目录经既有 TocSnapshot 反序列化校验结构，`ProcessingDocument::install` 再校验 document_id、版本和每个节点范围，全部通过后整体替换。expected Revision 防止覆盖调用方读取之后发生的其他安装、编辑或 overrides 修改。安装可能重新分配 NodeId，调用方安装后需重新读取目录。

overrides 独立于自动元数据，install 不改变 overrides；移除某个 override 后恢复使用自动值（沿用 `ProcessingDocument::metadata`）。

### 5.3 正文编辑

apply_edits 先校验 Revision，再调用既有 `apply`。返回的 ChangeMap 前后版本相同时视为空批次，不提交。正文变化后已安装结果自然变为 Stale 并保留，可供对比；preview 与 export 拒绝 Stale 结果，直到 install 新结果。

### 5.4 预览与导出

render_preview：若已有预览且 ExportOptions 相等，直接返回，不重渲染；否则调用 `render_book`，成功后安装新预览并关闭旧预览，关闭失败记入 warnings；渲染失败时保留旧预览（Revision 未变，旧预览仍有效）。

```rust
pub struct PreviewInfo {
    pub revision: Revision,
    pub options: ExportOptions,
    pub directory: PathBuf,
    pub files: Vec<String>,
}
```

`directory` 只在下一次提交、预览被替换或 Session 关闭之前有效，仅供核心 API 调用方使用；面向传输层的资源访问另行设计。

export_epub 调用既有 `export::export_epub` 独立渲染并打包，不读取也不替换预览。沿用"不覆盖既有目标"和 `persist_noclobber` 发布点；ExportArtifact 中的 cleanup_failures 原样返回。

### 5.5 关闭

`close(self)` 关闭预览，返回预览清理失败和尚未取走的 warnings。它只清理 Workspace 自己拥有的临时资源，不触碰输入文件和已发布的 EPUB。

## 6. 错误

```rust
pub enum WorkspaceError {
    InvalidConfig(TocConfigError),
    StaleRevision { expected: Revision, actual: Revision },
    NoDocument,
    AlreadyInitialized,
    ResultsNotCurrent,
    ReadTooLarge { requested: u64, limit: u64 },
    Extractor(ExtractorError),
    Document(DocumentError),
    Pipeline(PipelineError),
    Export(ExportError),
}
```

`is_cancelled()` 按底层错误判断（ExtractorError::Shutdown、DocumentError::Cancelled、ParserError::Cancelled、ExportFailure::Cancelled），不看令牌状态。因此提交之后才到达的取消不会把成功改报为取消，同时发生的错误与取消也以底层报告为准。

操作失败时不需要专门的"部分完成报告"类型：已提交的部分体现在 Revision 与 `status()` 中，运行时在操作结束后读取二者交给调用方。

## 7. 持久化预留

- WorkspaceState 即以后保存格式的完整覆盖范围；ParsedResults、Metadata、TocParserConfig、DocumentVersion 已有 serde 表示，TextDocument 以后需要在 document 层增加保持原 DocumentVersion 的持久表示与 crate 内部重建构造。
- 以后新增 `Workspace::restore(saved) -> Result<Workspace, _>` 与只读的 `save(&self, cx, ...)`；本期不提供占位实现。
- 源路径随工作区保存；恢复时不重新读取源文件，源文件变化检测由持久化 spec 决定。
- 预览与 warnings 不保存；恢复后预览为空，需要时重新渲染。

## 8. 测试

本层规则全部用同步测试覆盖，不需要 tokio：提交表与 Revision 计数、Stale 判定、预览失效与按 options 复用、Revision 冲突不改变状态、initialize 续跑时注入的非幂等过滤器只执行一次、接管前后取消、各步骤的已提交前缀，以及 install 拒绝跨文档、旧版本和越界结果。取消用预先取消的令牌或在指定步骤取消令牌的测试解析器驱动。
