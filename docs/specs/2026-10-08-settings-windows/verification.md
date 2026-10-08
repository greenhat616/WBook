# 设置多窗口：验证

- 日期：2026-10-08
- 状态：已实施并验证
- 对应：[requirements.md](requirements.md)；[design.md](design.md)；[tasks.md](tasks.md)

## 自动检查

| 检查                   | 结果                                            |
| ---------------------- | ----------------------------------------------- |
| `pnpm test:backend`    | 230 项通过                                      |
| `pnpm lint`            | 通过（仅既有的未使用依赖 / 字段 manifest 告警） |
| `pnpm bindings:check`  | 通过                                            |
| `pnpm exec vitest run` | 9 个测试文件、81 项通过；Node.js 24 下同样通过  |
| `cargo fmt --check`    | 通过                                            |

新增覆盖：

- core：过期 revision 的保存被拒绝且文件不变；订阅者收到初始值与每次成功保存，失败的保存不广播；非法设置不递增 revision。
- tauri：`save_settings` 的过期保存经 IPC 与 HTTP 均返回 `stale_revision`（409）且不落盘；设置 SSE 发送初始值与保存、断线重连取到最新值、`shutdown` 后结束；两个设置窗口重复打开只建一个、路由正确；Session 不存在或已关闭时拒绝打开本书设置窗口；新命令经 HTTP 调用返回 `platform_unsupported`；`session_of` 拒绝 `settings` 与 `session-settings-N`。
- 前端：设置流的地址、单调 revision、非法数据与断线；全局设置页跟随外部保存、编辑中保留输入并禁止保存、“载入最新”、自己的保存回传不误报、断线提示与重连、以最新 revision 保存；解析面板同一规则；桌面端 App Bar 与工作区入口调用打开窗口命令且不改路由、失败提示；设置窗口不显示导航与返回按钮；浏览器端保持路由。

tauri 的 mock runtime 销毁窗口时不发出 `Destroyed`，窗口仍留在应用的窗口表中，因此“Session 关闭时销毁本书设置窗口”只能在真实桌面流程中验证（见下）。

## 真实本机流程

### 桌面（Windows 11，WebView2 Edg/154）

`custom-protocol` 构建，以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` 启动，脚本经 CDP 操作各窗口的 DOM。全局设置写入真实配置目录，测试前备份 `settings.toml`，结束后恢复并核对 sha256 一致。24 项检查全部通过：

1. 工作区“本书设置”打开 `session-settings-1` 窗口，重复点击不新建；工作区窗口路由不变；设置窗口无返回链接、无首页导航。
2. 会话窗口与主窗口的 App Bar 设置按钮打开同一个全局设置窗口，不新建、不改各自路由；全局设置窗口无关闭按钮与设置按钮。
3. 主窗口经 IPC 保存全局设置：未编辑的全局设置窗口随之更新；编辑中的窗口保留输入、提示“设置已在其他窗口更新”、保存禁用；“载入最新”取回存储值；过期 revision 的保存返回 `stale_revision`。
4. 本书设置窗口保存解析规则，工作区“解析规则”面板随之更新；面板编辑中时，设置窗口保存不同的章节标记，面板提示更新并禁用“试解析”。
5. 关闭 Session 后，本书设置窗口与工作区窗口均被销毁，全局设置窗口保留。
6. 应用日志无警告或错误。

### 浏览器

临时测试宿主（未提交的 ignored 测试）在 127.0.0.1:1421 启动真实 AppRuntime，配置目录位于临时目录；Vite 运行在 1420；headless Edge 打开四个标签页。6 项检查全部通过：浏览器设置页保留“关闭设置”；一个标签页保存全局设置，另一标签页更新；编辑中的标签页保留输入并提示；本书设置页保存的解析规则到达另一标签页的解析面板；无未捕获的 JavaScript 异常。

## 未覆盖

- macOS 与 Linux 的桌面窗口未运行；同步不依赖浏览器存储，平台差异只在窗口创建，与现有 Session 窗口相同。
- 通过系统标题栏关闭设置窗口未在真实流程中操作（CDP 无法发出窗口关闭请求，页面也没有关闭窗口的权限）；由 `session_of` 的标签测试保证不会被当作关闭 Session。
