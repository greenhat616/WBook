# Session 状态订阅与预览访问：需求

- 日期：2026-10-05
- 状态：已实施；证据见 [verification.md](verification.md)
- 前置：统一过程宏 RPC / IPC PR #906
- 设计：[design.md](design.md)；任务：[tasks.md](tasks.md)

## 范围

用户确认接线补充 Session 状态订阅和预览资源访问，作为独立 PR。命令仍通过已生成的 IPC / RPC 客户端；本机路径输入保持不变。完成后再搭建前端脚手架。不增加原生文件对话框、远程上传、持久化、结果重放或独立服务器。

## R1：最新状态订阅

- 桌面和浏览器共用 `GET /bridge/sessions/{session_id}/events` SSE；桌面通过 get_port 获取宿主地址，浏览器沿用 RPC URL 的源。
- 首次发送当前 SessionSnapshot，随后发送 core watch 的最新快照，附带 seq；不承诺逐个阶段或完整事件历史。
- Closed 快照发送后结束。不存在返回 404；无效 ID 返回 400。订阅取消只释放观察者，不取消业务操作。
- 前端显式管理退订；连接或协议失败关闭连接并报告错误，不自动重连。重新订阅取得最新快照；忽略重复或倒序 seq。
- 停机能够完成，长连接不得阻止已有 core / HTTP 有序关闭。

## R2：预览资源

- 每个成功新生成的预览有独立不透明 ID；相同参数复用保持 ID，替换即使 revision 相同也生成新 ID。
- WorkspaceStatus.preview_id 与 PreviewInfo.id 一致，供客户端在合并快照下判定预览是否仍然有效；无预览时为 null。
- `GET /bridge/preview/{session_id}/{preview_id}/{resource}` 只服务当前预览的已生成 XHTML 和 CSS，不接受任意本机路径。
- core 提供只读当前预览描述符；忙碌、关闭或 workspace 丢失遵循现有准入错误。描述符读取不持有锁进行文件 I/O。
- 编辑提交、预览替换或会话关闭后，新请求不能再读取旧 ID；无效或失败操作不凭空使原预览失效。已经获准的在途读取可完成。
- 禁止路径穿越、非清单文件和越界符号链接；响应带准确内容类型、no-store、nosniff 和禁止脚本的 CSP。

## R3：客户端与宿主

- 复用 Specta 生成的 SessionSnapshot / PreviewInfo，不手抄领域 DTO。
- 提供订阅与预览 URL 助手，可供后续路由和页面直接消费。
- Tauri 窗口使用 HTTP scheme，以允许本机 HTTP SSE 与 iframe；沿用已有 Host / Origin 限制，不扩大到任意来源。
- 本期不添加页面。后续 iframe 必须设置 sandbox，允许同源样式但禁止脚本。

## 验收

V1：初始快照、更新、seq、终态和断开释放；不存在、参数错误、Host / Origin 拒绝；实际监听器带活动 SSE 停机。

V2：预览正文与 CSS 可读取；未知 ID / 文件、穿越、符号链接拒绝；替换、编辑、关闭失效；缓存复用与失败保留。

V3：桌面 / 浏览器 URL 选择，显式退订、异常和终态释放、重复快照过滤；生成一致性、全量 TS 与构建、Rust 和前端回归。
