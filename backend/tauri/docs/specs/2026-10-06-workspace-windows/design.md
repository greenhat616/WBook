# Session 工作区多窗口：设计

- 状态：已实施；证据见 [verification.md](verification.md)
- 需求：[requirements.md](requirements.md)

## 窗口身份

主窗口标签沿用 Tauri 默认 main。Session 窗口标签为 `session-{id}`，id 为十进制且无前导零；解析要求格式化后与原标签完全相同，因此 `session-01`、`session-`、`main` 都不是 Session 窗口。Tauri 自身的窗口表就是唯一注册表：是否已有窗口用 get_webview_window 判定，不另建映射，避免两份状态失步。

## 打开

open_session_window 是仅桌面的异步命令（Windows 上同步命令创建窗口可能死锁）。窗口命令需要 AppHandle / WebviewWindow 等带运行时参数的类型，因此统一命令宏允许仅桌面命令带唯一一个以 Runtime 为约束的类型参数；共享命令经 HTTP RPC 分发、不存在 Tauri 运行时，仍禁止泛型。宏以 `name::<tauri::Wry>` 注册：tauri_specta 为 invoke handler 去掉 turbofish，由 Tauri 推断实际运行时（测试中为 MockRuntime），只有 Specta 类型导出看到 Wry，而运行时注入的参数不进入导出类型。宏生成的内部项无法引用 builder 自身的 `R`，所以不能写成 `name::<R>`。流程：

1. 从 SessionManager 取句柄，快照 lifecycle 非 Open 时按 Rejected::Closing / Closed 返回错误。
2. 已有窗口则 unminimize、show、set_focus。
3. 否则以 `WebviewUrl::App("index.html#/sessions/{id}")` 创建窗口。Tauri 用 Url::join 拼接，片段保留，开发服务器和打包资源下都直达 hash 路由。显式设 use_https_scheme(false)，与 tauri.conf.json 中主窗口一致；否则新窗口 Origin 变为 https://tauri.localhost，SSE 与预览 iframe 会遇到混合内容限制。
4. 两个并发调用都可能走到创建；后者收到 WindowLabelAlreadyExists / WebviewLabelAlreadyExists 时转为聚焦已有窗口，不报错。
5. 仅创建成功的一方启动看守任务。

窗口创建或聚焦失败经 snafu 的 BridgeError::Window 保留 tauri::Error 来源，映射为 internal_error。

## 看守与关闭

看守任务持有 Session 的 watch::Receiver，用 wait_for 等待 Closed（发送端消失同样视为结束），随后销毁窗口。wait_for 先检查当前值，因此第 1 步之后、看守启动之前发生的关闭也会销毁窗口。看守不持有窗口句柄，按标签查找，窗口已不存在时无操作。

Builder::on_window_event 处理关闭请求：

- Session 窗口：prevent_close，异步执行 SessionManager::close 并记录清理失败，然后 destroy。NotFound / ShuttingDown 视为已关闭直接销毁。close 幂等，重复点击关闭只得到同一结果。destroy 不再触发 CloseRequested，不会递归。
- 主窗口：prevent_close 后调用 AppHandle::exit(0)。Tauri 的 exit 先发出可阻止的 ExitRequested，复用现有处理：关闭全部 Session、停止 RPC，结束后再次 exit 并销毁全部窗口。停机期间 Session 关闭会触发各看守销毁其窗口，与最终退出的销毁重叠无害。

主窗口始终拦截关闭，因此"最后一个窗口关闭"的隐式退出不会在 Session 窗口仍在时发生。

## 权限

capability 的 windows 增加 `session-*`，使 Session 窗口获得与主窗口相同的 core 权限。应用命令未声明 AppManifest 权限，本身对所有窗口可用。

## 前端

主页的打开入口在 Tauri 下调用生成的 commands.openSessionWindow，浏览器下页内导航；创建成功后与列表条目都经由它，失败显示在主页错误提示中。界面重做前不抽象到 features 层。Session 窗口内现有"关闭后返回首页"的导航保留：随后看守销毁窗口，短暂导航无副作用。主窗口列表依赖 TanStack Query 默认的窗口聚焦重新获取刷新，不新增推送事件；前端重做时再设计实时列表。

## 验证边界

MockRuntime 的 close / destroy 只在其事件循环中生效（每轮休眠 1 秒），因此生命周期逻辑拆为可直接测试的函数：标签解析、等待关闭、关闭请求对 Session 的处理；窗口创建、URL 与幂等用 MockRuntime 断言，MockRuntime 不记录标题。实际窗口销毁与退出顺序依赖真实事件循环，由不入库的 Wry 临时检查覆盖，见 verification.md。
