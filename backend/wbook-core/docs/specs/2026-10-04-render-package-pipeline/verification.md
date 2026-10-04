# 渲染与 EPUB 导出：实施验证

- 日期：2026-10-04
- 文档提交：`435c136`（先提交 spec，再实施）
- 核心入口：`export_epub`；可独立调用 `render_book` 与 `package_epub`
- 功能范围：三种布局、内嵌 Tera 模板、EPUB 3、结构校验、不覆盖发布

## 功能与回归

从仓库根目录执行：

```powershell
cargo test --manifest-path backend/Cargo.toml -p wbook-core --quiet
cargo clippy --manifest-path backend/Cargo.toml -p wbook-core --all-targets -- -D warnings
```

140 项核心测试通过，其中新增 25 项导出测试。核心 Clippy 通过；Cargo 仍报告原有的 workspace 字段和四个未使用依赖警告，没有因此修改无关依赖。

新增测试覆盖三种布局内容投影一致、目录改名与调序、元数据覆盖、卷内说明、前言与尾文、等分首行、VBook 同行标题及重复卷名前缀、删除目录保留文字、容器不复制正文、匿名与九层有名目录、Unicode 与 HTML / Tera 转义、XML 禁止字符、孤立 CR、跨片段 CRLF、长单行、过期与跨文档计划、旧快照 Unknown、范围缺口 / 重叠 / 混合模式、树环、取消、写入失败、无效 XML / 链接、临时文件生命周期、同目标并发发布和 Windows 清理失败诊断。

XML 测试使用独立的 roxmltree 解析器，检查解码后的正文与元数据，而非仅比较字符串快照。运行时使用 quick-xml 流式检查生成的 XML、重复 ID、包内链接、manifest、spine 及 ZIP 的 mimetype；这不是通用 EPUB 导入校验器，也不替代 EPUBCheck。

## EPUBCheck 与阅读效果

验证器为 EPUBCheck 5.4.0，从 W3C 项目发行包取得；Java 为本机 OpenJDK 8。实际命令：

```powershell
java -jar backend/target/epubcheck/epubcheck-5.4.0/epubcheck.jar backend/target/epub-examples-verified/wbook-split.epub
java -jar backend/target/epubcheck/epubcheck-5.4.0/epubcheck.jar backend/target/epub-examples-verified/wbook-paged.epub
java -jar backend/target/epubcheck/epubcheck-5.4.0/epubcheck.jar backend/target/epub-examples-verified/wbook-single.epub
```

三个文件均为 **0 fatals / 0 errors / 0 warnings / 0 infos**。该版本工具的实际输出说明使用 EPUB 3.4 rules；包描述版本属性为 EPUB 3 的 `3.0`，本实现只使用可重排正文、目录及常规包元数据。

使用 Chrome 154 和 EPUB.js 0.3.93 / JSZip 3.10.1 在本机读取成品 EPUB，检查了目录、翻页、章名和特殊字符。没有上传书籍内容。另直接浏览了同源 XHTML，确认预览与打包使用同一资源。

最初仅声明 `break-before: page` 时，EPUB.js 的多栏分页没有章前换页。改为屏幕 `break-before: column`、保留旧式 `page-break-before: always` 并在打印媒体中使用 `break-before: page` 后，分页版章节在新页顶部显示；连续版仍在当前页面接续正文。默认分章版按内容文件切换。不同阅读器仍可有自己的分页实现，不承诺相同页数。

截图和示例文件位于 `backend/target/epub-examples-verified/`，属于本地产物，不加入 Git。示例源文为自编的《沿河书简》，含两卷、四章与后记。

## 复现示例

以下命令生成三种 EPUB，并保留可浏览的 XHTML 资源副本。目标目录可自选；若其中同名 EPUB 已存在，导出会拒绝覆盖，应换一个输出目录。

```powershell
cargo run --manifest-path backend/Cargo.toml -p wbook-core --example render_epub -- backend/target/epub-examples-new
```

调用方已有编辑好的 `ProcessingDocument` 时，可直接使用：

```rust
use wbook_core::export::{export_epub, ExportOptions, OutputFormat, RenderLayout, RenderOptions};

let options = ExportOptions {
    render: RenderOptions { layout: RenderLayout::SplitChapters },
    format: OutputFormat::Epub,
    language: "zh-Hans".into(),
    identifier: None,
};
let artifact = export_epub(&cancellation, &document, &options, destination)?;
```

必须先安装当前版本的解析结果。旧目录快照缺少 `range_kind` 时可读取，但导出要求显式确认范围语义或重新解析。若希望保留同一本书的出版标识，传入固定 identifier；不提供时每次新作业生成 UUID URN。

## 规模测量

环境为 Windows x86_64、Intel Core i9-14900KF，仓库 release profile（`opt-level = "s"`、LTO）。`benchmark.py` 用 psutil 每 20 ms 读取 Windows 进程生命周期峰值工作集，每个规模 / 布局为独立进程、单次运行。

```powershell
cargo build --manifest-path backend/Cargo.toml -p wbook-core --release --example render_epub
python backend/wbook-core/docs/specs/2026-10-04-render-package-pipeline/benchmark.py backend/target/release/examples/render_epub.exe backend/target/export-benchmark-new
```

输入为重复的中文短行，每 400 行一个标题，规模略超过指定 MiB。示例生成器预留一整块余量，避免最后一章触发 String 容量翻倍；原始测量中发现该生成器成本后已修正。下面报告修正后的数据。时间从正文已解析并安装后开始，包含规划、渲染、打包、验证与发布；进程峰值内存则包含输入生成与解析，不能当作导出独占内存。

原始结果：[benchmark-results.json](benchmark-results.json)。

| 输入 MiB | 布局   | 渲染 ms | 打包/校验/发布 ms | 总计 ms | 峰值工作集 MiB | 正文临时资源 MiB |
| -------- | ------ | ------: | ----------------: | ------: | -------------: | ---------------: |
| 10       | split  |     254 |                73 |     327 |           16.9 |             10.9 |
| 10       | paged  |     180 |                39 |     219 |           16.8 |             10.8 |
| 10       | single |     176 |                39 |     215 |           16.6 |             10.8 |
| 100      | split  |    2566 |              1174 |    3740 |          112.7 |            108.7 |
| 100      | paged  |    1778 |               376 |    2154 |          107.6 |            107.9 |
| 100      | single |    1776 |               376 |    2152 |          107.8 |            107.9 |
| 300      | split  |   11592 |              3884 |   15476 |          326.4 |            325.9 |
| 300      | paged  |    7420 |              2245 |    9665 |          310.5 |            323.8 |
| 300      | single |    7497 |              2243 |    9740 |          310.7 |            323.7 |

正文临时资源大小由实际文件求和，不包含目录项、导航、CSS 与临时 ZIP，因此不等于总临时磁盘峰值。文本高度重复，EPUB 压缩率不能外推到普通小说。长单行测试证明内容正确，但仍会局部物化并进入 Tera context，不能承诺严格常数额外内存。取消行为已测试，尚未对每个阶段单独测量真实延迟上限；这些测量项在 tasks.md 保持未完成。

## 工作区限制

重新执行 `cargo check --manifest-path backend/Cargo.toml --workspace`，仍受既有 `backend/server/src/router.rs:33` 阻塞：WebSocket `Message::Text` 需要 `Utf8Bytes`，现有代码传入 `String`。

全工作区 `cargo fmt --manifest-path backend/Cargo.toml --all -- --check` 仍报告未修改的 `extractor/simple/encoding.rs:58` 与 `extractor/simple/tests.rs:161`。本次涉及的 Rust 文件单独通过 rustfmt 检查，`git diff --check` 通过。没有将核心检查通过描述为整个工作区通过。

MOBI、外部工具调用、发送到 Kindle、GUI / Session 接线未实施。`RenderedBook` 与已发布 EPUB 的边界已建立，后续转换可消费 EPUB，无需重新渲染。独立预览应显式调用 `RenderedBook::close()` 观察清理错误；未返回产物前的渲染错误和普通 Drop 使用 tempfile 尽力清理。
