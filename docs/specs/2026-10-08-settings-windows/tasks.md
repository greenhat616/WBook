# 设置多窗口：任务

- 状态：已实施并验证
- 证据：[verification.md](verification.md)

1. [x] core：全局设置 revision、`watch` 订阅、带 expected revision 的保存与 `Stale` 错误；测试覆盖过期保存不落盘、订阅收到初始值与更新。
2. [x] tauri：`save_settings` 带 revision；`/bridge/settings/events` SSE；重新生成绑定；HTTP 测试覆盖订阅流与冲突。
3. [x] tauri：抽出 `open_window`；`open_settings_window` / `open_session_settings_window`；Session 关闭销毁本书设置窗口；capability；`session_of` 拒绝设置窗口标签的测试。
4. [x] 前端：`subscribeSettings`；全局设置页接入订阅；`SettingsScreen` 与 `ParserPanel` 的外部更新 / 冲突处理及测试。
5. [x] 前端：桌面端设置入口改为打开窗口，设置窗口精简外观；浏览器端保持路由并保留返回按钮；测试。
6. [x] 验证与记录：自动检查；本机桌面流程（三窗口并排、跨窗口保存、Session 关闭连带销毁、并发保存冲突）；浏览器模式双标签页同步。
