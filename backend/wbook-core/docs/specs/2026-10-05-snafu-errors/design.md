# Snafu 领域错误：设计

- 状态：已实施；证据见 [verification.md](verification.md)

core 每个领域保留自己的枚举，用 derive(Snafu) 和明确 display 表达错误。原透明包装改用命名 source、context(false) 与 display 转发，保留 From 和每一层实际来源；Snafu 的 transparent 会委托 Error::source，因而会跳过当前来源层，不符合这里的下转型要求。Leaf 错误保留已有结构化数据。错误分类仍由枚举匹配决定，禁止依赖 message 文本判断取消或 HTTP 状态。

导出错误继续持有 stage、source 和 cleanup_failures；Workspace / Session 分层包装不得吞掉这组诊断。DTO 转换先提取 cleanup warnings，再映射 outcome。迁移所有 enum 构造、模式匹配和 From 转换，并验证底层来源可检索。

Tauri 增加内部 BridgeError：参数解码记录命令名，响应编码记录操作上下文，Receipt 接收记录 operation ID，文件 I/O 记录动作和路径；使用 Snafu selectors。内部错误转换到 CommandError 时按变体稳定映射 kind。预览不存在继续为 not_found，其余 I/O 为 internal_error。过程宏调用共享边界辅助函数而不复制 Snafu 变体名。

CommandError 是 wire DTO，内部 source 链不序列化。不会为了使用 Snafu 给无来源的字符串重新构造虚假 source，也不引入通用动态错误容器或无必要 backtrace。

参考：https://docs.rs/snafu/0.9.2/snafu/ 与 https://docs.rs/snafu/0.9.2/snafu/trait.ResultExt.html 。
