# Snafu 领域错误：验证

- 日期：2026-10-05
- 范围：core 领域错误与 Tauri / RPC 内部上下文；过程宏和状态订阅在前置 PR。

13 个 core 错误声明迁移到 Snafu。原 tuple 包装改为命名 source 字段，更新仓库内构造与匹配；这是公开 Rust 枚举形状变更。纯包装的 From 转换和分类保持；context(false) 与 display 转发保留每层实际来源，便于标准 Error::source 下转型。TocConfigError 的正则分支改为携带 pattern 的上下文构造，替代原无上下文的 From<regex::Error>；导出失败携带 stage，清理告警仍独立保留。

BridgeError 在 JSON 请求、路径、命令参数、响应编码、Receipt 接收与预览 I/O 失败点附加上下文。仅在 DTO 边界转为 kind / message；来源链不序列化，operation outcome、取消分类、revision 与两类 cleanup warnings 保持原契约。

## 本地检查

- Windows x64，全 workspace / all-features 测试与过程宏审查后回归：174 core 单元、3 来源链集成、20 Tauri、6 过程宏，共 203 项通过。
- workspace / all-targets / all-features Clippy 通过；仅有既有 manifest 和 unused-dependency 警告。
- 来源测试使用真实缺失输入、非法正则、导出 I/O、非法 JSON、断开的 oneshot 与缺失预览文件；断言 context、Error::source 和 downcast。
- 原真实 MockRuntime IPC / Axum 全流程及错误与清理告警测试通过。生成 bindings 仅包含前置接线 PR 的 preview_id；Snafu 不增加前端字段。

机器的全局编译缓存曾返回损坏产物，以上命令通过临时 Cargo 配置关闭 rustc-wrapper 执行；未更改全局配置。Clippy 的 --config 参数置于 clippy 子命令之后。

本迁移不改变错误恢复策略，不把导出后的清理失败视为未导出，也不自动重试已接受操作。现有动态 anyhow 来源保持可下转型。
