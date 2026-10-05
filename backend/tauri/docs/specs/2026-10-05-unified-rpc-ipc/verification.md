# Tauri / Axum 统一命令：验证记录与使用

- 日期：2026-10-05
- 需求：[requirements.md](requirements.md)
- 设计：[design.md](design.md)
- 任务：[tasks.md](tasks.md)
- 环境：Windows x64；rustc 1.101.0-nightly (c1070d693 2026-09-28)；pnpm 12.5.1；TypeScript 7.0.2。

## 交付范围

先创建本目录 spec，再实施 Tauri 命令、DTO、Axum RPC、共享 Wbook 生命周期、Specta 导出和前端传输。没有改动 core 的业务代码。命令由独立 proc-macro crate 的 `#[unified_commands]` 模块属性宏生成两种入口和 Specta 注册，新增命令无需分别维护 HTTP 与 IPC 列表。

已接入 14 个 Session 命令和桌面专属 get_port。运行中的同一 Wbook 为两个入口提供状态；HTTP 使用 POST /bridge/rpc，顶层参数 camelCase，嵌套参数沿用生成类型中的 Serde 名称。长操作返回带 outcome 的 OperationResponse；外层 Result 表示调用准入，outcome 表示已接受操作的结果。

## 调用方式

仓库根目录执行：

```powershell
pnpm bindings:generate
pnpm bindings:check
pnpm bindings:typecheck
```

生成物为 `src/bindings.ts`，不要手工编辑。导出器先生成到临时目录，再检查唯一 invoke import 并替换到 `./transport`；匹配失败不会覆盖已生成文件。固定使用 Specta / tauri-specta =2.0.0-rc.25 与 specta-typescript =0.0.12，依据安装版本的公开 API 接线；升级时需重跑生成检查和测试。

桌面从同一模块导入 commands，底层自动使用 invoke：

```typescript
import { commands } from './bindings'

const opened = await commands.createSession('G:/books/input.txt', {
  filters: [],
  toc: { SplitEvenly: { parts: 1 } }
})
if (opened.status === 'error') throw new Error(opened.error.message)

const initialized = await commands.initializeSession(opened.data.session)
if (initialized.status === 'error') throw new Error(initialized.error.message)
if (initialized.data.outcome.status === 'error') {
  throw new Error(initialized.data.outcome.error.message)
}
```

HTTP 由桌面应用宿主启动，默认监听系统分配的本地端口，可通过 commands.getPort() 获取。浏览器开发可在启动现有 Tauri 开发流程前设置：

```powershell
$env:WBOOK_RPC_PORT = '1421'
$env:VITE_WBOOK_RPC_URL = 'http://127.0.0.1:1421/bridge/rpc'
```

Vite 读取 VITE_WBOOK_RPC_URL；未配置时使用同源 `/bridge/rpc`。服务仅绑定 127.0.0.1，debug 构建允许 http://localhost:1420 与 http://127.0.0.1:1420 的浏览器页面。当前 server 不托管前端静态文件，release 默认允许 RPC 同源和 Tauri 源；需要浏览器生产部署时应另行设计静态托管。

HTTP 请求示例：

```http
POST /bridge/rpc
Content-Type: application/json

{"method":"get_session","params":{"sessionId":1}}
```

成功返回裸 SessionSnapshot；不存在时 HTTP 404 返回 `{ "kind": "not_found", "message": "session not found" }`。生成客户端对两个传输均包装为 `{ status: 'ok', data }` / `{ status: 'error', error }`。get_port 保持 Promise<number>，浏览器调用以 platform_unsupported 拒绝。网络、非 JSON 响应、HTTP 入口拒绝及 Tauri 框架字符串错误仍拒绝为 Error。

## 执行结果

以下完整检查在统一接入阶段执行。过程宏整理为原子提交时，在独立 `rpc-clean` worktree 重跑 workspace 全特性测试、生成物 `--check`、修改 Rust 文件格式检查与 diff 检查；185 个 Rust 测试通过。core 代码与前端生成物相对原统一接入版本保持不变，前端检查沿用该版本的结果。

```powershell
cargo test --manifest-path backend/Cargo.toml --workspace --all-features --locked --offline
cargo check --manifest-path backend/Cargo.toml --workspace --all-targets --all-features --locked --offline
cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets --all-features --locked --offline
cargo run --manifest-path backend/Cargo.toml -p wbook --locked --offline --example export_bindings -- --check
pnpm bindings:typecheck
pnpm lint:ts
pnpm exec vitest run tests/utils
pnpm build
git diff --check
```

- Rust：6 个过程宏测试、172 个 core 测试与 7 个 Tauri 适配测试，共 185 个测试通过；server / main / doc-tests 无测试，正常结束。
- workspace check / clippy：通过。仅保留既有 manifest / unused-dependency 警告；本次新增的 type_complexity 警告已消除。
- 生成物一致性：通过；生成客户端、适配器与传输测试的独立 TS 检查通过。
- 全量前端类型检查与生产构建通过；TypeScript 7、Tailwind 4 和 Tauri 2 配置修复作为独立前置提交。
- Vitest：2 个文件、12 个测试通过，其中 11 个为新增传输测试，另 1 个为既有测试。保留已有 Vite configLoader 导入路径警告。
- 修改的 Rust 文件单独 rustfmt --check 通过；手写 TypeScript 文件经 Prettier 检查；git diff --check 通过。没有对已有 core 文件做全量格式化。

Windows 上，tauri-build 默认只给应用二进制链接 Common Controls v6 manifest，导致新导出工具与 lib 测试在启动前报 STATUS_ENTRYPOINT_NOT_FOUND。build.rs 改为 MSVC 链接器统一嵌入同一依赖，避免二进制重复资源；导出、测试与全工作区编译已验证。移除重复的 lib / rlib 声明，保留 rlib / staticlib / cdylib。

## 验收证据

| 场景 | 测试与证据 |
| --- | --- |
| V5 过程宏 | 6 个过程宏测试覆盖普通 / 桌面专属声明、同步零参数 wrapper、签名诊断、模块约束、原函数作用域 / 属性和内部标识符冲突，原跨传输测试验证展开后真实接线。 |
| V1 / V2 双传输全链 | `ipc_and_http_share_the_complete_session_pipeline` 经真实 Tauri mock invoke_handler 和 Axum Router 交替完成创建、列举、快照、初始化、读取、编辑、解析、安装、overrides、预览、EPUB 导出、取消查询和关闭。比较同一 Session 的快照与错误；关闭后输入 / 输出保留且预览目录删除。 |
| V2 失败与参数 | `rejected_requests_and_failed_operations_preserve_their_contract` 验证配置失败不注册、camelCase、错误类型、非法 JSON、缺省 / null params、未知 / 桌面专属命令、读取不存在文件时凭据元信息、shutdown 后拒绝创建。 |
| V2 精度与告警 | `integer_limits_and_admission_error_categories_are_explicit` 覆盖嵌套安全整数边界与准入错误映射；`failed_exports_preserve_receipt_metadata_and_both_warning_sources` 验证取消、panic 分类，以及 Workspace warning 与 ExportError cleanup_failures 同时保留。 |
| V3 入口限制 | `host_and_full_origin_are_checked_before_dispatch` 验证错误 Host、不同端口 / scheme、null / 外部 Origin 返回 403 且未创建 Session；合法预检精确回传 CORS 源。 |
| V3 实际监听 / 关闭 | `runtime_publishes_the_bound_port_and_shuts_down_sessions_and_http` 绑定真实随机端口，以 TCP HTTP 请求取得 Session，等待 shutdown 后 Session 为 Closed、监听器不再可连接。 |
| V4 生成 / 客户端 | 唯一导入替换测试、生成 --check、TS 检查；11 个 Vitest 测试覆盖双传输选择、Result 包装、get_port、同源 / 配置端点、错误回退与安全整数。 |

跨传输测试未依赖 GUI 窗口运行，实际使用 Tauri MockRuntime 的命令解析和回调。未手动打开桌面窗口或浏览器页面，未宣称完成 UI E2E。活动操作的 Busy / cancel / panic / close 竞争由本次通过的既有 core 测试覆盖；新增传输测试对 cancel 验证的是 NotActive 路径，不宣称单独复现了所有活动操作竞争。导出错误告警转换使用确定的 OperationResult 测试输入，不伪称实际注入了 Windows 清理故障。

## 既有检查问题与边界

全量 ESLint 仍受既有配置格式阻塞：ESLint 10 不再读取 .eslintrc.cjs。全量 Stylelint 在 core 的 EPUB 模板 CSS 报告 15 项既有格式问题；迁移后的前端 CSS 单独检查通过。这些问题不纳入本期 RPC 契约改动。

Vite / Vitest 自动重写的 router.ts 和 auto-imports.d.ts 已恢复，不把无关生成差异纳入交付。提交时禁用会触发既有 ESLint 配置错误及全工作区格式化的 hook，手动执行上述检查和 commitlint。

SSE / Tauri Event、预览文件访问、浏览器上传下载、独立 HTTP 进程、远程认证与 Session 持久化未实施。浏览器当前使用宿主文件路径，适用于本机开发，不是远程文件上传协议。退出会等待 core 合作取消及 HTTP 在途请求，没有强杀超时；进程被强制终止不在有序关闭保证内。断开请求不撤销 core 操作，返回序列化失败也不代表业务未提交；客户端不自动重试。JSON DTO 未做全面前端运行时 schema 验证，只检查错误基本形状、JSON 可解析性与安全整数。
