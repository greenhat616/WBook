# 前端工作台脚手架：验证

- 日期：2026-10-05
- 状态：实施与验证完成
- 对应：[requirements.md](requirements.md)；[design.md](design.md)；[tasks.md](tasks.md)

## 自动检查

| 检查                                             | 结果                                                          |
| ------------------------------------------------ | ------------------------------------------------------------- |
| `pnpm exec tsc --version`                        | 7.0.2；原生编译器保持可用                                     |
| `pnpm lint:ts`                                   | 完整前端类型检查通过，使用包含 preview_id 的生成绑定          |
| `pnpm exec eslint --no-cache .`                  | 通过；flat config、TS6 API 兼容别名与 React 规则适配生效      |
| `pnpm exec stylelint src/styles/global.css`      | 通过                                                          |
| `pnpm exec vitest run`                           | 4 个测试文件、41 项测试通过                                   |
| `pnpm build`                                     | Vite 生产构建通过                                             |
| `cargo run --example export_bindings -- --check` | 在 rpc-procedural 后端工作区通过，覆盖当前过程宏与 preview_id |
| 修改文件 Prettier 与 `git diff --check`          | 通过                                                          |

绑定使用后端导出的 `src/bindings.ts`，其中 `WorkspaceStatus.preview_id` 明确表示当前生成物身份。后端工作区已执行生成物一致性检查，同一生成文件复制到前端工作区并通过类型检查；该生成物属于前置后端 PR，不重复纳入前端提交。后端错误契约的完整验证随其独立变更记录。

Session hooks 的受控测试覆盖命令错误和操作 outcome 错误、失败时保留 cleanup warnings、串行操作、取消、操作进行中关闭、StrictMode 不重复写入、订阅释放和旧路由响应隔离。新增回归验证相同 revision / 选项但不同 preview_id 的合并 SSE 快照会清除旧预览，迟到的 render receipt 与旧刷新不会恢复它；Closed 确认快照也不会被迟到的 readResults 再次填充。

关闭反馈使用真实 SessionPage、useSession 与 TanStack 内存路由做页面级回归：带 cleanup_failures 的成功报告保留 Session 页面，告警路径和原因可见，手动返回有效；没有告警时自动返回首页。测试刻意不发送 Closed SSE 快照，验证关闭 RPC 先返回时所有会话操作已禁用，且文案不再提示可以预览或导出。另验证关闭失败或等待期间卸载时不返回可导航的成功报告。

## 真实本机流程

使用 agent-browser 的独立浏览器会话，以及临时测试宿主启动的真实 AppRuntime（127.0.0.1:1421）和 Vite（1420）。页面通过现有 RPC / SSE / 预览桥接访问 core，没有替换为成功假响应。测试目录为 `G:/Programs/Rust/.ccg/WBook/frontend-qa-20261005`。

1. 输入该目录下的 `input.txt`，分段数设为 2，创建 Session。
2. 显式初始化后显示书名“山间来信”、作者“林间”与两条目录。
3. 生成预览，访问正文与 `nav.xhtml`，预览样式正常加载。
4. 导出该目录下的 `book.epub`，文件实际存在，大小为 2387 字节；界面显示真实目的路径。
5. 再次导出到相同目标，页面显示操作层 `export` 错误，没有误报成功。
6. 关闭 Session 后回到首页，旧预览 URL 返回 404；原始输入和已导出的 EPUB 保留。
7. 停止测试宿主后刷新列表，页面显示连接失败并提供重试，没有将网络失败展示为空列表成功。

整个浏览器流程未出现 JavaScript 执行错误；停止宿主后出现的网络拒绝符合失败场景预期。QA 期间发现 Windows 后端构建目录文件锁导致 Vite EBUSY，已通过排除 backend 文件监听修复。

## 布局、键盘与动效

- 390×844 手机尺寸和 800×600 桌面尺寸未出现横向溢出；首页、Session、导出与手机截图经过目视检查。
- `/#/sessions/nope` 显示找不到工作区及返回工作台入口，键盘可激活返回链接。
- 键盘聚焦“跳到主要内容”并按 Enter 后，焦点位于 `main-content`，路由 hash 保持 `#/`。
- 启用 prefers-reduced-motion 后媒体查询返回 true，页面活动动画数为 0。

截图保存在本机测试目录的 `screenshots` 子目录，没有作为产品资源提交：`home.png`、`session-export.png`、`session-mobile.png`、`session-800.png`、`disconnected.png`、`keyboard-focus.png`。

## 验证边界

本轮未运行原生 Tauri GUI；真实流程覆盖的是浏览器通过本机服务访问同一 AppRuntime / core。活动取消的竞态由 hooks 与 core 测试覆盖，未声称通过短文本手动操作稳定复现长任务取消。

仓库全量 stylelint 仍在既有 core EPUB 模板 CSS 中报告 15 项问题，本次修改的前端 CSS 检查通过；未为清除这些既有问题修改导出模板，也未将全量 `pnpm lint` 记录为通过。

当前工作台不持久化历史会话，不包含正文编辑器、完整规则 / 元数据编辑器或原生文件对话框。路径输入明确指向本机文件，尚未提供远程上传流程。
