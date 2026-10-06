# Snafu 领域错误：需求

- 日期：2026-10-05
- 状态：已实施；证据见 [verification.md](verification.md)
- 范围：用户确认同时迁移 core 领域错误与 Tauri / RPC 适配错误。
- 设计：[design.md](design.md)；任务：[tasks.md](tasks.md)

## R1：领域契约

core 使用 Snafu 表达范围、目录、提取、解析、文档、流水线、导出、工作区、会话和管理器的领域错误。保留各领域错误分类、取消判定、操作凭据、revision 与清理告警语义，不将错误统一成字符串或 Whatever。错误来源通过标准 Error::source 保留。

## R2：上下文与传输边界

在有实际底层来源的边界使用 context / with_context 添加有意义的操作上下文。JSON 解码 / 编码、Receipt 接收、预览资源 I/O 使用内部 Snafu 错误；仅在 IPC / HTTP 边界转为稳定 CommandError {kind,message}。不把 source、backtrace 或运行时句柄加入生成的前端 DTO。

## R3：兼容与范围

维持 HTTP 状态、CommandError.kind、outcome、warning 和成功 DTO 形状。已有用户可辨识错误信息保持含义；上下文可使适配错误信息更具体。Rust 源码错误 payload 可为 Snafu 的命名 source 字段调整，本仓库消费者和测试一起迁移；不宣称 Rust enum 形状零变更。不重构无关业务算法、清理机制或锁。

## 验收

V1：所有既有 core / 跨传输测试继续通过，保证取消、panic、预览、关闭、导出失败和警告不变。

V2：新增源链 / 下转型断言与实际失败上下文验证；多层领域错误保持底层类型。IPC / HTTP 的 kind 和 JSON DTO 不暴露内部 source。

V3：过程宏生成的参数解码和响应编码通过相同错误边界；绑定 --check、完整编译与 Clippy 通过。
