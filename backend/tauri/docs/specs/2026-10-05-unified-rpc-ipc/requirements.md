# Tauri / Axum 统一命令：需求与验收条件

- 日期：2026-10-05
- 状态：已实施；验证证据与边界见 [verification.md](verification.md)
- 设计：[design.md](design.md)
- 实施与验证：[tasks.md](tasks.md)

## 目标与当前事实

参考 `G:/Programs/Rust/.ccg/gety/bounded-subtree-delete/docs/reports/unified-rpc-ipc-design.md`，以同一份 Rust 命令声明接入 Tauri IPC、Axum HTTP RPC 和 Specta TypeScript 客户端。沿用 core 的 requirements / design / tasks / verification 工作流，先定义契约再实施。

实施前 core 已提供 Wbook、SessionManager、SessionHandle 及可序列化领域 DTO；Tauri 只有 get_port，server 只有问候与 WebSocket echo。现有页面仍是模板。本期接入公开 Session 操作，不实现编辑器 UI、SSE / Tauri Event、预览文件服务、独立服务端部署、远程访问、持久化或操作结果重放。

## R1：单一命令与共享状态

- 一份命令声明生成共享函数、Tauri wrapper、HTTP 参数解码与分发、Specta 收集；两种入口注入同一个 Arc<Wbook>。
- 接入 create_session、list_sessions、get_session、close_session、initialize_session、parse_session、install_results、apply_edits、set_metadata_overrides、read_text、read_results、render_preview、export_epub、cancel_operation。
- get_port 保留原 IPC 名称与 u16 返回类型并导出类型；HTTP 明确返回 platform_unsupported。
- 共享实现仅调用 core 公开 API，不复制工作区状态机、版本规则或文件处理逻辑。

## R2：调用与结果契约

- HTTP 使用 POST /bridge/rpc，请求为 `{ method, params }`；省略 params 等于空对象，显式非对象无效。命令顶层参数 camelCase，嵌套 DTO 沿用 Serde 命名。
- 成功 HTTP 返回裸命令值；命令失败返回与 IPC 相同的 CommandError（kind、message），HTTP 按错误类别选择状态码。请求 JSON 提取失败也返回该错误形状。
- 长操作在请求中等待 Receipt；返回 op、kind、revision、outcome 和 warnings，业务失败 / 取消 / panic 仍保留凭据元信息。准入拒绝与不存在的 Session 作为命令错误。
- 不增加任务缓存和后台结果查询。快照可以查询活动 OperationId 后另发 cancel；断开连接不自动取消，也不保证可重新取得操作结果。
- u64 / usize 在 TypeScript 为 number；边界拒绝超出 JavaScript 安全整数范围的输入 / 输出，禁止静默舍入。序列化失败不伪造成功。

## R3：类型生成与前端传输

- 固定兼容的 Specta / tauri-specta 预发布版本；命令及 DTO 类型来自 Rust。
- 提供可重复执行的生成命令与已生成文件；检测生成物漂移。传输入口替换必须精确匹配一次，否则生成失败。
- 客户端自动选择 Tauri invoke 或 HTTP fetch；浏览器调用 get_port 明确失败。HTTP 非 JSON、网络失败、入口拒绝和无效错误响应保留为 Error rejection，不伪装为领域错误；不自动重试。没有对所有成功 DTO 做运行时 schema 验证。
- 生成客户端与独立适配器通过 TypeScript 检查和双传输测试。

## R4：启动与退出

- 监听器先绑定再发布实际端口；只监听 127.0.0.1，默认系统分配端口，可用 WBOOK_RPC_PORT 指定开发端口。
- 服务端校验实际 Host 及允许的完整 Origin，移除任意 Origin 授权。debug 构建的开发源仅 http://localhost:1420 / http://127.0.0.1:1420，允许当前 RPC 同源与 Tauri 官方本地源。不提供远程认证。
- Tauri 初始化 Wbook，并在退出时先停止 HTTP 接收、请求所有 Session 关闭、等待 core shutdown 和 HTTP 在途请求收尾，再退出；重复退出事件不重复启动关闭流程。

## 验收

V1：真实 Tauri mock IPC 与 Axum Router 使用同一状态，跨传输创建 / 查询 / 编辑 / 关闭，返回值与错误形状一致。

V2：覆盖完整 Session 命令链、版本冲突、非法参数、未知命令、桌面专属命令、无效 JSON、安全整数边界；保留操作失败的 op / revision / warnings。

V3：Host / Origin 拒绝发生在命令执行前；绑定的实际端口可读；服务器可有序关闭。

V4：生成物一致、无 DTO 手抄、TS 检查、传输测试与前端构建；运行 core 回归和 workspace check / clippy。如遇既有阻塞，如实记录。
