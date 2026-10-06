# Tauri / Axum 统一命令：设计

- 日期：2026-10-05
- 状态：已实施；证据与限制见 [verification.md](verification.md)
- 需求：[requirements.md](requirements.md)
- 任务：[tasks.md](tasks.md)

## 分层与声明

`wbook-command-macros` 是独立 proc-macro crate。`#[unified_commands]` 标注内联模块，普通 Rust 命令函数使用 `&Wbook` 服务参数、拥有所有权的命名输入和 `Result<T, CommandError>` 返回值。宏保留原函数、属性和所在作用域，函数互调与递归不改变含义；另外生成同名子模块的 `name::call`，在委托原函数前后检查传输整数范围，以及 camelCase Args 和 HTTP 分发。带 Tauri / Specta 标记的 wrapper 放在私有模块中，由同一个 collect 注册。`#[desktop_only]` 标记 get_port，保持原返回值并排除 HTTP 调用。无需再维护声明宏 DSL 或第二份命令列表。

该属性宏保留参考设计的四项产物，但由一个内联模块提供静态注册范围，不需要 inventory 或 TypeId 容器。当前服务固定为 Wbook；明确验证函数签名，拒绝 self、泛型、不支持的借用输入、unsafe / extern / variadic 和错误返回类型，错误定位到原声明。宏处理只做编译期接线，不处理业务状态。

依赖为 Tauri → server、core；server 仅负责监听与 HTTP 入口限制，接收已装配的 Axum Router。RPC 协议、领域 DTO 转换及生成器放在 Tauri 层，core 不感知任何传输。

## DTO 与错误

直接复用已有 Serialize / Deserialize / Type DTO。运行时的 Receipt、OperationResult、CloseReport 与 ExportArtifact 由 Tauri DTO 映射；不为 runtime 通道或错误对象强加序列化。操作响应 `OperationResponse<T>` 保留 op、kind、revision、outcome（status: ok / error，data / error）及清理告警，失败时合并 ExportError 自带的 cleanup_failures。导出 DTO 保留产物路径、版本、标识与清理告警，预览仍返回已有 PreviewInfo，不提供路径资源访问 API。

CommandError 使用稳定 kind：invalid_params、method_not_found、platform_unsupported、not_found、shutting_down、busy、unavailable、closing、closed、invalid_config、stale_revision、no_document、already_initialized、results_not_current、read_too_large、cancelled、panicked、extractor、document、pipeline、export、internal_error。HTTP 400 用于参数 / 平台 / 配置等输入问题，404 用于命令 / Session 不存在，409 用于状态冲突，503 用于 shutdown / unavailable，500 用于内部故障。已接收操作的失败位于 outcome，HTTP 仍为 200。

共享调用在执行前验证输入整数、执行后验证序列化输出的整数范围。该协议承认 JSON number 边界；输出编码失败可能发生在 core 已提交之后，不能通过重试推定未执行。

## 传输与类型

Axum 的 `/bridge/rpc` 解析请求，按宏生成的 match 分发；所有入口错误转为 CommandError。get_port 不进入业务分发，单独返回 platform_unsupported。Tauri 的原生命令参数解析错误保留框架行为，由客户端转为传输 Error；已经进入共享实现的领域错误在两端一致。

使用 tauri-specta Builder 的 invoke_handler 和 TypeScript exporter。生成到临时文件，校验唯一 invoke import 并替换为 `./transport`，再写入 `src/bindings.ts`；不在启动时修改源码。导出器保留 Serde 输入 / 输出差异，显式选择 Result 错误模式和安全整数协议对应的 number 类型。

`src/transport.ts` 在调用时检测 Tauri；浏览器使用 `VITE_WBOOK_RPC_URL`，默认 `/bridge/rpc`。固定开发示例使用 WBOOK_RPC_PORT=1421 与 VITE_WBOOK_RPC_URL=http://127.0.0.1:1421/bridge/rpc。生成器同步导出桌面专属命令常量；适配器仅在调用时读取该常量，ES module 相互引用不在模块初始化阶段访问未初始化绑定。

## 生命周期

setup 内通过 Tauri runtime 构造 Wbook，路径来自 Tauri app_data_dir / app_config_dir。绑定 TCP 后取得实际端口，传给 Port 状态和 server；不再通过 portpicker 先探测后抢占端口。

HTTP shutdown 信号与完成任务句柄随应用管理。ExitRequested 首次阻止退出并异步开始有序关闭，后续请求继续阻止，完成后显式 app.exit；结束标志允许最终事件通过。先发送 HTTP 停止信号，然后 await Wbook.shutdown，再等待 server 任务，以免长命令阻止取消启动。

## 验证策略

真实 Tauri mock runtime 通过 invoke_handler 解码和执行命令；Axum tower oneshot 执行 HTTP。两者交替访问同一个 Wbook，并比较快照、返回 JSON 和失败 JSON。临时输入 / 输出显式关闭，不能污染仓库。6 个过程宏测试检查注册、同步 / 异步展开、原函数属性及互调 / 递归作用域、内部标识符冲突和不支持的声明诊断；跨传输测试保护参数映射、整数范围、错误与生命周期。TS 测试以 mock invoke / fetch 保护传输选择和异常。生成物检查由同一个生成器的 --check 完成。
