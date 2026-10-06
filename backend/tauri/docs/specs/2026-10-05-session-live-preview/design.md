# Session 状态订阅与预览访问：设计

- 状态：已实施；证据见 [verification.md](verification.md)
- 需求：[requirements.md](requirements.md)

## 状态流

Tauri 层 subscriptions 模块为 Arc<Wbook> 装配独立 Router，再合入 RPC Router。SSE body 自己持有 watch::Receiver，不创建后台转发任务或订阅注册表。使用 borrow_and_update / changed 形成最新状态流；Closed 后终止。每条快照使用已有安全整数检查。正常事件名称为 session，id 为 seq；序列化或范围错误以 bridge-error 事件结束。

浏览器 EventSource 和桌面 WebView 共用传输。命令依旧由生成客户端选择原生 IPC 或 fetch。客户端首个网络错误即关闭，避免默认自动重连造成 404 循环；重新连接由消费者主动发起，不重放历史。

## 预览所有权

Workspace 的 Preview 继续独占 RenderedBook 与临时目录。新增 UUID 字符串 id，仅在成功发布新 Preview 时生成；PreviewInfo 暴露 id。SessionHandle::current_preview 只在 Open / Idle 状态返回克隆的描述符，保证不泄漏内部 Workspace 引用。

WorkspaceStatus 同时携带 preview_id；无预览时为 null，替换时更新，业务提交使预览失效时清空。前端按 revision 与 preview_id 判断现有预览是否有效，不能仅比较 options：watch 可以合并中间状态，同一 revision 的 A → B → A 渲染会产生新 ID。Closed 快照仍保留最后提交状态，消费者必须同时检查 lifecycle。

Axum 从 core 查询描述符、核对 ID、检查资源白名单和规范路径，再异步读取文件。允许 files 中列举的正文，以及当前渲染器固定生成的 nav.xhtml、styles/book.css。禁止绝对路径、空段、点段、反斜线、冒号和 NUL；canonicalize 后仍须位于目录内。请求通过描述符检查后遇到并发清理可返回未找到，不承诺读事务或延长预览生命期。

资源响应设 Cache-Control: no-store，避免关闭后浏览器继续把旧资源当作当前有效内容；Content-Type 按白名单类型决定，设置 nosniff 和 CSP 禁止脚本、表单、外部资源及 base URL。CSP sandbox allow-same-origin 与后续 iframe sandbox allow-same-origin 配合，让同源 CSS 请求继续通过 Host / Origin 检查，不允许脚本。

## 地址与宿主

bridge 客户端助手解析统一资源源：Tauri 调用 get_port 得到 http://127.0.0.1:{port}；浏览器从 VITE_WBOOK_RPC_URL 或同源 /bridge/rpc 得到源。路径固定为 /bridge 下，不支持反向代理子路径配置。

Tauri 官方配置说明 useHttpsScheme 会限制混合 HTTP 端点，因此窗口改为默认 HTTP scheme。当前没有依赖 HTTPS scheme 的应用功能或浏览器存储；这项变更使本机 SSE 与预览在桌面端采用相同访问路径。此配置改变了 Windows / Android 的 WebView 存储源，不能在已有持久化数据的版本间无迁移切换。参考：https://v2.tauri.app/reference/config/#windowconfig 。

## 验证边界

core 测试覆盖 ID 生命周期和只读访问；Tauri 路由测试覆盖实际资源、SSE 与入口限制；客户端测试模拟 EventSource 生命周期。实际 TCP 测试证明长连接不阻塞关机。此处测试不能替代后续页面的浏览器视觉和交互检查，也不宣称 GUI E2E 通过。
