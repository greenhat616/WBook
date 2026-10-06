# Session 工作区多窗口：验证

- 日期：2026-10-06
- 需求：[requirements.md](requirements.md)
- 环境：Windows x64，Rust nightly 1.101，Tauri 2.11.6，pnpm 12.5.1

## 实施结果

windows 模块以 `session-{id}` 标签把窗口与 Session 一一对应，Tauri 窗口表是唯一注册表。open_session_window 为仅桌面命令，经类型擦除的 SessionWindows 状态访问 AppHandle；已有窗口聚焦，并发创建的标签冲突转为聚焦。创建成功后看守任务等待 Session Closed 再销毁窗口。Session 窗口的关闭请求先完成 core 关闭再销毁；主窗口的关闭请求转为应用退出，复用现有停机流程。capability 覆盖 `session-*`。

主页在桌面端创建 Session 或点击列表时打开 / 聚焦其窗口，浏览器端保持页内导航。

## 执行证据

- `cargo test --manifest-path backend/Cargo.toml --workspace --all-features`：174 个 core、24 个 Tauri（新增 4 个）、6 个过程宏、3 个 server 测试通过。
- `cargo clippy --all-targets --all-features`：无代码警告；仅有既有的 manifest 未使用依赖警告。
- 修改的 Rust 文件 rustfmt 检查通过。
- `pnpm bindings:generate` 后 `pnpm bindings:check` 通过；生成文件新增 openSessionWindow，DESKTOP_ONLY_COMMANDS 含 open_session_window。
- `pnpm lint:ts`、`pnpm bindings:typecheck`、`pnpm build`：通过。
- `pnpm exec vitest run tests`：4 个文件、44 个测试通过，其中 3 个为本期新增主页测试。修改的 TypeScript 文件 Prettier 与 ESLint 通过。

### 真实事件循环检查

临时 `#[ignore]` 测试（未入库）以 `Builder::any_thread()` 在 Wry 上运行与 `run()` 相同的命令、窗口事件和退出处理，并在桌面上实际创建窗口。开发服务器未启动，页面内容加载失败不影响窗口生命周期断言。结果：

| 场景                                                | 结果                                     |
| --------------------------------------------------- | ---------------------------------------- |
| 同一 Session 连续打开两次                           | 窗口总数 2（主窗口 + 1 个 Session 窗口） |
| 对 Session 窗口调用 close() 产生真实 CloseRequested | Session 变为 Closed，窗口随后销毁        |
| 从 core 关闭另一个 Session                          | 看守销毁其窗口，主窗口保留               |
| 打开第三个 Session 后关闭主窗口                     | 停机关闭该 Session，run_return 返回 0    |

## 验收对应

| 场景 | 证据                                                                                                                                                                                   |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| V1   | 标签往返与 8 种非法标签；IPC 打开两次仅 1 个 Session 窗口，URL 为 http scheme、片段 `/sessions/1`；关闭后与不存在的 Session 返回 not_found 且不建窗口；RPC 返回 platform_unsupported。 |
| V2   | until_closed 在关闭前挂起、关闭后完成，已关闭时立即完成；close_session 关闭 Session，对已移除或不存在的 Session 无错误；真实事件循环检查覆盖销毁与退出顺序。                           |
| V3   | 桌面创建与列表点击调用 openSessionWindow 且路由停留在首页；打开失败显示错误；浏览器导航到工作区路由且不调用窗口命令。                                                                  |

## 边界

SessionManager 在关闭后立即移除 Session，因此打开已关闭 Session 实际返回 not_found；Closing / Closed 分支只在关闭竞争窗口中出现，未单独构造。窗口标题未断言（MockRuntime 不记录标题），真实检查中未读取。未执行带前端页面的桌面 GUI E2E；主窗口列表靠聚焦重新获取刷新，未做实时推送。macOS / Linux 未运行。

本机全局 kache 编译器 wrapper 曾产出损坏的 rlib，本次 Rust 命令均以 `RUSTC_WRAPPER=` 关闭该 wrapper 执行。
