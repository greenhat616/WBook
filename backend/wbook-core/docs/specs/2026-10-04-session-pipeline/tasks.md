# Session 管线：实施与验证

- 日期：2026-10-04
- 状态：T1–T5 已实施并验证；T6 的示例、规模测量与文档已完成，V5 特定清理失败注入仍有限制（见 [verification.md](verification.md)）
- 需求：[requirements.md](requirements.md)
- 设计：[总览](design.md)、[Workspace 层](design-workspace.md)、[Session 运行时层](design-session.md)

## 执行边界

核心 Rust API 的实施证据与限制见 [verification.md](verification.md)。任务顺序表达依赖，不要求每个任务单独提交；不可分割的 API 迁移需形成完整、可编译的提交。不得为了通过验证修改无关前端、服务端或格式问题。

不实施 Session 持久化、自动保存、重启恢复、GUI / HTTP / WebSocket / Tauri 接入。所有验收通过核心 Rust API 进行，不能将其描述为桌面应用全链路已完成。

## T1：Workspace 类型与配置

- [x] 新建 `workspace` 模块：WorkspaceId、Revision、WorkspaceState、ProcessingOptions、FilterConfig、WorkspaceStatus、WorkspaceError、OpContext、Phase。
- [x] `Workspace::new` 生成 WorkspaceId 并编译验证配置，不做 I/O。
- [x] 私有提交函数：Revision 加一并关闭预览；持久字段只能经提交修改。
- [x] ExportOptions、RenderOptions 增加 PartialEq。

依赖：无。对应 R1、R3、R8。验证 V1、V8。

## T2：Workspace 操作

- [x] initialize：提取、接管前检查取消、逐个过滤并记录进度、解析、安装；首次安装前可续跑，之后返回 AlreadyInitialized。
- [x] parse（显式 TOC 配置、不修改工作区）与 install（Revision 校验加既有 install 校验）。
- [x] apply_edits、set_metadata_overrides、read_text、results；空修改不提交。
- [x] render_preview（按 options 复用、失败保留旧预览）、export_epub（独立渲染）、take_warnings、close。
- [x] `WorkspaceError::is_cancelled` 按底层错误判定。
- [x] crate 内部 `initialize_with` 注入测试解析器。

依赖：T1。对应 R2、R3、R4、R5。验证 V2、V3、V4、V5（同步测试，无需 tokio）。

## T3：Session 运行时

- [x] SessionHandle、Lifecycle、Slot、ActiveOp；准入在锁内完成。
- [x] 通用 `run`：spawn_blocking 独占执行、业务失败交回 Workspace、完成收口只执行一次、先发布快照再完成凭据。
- [x] 进度回报按 OperationId 过滤；SessionSnapshot 与 watch 订阅。
- [x] cancel 按 OperationId 匹配；JoinError 置为 Lost 并返回 Panicked。
- [x] 各数据操作的 typed 转发方法；替换占位的 SessionState、Timeline、Command、SessionError。

依赖：T2。对应 R1、R5、R7。验证 V6、V9。竞争测试使用受 barrier / channel 控制的合成闭包，不以 sleep 决定竞争结果。

## T4：关闭

- [x] close 的单调状态转换、取消活动操作、spawn 关闭任务、等待 Idle 后 `Workspace::close`、共享 CloseReport。
- [x] Lost 工作区可关闭；关闭期间完成的发布仍报告成功；不删除输出与输入。

依赖：T3。对应 R6、R7。验证 V7。

## T5：Manager 与 Wbook

- [x] `Mutex<Registry>`：open / create / get / list / close；关闭完成后经 Weak 移除。
- [x] shutdown 与 open 共用锁；先请求全部关闭再等待并汇总。
- [x] Manager / Wbook 的 Drop 只尽力请求停止；Wbook 持有 Params 与 Manager。
- [x] 移除 dashmap 依赖。

依赖：T4。对应 R1、R6、R8。验证 V7、V8。

## T6：端到端验证与文档更新

- [x] 添加仅调用 Wbook / Handle 的示例：创建、初始化、编辑正文、重解析并安装、调整目录和元数据、预览、导出、继续编辑、关闭及 shutdown。
- [ ] 完成下表全部自动化场景，检查既有解析与导出回归。
- [x] 测量至少 10 MiB 和 100 MiB 文本的初始化 / 预览 / 导出取消响应，以及两个 Session 并发与关闭；记录环境、触发阶段和实际延迟，不编造最大保证。
- [x] 检查快照不含全文、操作以移动传递工作区、Session 不累积正文 / 目录 / 操作历史；说明打开文档数量决定总内存占用。
- [x] 更新 DESIGN_NOTE.md Lifecycle、公开 API 说明和 examples；明确 GUI 与持久化仍未接入。
- [x] 新增 verification.md 记录命令、测试数、竞争测试方式、测量、限制与既有阻塞；按实际证据更新任务状态。

依赖：T1–T5。对应 R1–R8。验证 V1–V10。

证据：172 个核心测试通过，包含既有解析与导出回归；V1–V10 的逐项测试名称见 [verification.md](verification.md)。V5 已覆盖真实 Windows 预览清理告警与 pending warning 下导出成功；本次 EPUB 成功发布后其内部渲染目录关闭失败，现有 API 无确定注入入口。因此“全部自动化场景”项保留未勾选，不将替代场景宣称为该分支的完整证明。

## 验收矩阵

| 场景                 | 核心断言                                                                                                                                                            | 需求       | 任务   |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | ------ |
| V1：创建与配置       | 配置错误不注册；创建不读文件；读取错误作为 initialize 失败且可再次 initialize；SessionId 不复用                                                                     | R1         | T1、T5 |
| V2：初始化与续跑     | 精确验证顺序和调用次数；失败不进入后续步骤；续跑时非幂等过滤器只执行一次；接管前取消保持 Absent；安装后 initialize 被拒绝；部分过滤后可显式 parse + install         | R2、R5     | T2     |
| V3：编辑与提交计数   | 提交表中每种操作的 Revision 变化；空编辑与相同 overrides 不递增；旧 Revision 拒绝且不部分生效；正文修改使结果 Stale 并拒绝预览与导出                                | R3、R4     | T2     |
| V4：重解析与安装     | parse 不改变工作区；install 拒绝跨文档、旧版本、越界目录；安装修改后的目录与元数据；overrides 不受 install 影响                                                     | R3         | T2     |
| V5：预览与导出       | 相同 options 复用不重渲染；不同 layout / language / identifier 重渲染；任何提交使预览失效；渲染失败保留旧预览；导出后可继续编辑；不覆盖既有目标；清理告警不覆写成功 | R4         | T2     |
| V6：准入、取消与凭据 | 同 Session Busy；多 Session 并行；旧 OperationId 不取消新操作；控制操作不被阻塞；每个凭据恰好完成一次；快照先于凭据；丢弃凭据不取消；迟到进度不覆盖                 | R1、R5、R7 | T3     |
| V7：关闭与 shutdown  | 空闲 / 运行 / Lost 均可关闭；并发 close 共享结果；等待工作线程实际返回才 Closed；发布后关闭保留产物；旧 Handle 只读终态；创建与 shutdown 无遗漏                     | R6         | T4、T5 |
| V8：持久化就绪       | WorkspaceState 不含运行时资源或 trait object；持久字段只经提交修改；Revision 变化与持久状态变化一致；Manager 以 open(Workspace) 为入口                              | R8         | T1、T5 |
| V9：异常隔离         | panic 后 Lost 并完成凭据；Lost 拒绝数据操作但可查询与关闭；其他 Session 正常                                                                                        | R7         | T3     |
| V10：端到端与规模    | 公共 Session API 完成全链；大文本与并发有实测取消延迟；无逐阶段全文复制、无界命令队列或无限历史；关闭不删输入与产物                                                 | R1–R8      | T6     |

清理失败同时覆盖可确定的测试替身和平台支持的真实文件占用；无法稳定复现的系统条件如实记录，不把 mock 成功当作真实 OS 行为的完整证明。

## 实施时检查命令

以下命令已从仓库根目录执行，实际结果与示例命令见 [verification.md](verification.md)。核心 check / test / clippy 通过；全工作区 check 与全局 fmt --check 仍受已记录的既有问题阻塞：

```powershell
cargo test --manifest-path backend/Cargo.toml -p wbook-core
cargo check --manifest-path backend/Cargo.toml -p wbook-core --all-targets
cargo clippy --manifest-path backend/Cargo.toml -p wbook-core --all-targets
cargo check --manifest-path backend/Cargo.toml --workspace
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
git diff --check
```

本批修改的 Rust 文件单独通过 rustfmt --check；未重格式化无关 extractor 文件。示例运行、两次 release 测量与具体既有阻塞均已写入验证记录，不宣称全工作区通过。

## 明确延后

- [ ] TODO：持久化保存、自动保存、重启 / 崩溃恢复及数据库 / 项目格式。
- [ ] TODO：全局并发配额、预览资源传输层访问、导出复用预览、导出细分阶段进度。
- [ ] TODO：传输协议、产品 UI 与真实应用退出事件接线。

上述条目仅跟踪后续工作，不纳入 T1–T6 本期完成标准。
