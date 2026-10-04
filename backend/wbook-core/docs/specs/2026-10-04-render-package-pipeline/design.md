# 渲染与打包流水线：设计

- 日期：2026-10-04
- 状态：核心渲染与 EPUB 导出已实施；验证结果及限制见 [verification.md](verification.md)
- 需求：[requirements.md](requirements.md)
- 实施与验证：[tasks.md](tasks.md)

## 1. 与现有核心衔接

`ProcessingDocument` 已持有当前正文、版本化解析结果和元数据 overrides。`current_results()` 拒绝过期结果，`TextView` 提供区间读取；现有 `OutputPlan` 仅保存调用方传入的范围，不推导章节、不渲染、不打包。

当前 `TreeNodeMeta.range` 有三种来源：规则解析器给出标题范围，等分解析器给出完整正文块，`TocBuilder` 可回填容器聚合范围。本次为 wire 格式补充 `range_kind` 判别信息，章节规划据此解释范围，不能把任意 range 直接拼成 EPUB。

实施前仓库声明 `tera = "2"`、`epub-builder = "0.8"`，锁定为 2.4.0 与 0.8.3。本次保留 Tera 2.4.0，将未被使用的 epub-builder 替换为 zip 6.0.0，新增 export 模块。`SessionHandle` / `Command` 仍是占位，本期提供可独立调用的核心导出入口，不以补完 GUI 和 Session 为前提。

## 2. 流程与职责

```mermaid
flowchart LR
    A[完成编辑的 ProcessingDocument] --> B[固定输入并校验]
    B --> C[BookPlan 逻辑节与导航]
    C --> D[Tera 渲染]
    D --> E[RenderedBook XHTML 与 CSS]
    E --> F[EPUB 打包]
    F --> G[结构校验]
    G --> H[发布 EPUB]
    G -. 后续扩展 .-> I[依赖 EPUB 的转换器]
    I -.-> J[MOBI 校验与发布]
```

规划器决定文字归属、阅读顺序和导航目标；渲染器决定 XHTML 与文件布局；打包器决定 EPUB 容器与包描述；导出入口负责取消、临时资源和最终发布。打包器不再解析正文，转换器不重新渲染。

第一版使用普通函数和具体类型；只有一个打包器时不建立动态插件注册表。建议文件边界为 `export/plan.rs`、`export/render.rs`、`export/epub.rs` 和 `export/mod.rs`；默认模板放在 `export/templates/`。

## 3. 输入固定与 options

已实施的核心 options：

```rust
pub enum RenderLayout {
    SplitChapters,
    Paged,
    SingleHtml,
}

pub struct RenderOptions {
    pub layout: RenderLayout,
}

pub enum OutputFormat {
    Epub,
}

pub struct ExportOptions {
    pub render: RenderOptions,
    pub format: OutputFormat,
    pub language: String,
    pub identifier: Option<String>,
}
```

默认布局为 `SplitChapters`。首期只有内嵌模板，不增加只有一个值的模板选择字段。后续开放模板时，另加明确的模板来源与版本契约。

核心入口 `export_epub(ct, &ProcessingDocument, &ExportOptions, destination) -> Result<ExportArtifact, ExportError>`；`OutputFormat` 用于上层请求和目标检查，当前分派只接受 EPUB。`ExportArtifact` 至少包含实际格式、最终路径、文档版本和本次出版标识。

入口开始时检查 `current_results()`，借用固定 `TextView`，复制目录中用于输出的标题、深度、范围及小体积元数据，合并 overrides，解析 options。同步核心调用期间不可变借用阻止正文修改；调用方应在 worker 内串行访问同一文档，不持有异步锁跨越整个导出。并行编辑快照不属于本期。

`render_book` 返回拥有临时资源的 `RenderedBook`，`package_epub` 接受这份产物，允许预览后原样打包。`BookPlan` 是作业内私有值，带 `DocumentVersion`；渲染入口仍检查版本。目录的标题、深度与正文范围、options 与出版元数据由计划拥有，不在后续阶段重新读取 `metadata_overrides`。本期不缓存或持久化计划，避免仅凭正文 revision 误认目录、标题或 options 未变。

出版元数据规则：合并后标题为空或全空白则报错；作者为空则省略；language 必填合法 BCP 47 标签，调用方不知道语言时可显式选 `und`。identifier 未提供时为作业生成一次 UUID URN，修改时间取作业开始的 UTC 时间。重试一个新作业可产生新标识；需要保持书籍身份的调用方传入同一标识。固定标识与时间可用于确定性测试。

## 4. 目录范围与兼容策略

`TreeNodeMeta` 已增加 `range_kind: TocRangeKind`，取值为 `Heading`、`Body`、`Container`、`Unknown`。保留 `range`，减少现有树操作与显示范围的迁移；安装检查版本与范围边界，规划进一步检查字段组合和结构：

| kind      | range 的含义                   | 规划行为                               |
| --------- | ------------------------------ | -------------------------------------- |
| Heading   | 源标题的非空范围，不含行结束符 | 替换为编辑后的目录标题，并推导后续正文 |
| Body      | 非空完整正文块                 | 全部作为正文，显示标题为附加标题       |
| Container | 可为 None 或子项聚合范围       | 不消耗任何源文字                       |
| Unknown   | 旧快照缺失语义                 | 可供查看，不可据此导出                 |

规则 / VBook 的实际标题事件标记 Heading；等分事件标记 Body；自动补层与纯分组节点标记 Container，回填 range 不改变 kind。需同步调整事件、builder、树新增入口及 serde / specta，不能仅在导出器猜来源。

旧快照缺少字段时读为 Unknown，不自动把所有 range 当标题。用户需重新解析并显式安装，或在当前文档上确认各范围语义；不会静默覆盖已编辑目录。结构快照仍不携带版本认证。NodeId 只在作业内用于关联；现有快照恢复可能重分配 ID，因此输出路径和锚点使用计划生成的编号。

首期同一目录的非容器节点必须全部为 Heading 或全部为 Body；混合模式报 `InvalidInput`，留待存在真实需求后扩展。Container 可出现在两者之间。用户修改目录后再次检查范围、唯一性、树结构和当前文档版本。

## 5. BookPlan 与章节推导

计划保存有序逻辑节，每节包含本次生成的 ID、可选节点关联、显示标题、深度、正文范围与源标题消耗范围；导航另存层级、标签和节目标。范围是当前文本 UTF-8 半开区间；正文仍借助 TextView 读取，不复制到计划。

### 标题式目录

1. 收集 Heading 节点，按源 range.start 排序。检查非空、边界有效、无重叠、同起点无歧义，且范围不含 CR/LF。VBook 的同行卷名、章名允许相邻的局部范围；重复的卷名前缀并入后续章名的消耗范围。不能用显示标题与原行相等来验证，因为用户可能已改名。
2. 源标题消耗范围为标注的范围及紧随其后的最多一个 LF 或 CRLF；移除的是标题结尾的排版换行，不吞掉其后的空行。
3. 每个标题的正文从消耗范围末尾延伸至下一源标题开始；最后延伸至文档末尾。首标题之前的 `[0, first_start)` 为前言，有字节就保留。只有标题没有正文的节仍合法。
4. 按目录 DFS 前序排列这些逻辑节。卷标题后、子章前的正文属于卷自己的节；父节不包含子节正文。调换子树时子树内各节一起按新树顺序输出，前言固定最前。
5. 删除某一 Heading 目录项后，它不再作为标题消耗；原行与正文留在相邻源节内按普通文字渲染。删除所有目录项时，整个非空文本成为一个无附加标题的正文节。

例如源文本为 `前言 / 卷一 / 卷说明 / 第一章 / 正文甲 / 第二章 / 正文乙`（每项一行）：计划为前言、卷一自身节、第一章、第二章。交换两章后输出前言、卷一、第二章、第一章；卷说明只出现一次。

### 正文块目录与容器

Body 节点范围按源坐标排序后必须无缝、无重叠覆盖全文，等分解析器满足这一契约。每块可在任意 UTF-8 字符边界结束，不要求整行；不得删除正文首行。阅读顺序仍由目录决定。修改这类目录造成范围缺口或重叠时拒绝导出，调用方需显式合并或重新确认正文范围。

Container 无论是否有聚合 range 都不复制正文。有名容器在其后代前生成一个只含标题的逻辑节，提供自己的导航锚点；空的有名容器也可保留该标题节。匿名容器不输出标题或导航条目，导航提升其有名后代；空匿名容器忽略。标题显示层级按去掉匿名容器后的深度计算，超过六级时用 `h6` 加深度 class，导航仍保留真实的有名层级。

若非空文档只有容器，全文放在最前的无标题正文节，随后输出有名容器标题节。空文档即使有手工目录也返回 `InvalidInput`（empty document）。

规划验证源正文范围与被替换的标题消耗范围的并集恰好覆盖全文、两两不相交。随后重排只改变顺序；不得以被打包文件的字节相等来检验这一性质，因为转义、标题替换和换行排版会改变字节。

## 6. Tera 模板与渲染产物

默认模板由 `include_str!` 嵌入，通过 Tera 注册一次复用。内嵌 `document.xhtml`、`section.xhtml`、`paragraph.xhtml`、`package.xml` 与 `style.css`，结束标签使用固定片段；每个文件可由这些片段依次写入 writer，无需先构造整本 `body_html`。

模板 context 只包含出版元数据、当前节 ID / 标题 / 深度、当前段落文本和由核心生成的相对资源路径。模板不能访问任意文件、网络或 shell；本期不开放用户模板。显式给 `.xhtml` 开启自动转义，正文和元数据不得使用 `safe`。Tera 默认自动转义后缀不包含 `.xhtml`，且不做上下文感知转义，因此数据只进入文本节点或带引号的普通属性，不插入脚本、CSS 或任意标签名。[Tera 官方文档](https://keats.github.io/tera/)

每个非空逻辑行输出一段，保留行内空白；空行输出带空行样式的合法空段。LF / CRLF 只参与段落边界，无末尾换行不影响末段；终止换行不额外创造空段。跨片段行先按当前文本拼接；Body 边界截断逻辑行时分别在各节成段。禁止 XML 字符须在写入前检测，错误带源偏移或元数据字段名。Tera 自动转义后通过独立 XML 解析测试；另将孤立 CR 写为数字实体，避免 XML 换行归一化改变正文。

`RenderedBook` 保存临时资源目录、按 spine 顺序排列的内容文件列表和逻辑计划；媒体类型由固定资源类别决定，导航 XHTML 从同一计划生成。资源名由核心生成，例如 `text/section-0001.xhtml`、`text/book.xhtml`、`styles/book.css`；锚点如 `section-0001`，不用用户标题作路径。所有引用在打包前校验为包内相对地址，预览使用同一资源目录及链接。

| 模式          | 正文资源               | 章节边界                                                                                                                              |
| ------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| SplitChapters | 每逻辑节一个 XHTML     | 文件边界；不再额外要求首行分页                                                                                                        |
| Paged         | 一个 `text/book.xhtml` | 后续节添加分页 class，屏幕采用 `break-before: column` 适配多栏阅读器，保留 `page-break-before: always`，打印采用 `break-before: page` |
| SingleHtml    | 一个 `text/book.xhtml` | 标题元素与节锚点，不加分页 class                                                                                                      |

Paged 和 SingleHtml 的区别仅在分页 class；分页是阅读器可重排布局提示，不生成固定页码或 `page-list`。预览与阅读器可以不同，正文、标题和链接必须相同。

渲染内存目标是计划与导航 O(节点数)、局部文本 O(最长逻辑行) 及有界输出缓冲；单文件不等于整本驻留内存。打包库可能有额外缓冲，需独立测量，不能据此承诺全流程常数内存。

## 7. EPUB 打包

依赖检查确认 `epub-builder 0.8.3` 的 `ZipLibrary` 持有整包 `Cursor<Vec<u8>>`，且 `add_content` 同时使用 TOC URL 作为内容文件路径。其公开接口不适合本期独立导航和任意出版标识契约。因此改用 zip 6.0.0 直接写文件，并用 Tera 生成 OPF 与 XHTML 导航；container 使用固定 XML，不引入外部 zip 或 Kindle 工具。范围规划和正文渲染不依赖 ZIP 类型。

按 [EPUB 3.3](https://www.w3.org/TR/epub-33/) 验证这些最低结构要求：

- 根目录首个 ZIP entry 为不压缩、不加密、无额外字段的 `mimetype`，内容精确为 `application/epub+zip`，不带 BOM 或换行。
- `META-INF/container.xml` 指向有效 OPF；package 的 `version` 为 `3.0`，unique-identifier 引用存在的标识。
- OPF 包含标题、语言、标识、UTC 修改时间；manifest 包含正文、CSS 与带 `nav` 属性的导航资源。
- spine 按计划顺序引用正文内容文件；单文件模式只有一个正文 itemref。
- XHTML 使用正确命名空间；导航具备 TOC nav，链接包含真实文件及锚点。本期不生成可选 NCX，以 EPUB 3 导航为准。

运行时在临时产物上检查 ZIP / XML、必需资源、manifest / spine、唯一 ID 及链接引用。开发验收另外使用 [EPUBCheck](https://www.w3.org/publishing/epubcheck/)；不把 Java 或 EPUBCheck 变成普通用户每次导出的安装前置条件。ZIP 写入成功并不自动证明以上条件。

## 8. 产物依赖与未来后处理

首期执行固定顺序 `Plan → Render → PackageEpub → Validate → Publish`。`RenderedBook` 与 `ExportArtifact` 分开，使后续能加 `convert_epub_to_mobi(input_artifact, tool, ct)`；不预先实现插件 trait、通用图算法或外部进程运行器。

后续格式实现遵守以下契约：

| 请求目标            | 必需产物                     | 外部依赖               | 发布对象                   |
| ------------------- | ---------------------------- | ---------------------- | -------------------------- |
| EPUB                | RenderedBook                 | 无 Kindle 依赖         | EPUB                       |
| MOBI（后续）        | 经校验的 EPUB                | 经验证支持该转换的工具 | MOBI；中间 EPUB 为临时文件 |
| EPUB + MOBI（后续） | 共用一次渲染、一次 EPUB 打包 | 同上                   | 各自成功且被请求的产物     |

转换器描述输入 / 输出格式、工具路径与支持版本 / 平台 / 能力。只针对所请求目标预检；缺少必要工具在昂贵渲染前失败，不能降级为成功返回 EPUB。选择同一转换路线时前置产物按作业复用；不建立跨作业缓存。未来多条路线必须显式选择，不凭安装顺序猜测。

Amazon 文档给出通过 Kindle Previewer 打开 EPUB 后导出 MOBI 的操作，但这不证明任意安装版本都提供可自动调用的 CLI。MOBI 适配实施时必须重新核实所选版本的自动化接口、输出能力及平台；不硬编码未验证命令、不假定可分发其私有工具。[Amazon 导出说明](https://kdp.amazon.com/en_US/help/topic/G200641240)

未来进程适配使用独立程序路径与参数数组，捕获退出码和有界诊断，支持超时 / 取消并回收其启动的子进程；输出存在且验证通过才生成产物记录。工具缺失、版本不支持、执行失败、产物无效分别报告。Kindle 发送功能属于另一种有外部副作用的后处理，不随导出隐式执行。

多目标首期不开放；扩展时采用逐产物提交：EPUB 成功且被请求可先发布，随后 MOBI 失败返回带 EPUB 成果的部分成功报告；只请求 MOBI 时不把中间 EPUB 发布为最终结果。转换只能读取前置产物，失败不能修改已生成的 EPUB。

## 9. 取消、错误与文件生命周期

每个作业拥有独立临时目录；渲染资源存于该目录，最终包临时文件位于目标所在文件系统。目标父目录不存在或目标已存在时预检失败。发布采用具备“不覆盖既有目标”语义的同文件系统原子发布原语，处理检查后目标才被创建的竞态；禁止先删除目标再 rename。

阶段为 `Planning`、`Rendering`、`Packaging`、`Validating`、`Publishing`。错误中报告阶段；本期未引入进度事件或完整 Session 状态机。所有循环、writer 包装及阶段交界检查 `CancellationToken`；打包库不可中断的调用返回后也必须先查取消再发布，记录这种取消延迟。

`ExportError` 区分文档 / 版本错误、范围语义缺失、结构错误、非法元数据 / 文本、模板、打包、校验、I/O、目标已存在、不支持格式与取消，并附阶段、节点 / 源范围 / 资源路径等相关上下文。模板内部失败不包含整本正文日志。

临时资源由作业所有者清理，只删除自己创建的路径。发布前失败或取消不留下完整目标名；打包临时文件的显式清理失败保留原始错误并带出待清理路径；完整 export 入口也报告已渲染资源的清理失败。独立预览调用者用 `RenderedBook::close()` 观察清理错误；渲染尚未返回产物即失败时和普通 Drop 使用 tempfile 的尽力清理。进程崩溃可能留下临时目录，本期不增加全局自动清理器。成功发布是提交点，随后取消不撤回结果；不宣称跨多个输出文件的原子事务或掉电持久性。

## 10. 实施边界

现有 `OutputPlan` 保持原始区间写出用途；新增 `BookPlan` 负责章节语义，不悄悄改变已有低层 API。对目录范围字段的迁移只服务于 R3，不顺便改造 slab ID、树操作或解析规则。现有设计说明在实现落地后同步更新；本 spec 先作为下一阶段的明确入口。

首期交付是核心 API、默认模板、三种布局、EPUB 文件及验证证据。MOBI、通用后处理平台和 UI 另立实现任务，不把未来接口草图当作已经可用的能力。
