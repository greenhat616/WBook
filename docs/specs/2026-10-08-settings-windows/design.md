# 设置多窗口：设计

- 状态：已实施并验证
- 需求：[requirements.md](requirements.md)；任务：[tasks.md](tasks.md)；证据：[verification.md](verification.md)

## 同步方案调研：浏览器设施

2026-10-08 在本机实测两个 Tauri 窗口（主窗口 + 会话窗口）之间的浏览器设施。方法：以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` 启动应用，通过 CDP 在两个页面中执行脚本：A 写入 `localStorage` / `sessionStorage` 并经 `BroadcastChannel` 发消息，B 监听 `storage` 事件与同名频道并读取存储；反方向再发一次。

环境：Windows 11（10.0.26300），WebView2 运行时 Edg/154.0.0.0，Tauri 2，两个窗口均为 `useHttpsScheme: false`。

| 设施                      | dev（`http://localhost:1420`） | custom-protocol 构建（`http://tauri.localhost`） |
| ------------------------- | ------------------------------ | ------------------------------------------------ |
| `localStorage` 跨窗口读取 | 共享                           | 共享                                             |
| `storage` 事件            | 另一窗口收到                   | 另一窗口收到                                     |
| `sessionStorage`          | 不共享（B 读到 `null`）        | 不共享（B 读到 `null`）                          |
| `BroadcastChannel` 双向   | 收到                           | 收到                                             |

结论：

- `sessionStorage` 按顶层浏览上下文隔离，每个 Tauri 窗口各自一份，**不能**用于跨窗口同步。
- `localStorage` + `storage` 事件、`BroadcastChannel` 在 Windows 上可以跨窗口工作（两个窗口同源且共用同一 WebView2 用户数据目录）。
- 未验证：macOS（WKWebView）与 Linux（WebKitGTK）。本机无法测试，这两个平台上的跨窗口行为在本设计中视为未知。

不采用浏览器设施作为同步机制：

1. 设置的真实来源在后端（`settings.toml` 与 workspace），浏览器消息只能转告“本窗口保存过”，覆盖不到后端发起的变化，也覆盖不到另一浏览器或另一台客户端的标签页。
2. macOS / Linux 行为未验证；即使可用，也要和后端推送维护两条通知路径。
3. 后端已有同类推送：Session 快照经 `/bridge/sessions/{id}/events`（SSE）送达桌面与浏览器两种模式，本书设置的变化已经通过快照 revision 传播。

## 同步：后端推送

### 本书设置

沿用现有机制：`set_session_settings` 递增 workspace revision → 快照 SSE → `useSession` 按 revision 重新读取 `get_session_settings`。并发保存已由 expected revision 拒绝（`stale_revision`）。本次只补前端表单的“外部更新”处理（见下）。

### 全局设置

`StoredSettings` 增加 `revision: u64`：进程内从 0 开始，每次成功保存 +1，不写入文件（多进程同时写同一文件不在支持范围内）。

`SettingsStore` 增加 `watch::Sender<StoredSettings>` 负责发布；现有 `Mutex` 保留，只用于串行化保存，维持“文件与最后一次存储值一致”的不变量：

- `get()` 读取 watch 当前值；`subscribe()` 返回接收端。
- `save(expected, settings)`：校验 → 取保存锁 → 比较 revision，不一致返回 `SaveError::Stale`（映射为现有 `ErrorKind::StaleRevision`）→ 写文件 → `send_replace` 新值 → 释放锁。写文件失败时不更新值、不广播。
- 命令 `save_settings(expected, settings)` 带上 revision；绑定重新生成。
- `closing()` 返回一个在 `Wbook::shutdown` 时取消的 `CancellationToken`。Session 快照流在 Session 关闭时自然结束，设置存储却与进程同寿；订阅流不靠它结束的话，HTTP 服务的优雅关闭会一直等待已连接的设置订阅。

新增 SSE `/bridge/settings/events`，与 Session 快照流同一写法：先发当前值，之后每次变化发一条 `settings` 事件，`id` 为 revision；序列化失败发 `bridge-error` 并结束；应用关闭时结束。桌面与浏览器模式共用。

不采用 Tauri 事件：只覆盖桌面，浏览器模式仍需另一条路径。

### 前端

- `bridge.ts` 把 EventSource 的连接、校验、单调序号与错误处理抽成内部 `subscribe`，`subscribeSession` 与新增的 `subscribeSettings` 共用；设置流按 revision 单调，不会自行结束。
- `useSettingsUpdates` 订阅后写入 `queries.getSettings()` 的缓存（只接受不旧于缓存的 revision，保存响应与推送谁先到都一样）；断线时在设置页提示并提供“重新连接”，不阻止编辑。
- `useSyncedForm(saved, codec)` 承载“外部更新”规则，`SettingsScreen` 与 `ParserPanel` 共用。它按**设置内容**而不是 revision 判断外部变化：Session 的 revision 会因编辑正文、解析等与设置无关的操作递增，按 revision 判断会误报。`saved` 变化时：
  - 新值等于表单起点或等于当前表单（例如自己的保存回传）→ 只前移起点；
  - 表单无修改 → 用新值重建表单；
  - 表单有修改 → 保留输入，标记 `outdated`：提示“已在其他窗口更新”，保存 / 试解析禁用，“撤销修改”变为“载入最新”。
- 由于过期表单在前端就不能保存，全局设置保存时以缓存中最新的 revision 作为 expected；推送与点击之间的竞争仍由后端的 revision 检查拒绝。Session 设置沿用保存时快照的 revision。

## 窗口

在 `windows.rs` 中把现有 `open` 的建窗部分（隐藏创建、`use_https_scheme(false)`、`ready` 显示、超时兜底、并发建窗的标签冲突处理）抽成 `open_window(app, label, route, title)`，Session 窗口、全局设置窗口、本书设置窗口共用。

| 窗口     | 标签                 | 路由                              | 关闭                         |
| -------- | -------------------- | --------------------------------- | ---------------------------- |
| 全局设置 | `settings`           | `index.html#/settings`            | 直接关闭                     |
| 本书设置 | `session-settings-N` | `index.html#/sessions/N/settings` | 直接关闭；Session 关闭时销毁 |

- 命令：`open_settings_window()`、`open_session_settings_window(session_id)`，均为 `#[desktop_only]`。后者先按 Session 窗口同样的规则检查生命周期（Closing / Closed 拒绝），建窗后启动 `until_closed` 监视，Session 关闭时销毁该窗口。
- `session_of("session-settings-1")` 必须返回 `None`，否则 `on_window_event` 会把关闭设置窗口当作关闭 Session；加测试锁定。
- capability `migrated.json` 的 `windows` 增加 `settings`、`session-settings-*`（`session-*` 已能匹配后者，显式列出以免日后收窄通配符时遗漏）。
- 退出：主窗口关闭时的退出流程已销毁所有窗口，无需改动。

## 前端入口与外观

- 桌面端：App Bar 设置按钮调用 `openSettingsWindow()`；工作区“本书设置”调用 `openSessionSettingsWindow(id)`；失败时在当前页提示。浏览器端仍为路由链接。
- 设置窗口内不显示 App Bar 的首页 / 设置导航，只保留标题；关闭使用系统标题栏。桌面端的入口不再在窗口内导航到设置路由，因此 `AppShell` 以“桌面 + 路由以 `/settings` 结尾”识别设置窗口，无需读取窗口标签。页内“关闭设置”返回按钮只在浏览器模式显示（桌面窗口没有可返回的历史，关闭窗口还需额外的 `core:window:allow-close` 权限）。
- 窗口标题：全局设置为“设置”；本书设置为“本书设置 · 文件名”。
