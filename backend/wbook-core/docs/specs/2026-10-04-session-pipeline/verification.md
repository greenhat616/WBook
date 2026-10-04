# Session 管线：实施验证

- 日期：2026-10-04
- 范围：wbook-core 的 Workspace、Session、Manager / Wbook 核心 Rust API；不包含产品 UI 或持久化。
- 状态：T1–T5 已验证；T6 的示例、规模测量与文档已完成。V5 的一次特定清理失败注入仍受既有 export API 限制，保留未完成勾选，见下文。
- 任务与验收矩阵：[tasks.md](tasks.md)。设计文档保持原内容，不改写为实现记录。

## 环境

仓库 `G:/Programs/Rust/WBook`，分支 `feat/render-epub-pipeline`；未提交、stash、reset、切分支或 push。

- OS：Microsoft Windows 11 专业版，版本 `10.0.26300`。
- CPU：Intel Core i9-14900KF，24 核 / 32 逻辑处理器；Rust `available_parallelism` 为 32。
- Rust：`rustc 1.101.0-nightly (c1070d693 2026-09-28)`。
- 核心测试和全链示例：dev / test profile。
- 取消测量：release profile，沿用仓库 `opt-level = "s"`、`lto = true`、`codegen-units = 1`。

环境通过 `Get-CimInstance Win32_OperatingSystem`、`Get-CimInstance Win32_Processor` 和 `rustc --version` 读取。没有测量峰值内存或固定取消上限。

## 执行命令与结果

以下命令从仓库根目录执行，使用现有锁文件和依赖：

```powershell
cargo check --manifest-path backend/Cargo.toml -p wbook-core --all-targets
cargo test --manifest-path backend/Cargo.toml -p wbook-core
cargo clippy --manifest-path backend/Cargo.toml -p wbook-core --all-targets
git diff --check
rustfmt --edition 2021 --check backend/wbook-core/examples/session_pipeline.rs backend/wbook-core/examples/session_latency.rs backend/wbook-core/src/session/tests.rs
```

- check：通过，含新示例与全部测试目标。
- test：172 passed，0 failed，0 ignored；doc-tests 0。包含全部既有 parser / document / TOC / export 回归测试、14 个 Workspace 测试与 18 个 Session 测试。
- clippy：通过；测量示例最初出现的 `sliced_string_as_bytes` 提示已修正，最终仅保留既有 manifest / unused-dependency 警告。
- diff 检查：通过；新建、未跟踪的示例、测试与本记录另用 `git diff --no-index --check -- NUL <path>` 检查。
- rustfmt：本批修改的三个 Rust 文件通过。只格式化了这些文件，没有修改已有 extractor 格式差异。

```powershell
cargo run --manifest-path backend/Cargo.toml -p wbook-core --example session_pipeline
```

实际输出：

```text
session_pipeline: exported_bytes=2230, export_revision=7, final_revision=8, preview_invalidated=true, closed=true, shutdown_reports=0
```

示例使用生成的临时文本，只经 Wbook / SessionHandle 完成创建、initialize、正文编辑、parse + install、修改 ParsedResults 的目录与自动元数据再 install、overrides、预览、导出、继续编辑、close 与 shutdown。DTO、EditBatch 与 TOC 编辑方法只是操作输入 / 返回值，不直接调用 Workspace、ProcessingDocument、parser 或 export 流水线。close 后验证输入与 EPUB 仍存在、预览已失效，随后显式关闭示例的 TempDir，因此样本输入与输出不会留在仓库或临时目录中。shutdown 返回 0 是因为该示例已先显式关闭唯一 Session；多个活动 Session 的 shutdown 由自动化测试另行覆盖。

## V1–V10 证据映射

测试位于 `src/workspace/tests.rs` 与 `src/session/tests.rs`，下列名称可用 cargo test 的过滤参数单独定位。

| 场景               | 自动化证据 / 审查                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | 结果与边界                                                                                                                                                                                                                                              |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| V1 创建与配置      | `creation_validates_config_without_reading_and_extraction_can_retry`；`creation_is_lazy_config_is_validated_and_ids_are_not_reused`                                                                                                                                                                                                                                                                                                                                                                  | 错误配置不注册；创建不读缺失文件；初始化失败后补文件再成功；关闭后 SessionId 不复用；Manager.open 保留 WorkspaceId。                                                                                                                                    |
| V2 初始化续跑      | `initialize_resumes_without_repeating_non_idempotent_filters`；`cancellation_before_takeover_keeps_absent_and_later_boundaries_keep_prefix`；`parsing_failure_or_cancellation_keeps_filters_and_stops_installation`；`explicit_install_after_partial_filters_disables_initialize`                                                                                                                                                                                                                    | 精确阶段顺序、过滤器调用数、空过滤提交、已提交前缀、提取返回后接管前取消、显式安装部分处理正文。没有用睡眠猜测取消边界。                                                                                                                                |
| V3 编辑提交        | `commit_table_and_stale_results`；`stale_revision_rejects_every_guarded_operation_without_changes`；`rejected_edits_are_atomic_and_reads_validate_versions_and_utf8_byte_limit`                                                                                                                                                                                                                                                                                                                      | 提交表、空批次 / 相同 overrides、全部 Revision 前置条件、失效预览、保留 Stale 结果并拒绝预览 / 导出。                                                                                                                                                   |
| V4 解析安装        | `install_validates_document_version_and_ranges_and_preserves_manual_values`；新增 `public_session_flow_keeps_manual_results_and_overrides_after_export`                                                                                                                                                                                                                                                                                                                                              | parse 不修改状态；跨文档、旧版本、越界安装被拒绝；调用方编辑的 TOC / 自动元数据与独立 overrides 经公开 API 安装、读取并导出。                                                                                                                           |
| V5 预览导出        | `preview_reuses_equal_options_replaces_changed_options_and_keeps_old_on_failure`；`export_is_independent_preserves_target_and_allows_further_edits`；Windows `preview_cleanup_failures_warn_without_overriding_commits_or_replacements`、`close_returns_pending_and_new_cleanup_warnings`                                                                                                                                                                                                            | 选项缓存、替换、失败保留、不覆盖目标、导出不读预览、继续编辑已覆盖；真实文件占用验证清理告警不覆写提交 / 替换成功，以及有未取告警时仍可成功导出。直接注入“本次 EPUB 成功发布后，其内部渲染目录关闭失败”尚不可确定复现，未声称该特定分支已经自动化覆盖。 |
| V6 准入取消凭据    | `one_session_is_busy_while_two_sessions_and_control_operations_run_in_parallel`；`cancelled_old_operations_cannot_cancel_new_ones_and_errors_keep_their_category`；`committed_status_is_recomputed_outside_the_worker_and_snapshot_precedes_receipt`；`receipts_are_independent_and_dropping_receipts_handles_or_subscribers_does_not_cancel`；`progress_from_an_old_operation_does_not_overwrite_new_or_terminal_state`；`discarded_operation_results_are_dropped_without_holding_the_session_lock` | Busy、两工作线程同时进入、控制调用、旧 OperationId、独立凭据、快照先于交付、丢弃不取消、迟到进度均覆盖。oneshot Sender 由唯一收口消费，不存在第二次发送路径。                                                                                           |
| V7 关闭与 shutdown | `concurrent_idle_closes_share_a_report_and_old_handles_are_read_only`；`running_close_waits_for_worker_return_and_survives_a_dropped_close_future`；`publication_completed_during_a_running_operation_is_preserved_by_close`；`shutdown_requests_every_close_before_waiting_for_any_worker`；`create_racing_shutdown_never_leaves_an_accepted_session_open`；`dropping_manager_or_wbook_outside_a_runtime_context_is_nonblocking_and_breaks_no_cycles`                                               | Idle / Busy / Lost、同一 Arc 报告、等真实返回、丢弃 close 等待、保留发布产物、终态旧 Handle、注册移除、创建竞争、runtime 上下文外 Drop 均覆盖。                                                                                                         |
| V8 持久化就绪      | Workspace 提交测试、Manager.open 测试；新增 `workspace_allocation_is_moved_and_snapshots_contain_no_body_toc_or_history`；类型与字段审查                                                                                                                                                                                                                                                                                                                                                             | WorkspaceState 私有、无 Serialize 派生，仅拥有 id / revision / source / options / document / filters_applied；无 trait object、令牌、runtime handle 或临时路径。没有实现或假称验证 save/load。                                                          |
| V9 panic 隔离      | `panic_completes_receipt_loses_only_its_workspace_and_allows_close`                                                                                                                                                                                                                                                                                                                                                                                                                                  | Panicked 凭据完成；Lost 拒绝数据操作，查询 / 关闭可用，其他 Session 继续运行。unwind panic 可隔离；abort / OOM / 强杀不在保证内。                                                                                                                       |
| V10 全链与规模     | `typed_forwarders_return_workspace_results_and_wbook_shuts_down`；上述两个新增 Session 测试；实际运行 `session_pipeline` 与 release `session_latency`                                                                                                                                                                                                                                                                                                                                                | 公开 API 全链、10 / 100 MiB、两并发 Session、关闭延迟已执行；结构审查与下文测量说明范围。                                                                                                                                                               |

新增测试补足了公开 API 的手工 TOC / 自动元数据 / overrides 在导出后的保持，以及无正文 / 目录 / 历史的快照字段检查。没有为早已通过的场景另建重复测试。

## 竞争测试的控制方式

- 合成闭包通过 crate-private `SessionHandle::run` 提交。工作线程用 std mpsc 等待释放，进入时发送 tokio oneshot；只有收到进入通知后，测试才执行 Busy / cancel / close / 查询断言。
- 双 Session 的两个闭包都收到进入通知且尚未释放，才验证并发和控制操作；shutdown 测试要求两个快照均先成为 Closing，再释放任一工作线程。
- close 测试用 AtomicBool 记录实际返回、阻塞通道控制返回点；close 等待任务被 abort 后，再释放工作线程并验证后台关闭仍完成。
- 创建 / shutdown 竞争用 Barrier 同步起点。允许任一方赢得 registry 锁，分别断言“创建被拒绝”或“已接受 Session 一定被 shutdown 收集并关闭”，32 次循环不依赖固定调度结果。
- 旧进度通过实际 progress helper 携带旧 OperationId 投递，比较快照与序号完全不变。
- 30 秒 timeout / recv_timeout 只是卡死保护；没有用固定 sleep 决定竞争结果。Windows cleanup 测试用禁止 delete sharing 的真实文件句柄，释放句柄后由测试清理其拥有的临时目录。

## 取消与关闭测量

```powershell
cargo run --manifest-path backend/Cargo.toml -p wbook-core --release --example session_latency
```

运行两次，均成功结束并显式清理样本 TempDir。每次生成恰好 10,485,760 与 104,857,600 UTF-8 字节，重复短行含中文、café 与 ASCII，尾部按 UTF-8 边界补齐；源文件刚写完，不能视为冷缓存 I/O。没有为了进入某个阶段而 sleep 或加入产品故障钩子。

单次 cancel 测量在 watch 观察到指定 Phase 后记录 Instant，调用 `cancel(op)`，直到对应 receipt 被 await 完成；包含取消调用、工作线程剩余工作、完成收口与等待者调度。若操作已完成，示例会如实输出 null phase / NotActive / 成功，而不是伪称取消。本次两次运行的所有 cancel 均为 Requested，实际结果均 cancelled。

下表为毫秒，四位小数保留原采样精度；每格按第一次 / 第二次运行列出，既非均值也非上限：

| 操作                                        | 取消 / 关闭时观察到的阶段 | 10 MiB：第一次 / 第二次 | 100 MiB：第一次 / 第二次 |
| ------------------------------------------- | ------------------------- | ----------------------: | -----------------------: |
| initialize：cancel → receipt                | Extracting                |         4.1928 / 3.2277 |        36.0229 / 27.6386 |
| preview：cancel → receipt                   | Rendering                 |         0.0636 / 0.0502 |          0.0165 / 0.0169 |
| export：cancel → receipt                    | Exporting                 |         0.1455 / 0.1445 |          0.1658 / 0.1661 |
| close 活动 preview：close → CloseReport     | Rendering                 |         1.0350 / 0.3798 |          8.6441 / 3.4071 |
| 并发 initialize A：cancel → receipt         | Extracting                |         4.1316 / 2.9235 |        44.1235 / 31.2936 |
| 并发 initialize B：cancel → receipt         | Extracting                |         4.3602 / 2.9961 |        44.1473 / 31.2692 |
| shutdown 两个 Idle Session：调用 → 全部报告 | Idle                      |         0.0398 / 0.0199 |          0.0794 / 0.0372 |

两次运行、两种大小的并发采样均打印 `both_running_before_cancel=true`，在发出 cancel 前两个 Session 都 Running。两者对应凭据独立等待与计时。shutdown 均返回两个报告；活动 preview 的 close 均使其 receipt cancelled，所有关闭报告 `lost=false`、`cleanup_failures=0`。initialize 取消结束 Revision 为 0；已准备好正文的 preview / export 取消保持 Revision 2。

这些采样发生在观察到阶段通知后的早期，不证明整个阶段中最慢的取消响应。Rendering 包括渲染计划与渲染，Exporting 包括计划、渲染、打包、校验、发布；现有 API 没有后者的细分实时通知，因此不能据此声称测到了 ZIP 包装中途或发布前最后一刻的响应。文件读取 `std::fs::read` 自身不轮询 token；解码与部分库调用也不可抢占，必须返回后才进行取消检查。提取取消的几十毫秒延迟与该边界一致，但本测量没有额外仪器确认 cancel 当时的具体内部语句。close 延迟还包含正文释放和临时资源清理；不设强杀超时或最大响应保证。

## 所有权与内存审查

- `SessionSnapshot` 只有 seq、SessionId、WorkspaceId、源路径、生命周期、活动阶段、WorkspaceStatus、last；WorkspaceStatus 也没有正文或整棵 TOC。新测试在 170,000 字节以上的正文上检查精确字段、没有正文 marker / TOC 标题，序列化快照小于 2 KiB。该样本大小阈值不是任意路径 / options 的通用序列化大小上限。
- `run` 从 Idle 中 `mem::replace` 取出 `Box<Workspace>`，移动进 blocking 闭包，再随结果归还；没有 Workspace.clone 或按操作复制全文。新测试验证三个相继操作看到同一 Workspace 地址。字段与代码审查进一步确认没有正文复制；地址测试本身不声称证明所有库内部临时分配。
- Inner 只持有 lifecycle、slot、next_op、last；没有正文、TOC、时间线或无限 operation history。SessionSnapshot 只保留最近一次摘要，业务结果通过独立 oneshot 交付，watch 可以合并通知，没有无界命令队列。
- 打开的每个 Workspace 仍拥有其正文、编辑缓冲区和已安装结果，打开文档数量决定总正文内存。提取时还暂存输入字节与解码内容，编辑会增加缓冲区；读取结果 DTO、跨片段文本、渲染段落可产生局部或目录副本。未测量或承诺总进程内存为严格一份全文。

## 限制与既有阻塞

```powershell
cargo check --manifest-path backend/Cargo.toml --workspace
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
```

两条命令仍失败，均未通过修改无关文件规避：

1. `backend/server/src/router.rs:33` 的 `Message::Text(format!(...))`：E0308，expected Utf8Bytes，found String。该行与 HEAD 相同，server 没有 diff，Session 变更未调用该路径；锁文件只删除 dashmap 项与 wbook-core 的依赖条目。全工作区因 server 编译失败受阻，不宣称整个桌面 / server 工作区通过。
2. `src/extractor/simple/encoding.rs:58` 和 `src/extractor/simple/tests.rs:161` 的既有换行格式差异使全局 fmt --check 失败。两文件未改动；本批 Rust 文件单独通过 rustfmt --check。
3. 既有 warning：workspace.package.version 未使用；wbook-core 的 config、rocksdb、send-to-kindle、tracing-subscriber 未使用。workspace 检查还报告 server 的 simd-json、wbook-core 依赖未使用。
4. V5 的“成功发布后本次内部 render 目录 close 失败”没有确定的注入入口；Workspace export 固定调用现有 export_epub，内部 RenderedBook 不对测试暴露。真实 Windows 占用已覆盖预览关闭、替换、提交、关闭报告与 pending warning 下导出成功；现有 export 回归还覆盖清理失败保留原错误。不能把这些证据替换为对上述具体成功分支的确定自动化证明，因此 T6 的“全部自动化场景”项保持未勾选。没有增加 export 故障钩子或用不可确定的文件监视竞争强行覆盖。
5. panic 报告的 Revision 仅为最后已知值，副作用未知。runtime 提前销毁、进程强杀、abort / OOM 不在有序关闭保证内；放弃 shutdown 等待不会获得关闭保证。

本次只交付核心 Rust API、示例和验证记录。GUI / HTTP / WebSocket / Tauri 退出事件尚未接线，保存格式、数据库、自动保存、重启恢复、全局并发配额和预览传输层资源访问仍是原 spec 的 TODO，未创建占位实现。DESIGN_NOTE 只更新 Lifecycle，原有 Session spec 链接改写为实施后的描述。
