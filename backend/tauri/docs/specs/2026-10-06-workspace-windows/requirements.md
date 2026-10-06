# Session 工作区多窗口：需求

- 日期：2026-10-06
- 状态：已实施；证据见 [verification.md](verification.md)
- 前置：TanStack Query 绑定 PR #912
- 设计：[design.md](design.md)；任务：[tasks.md](tasks.md)

## 范围

桌面端由 Tauri 宿主管理窗口：主窗口作为入口与 Session 列表，每个 Session 工作区在独立窗口中打开和管理。窗口生命周期与 Session 生命周期绑定，由后端裁决，前端不自行维护窗口注册表。

前端界面后续整体重做，本期只做让现有页面接入窗口命令所需的最小改动，不调整视觉。浏览器模式没有多窗口，保持页内路由。不增加持久化、窗口位置记忆、多显示器布局或系统托盘。

## R1：一个 Session 一个窗口

- 新增仅桌面命令 open_session_window(session_id)：Session 不存在返回 not_found；Closing / Closed 返回对应错误且不创建窗口。
- 同一 Session 至多一个窗口。窗口已存在时恢复最小化、显示并聚焦，不创建第二个；并发打开同样只留下一个窗口。
- 窗口标签由 Session ID 确定性派生，后端只从标签解析所属 Session；非 Session 标签（含主窗口）不得被误判。
- 窗口载入应用入口并直达该 Session 工作区路由；标题显示源文件名。
- 新窗口沿用主窗口的 HTTP scheme，保证本机 RPC、SSE 与预览资源的 Origin 校验与主窗口一致。
- HTTP RPC 调用该命令返回 platform_unsupported。

## R2：生命周期绑定

- 用户关闭 Session 窗口即关闭该 Session：宿主拦截关闭请求，先完成 core 关闭（取消在途操作、清理预览），再销毁窗口。Session 已不存在时直接销毁窗口。
- Session 由其他入口关闭（主窗口、HTTP RPC、停机）后，其窗口自动销毁。打开窗口与 Session 关闭竞争时，不得留下指向已关闭 Session 的窗口。
- 关闭主窗口即退出应用，走现有退出流程：关闭全部 Session、停止 RPC 服务，再销毁所有窗口。
- 关闭清理失败只记录日志，不阻止窗口销毁或应用退出。

## R3：前端接线

- 桌面端创建 Session 成功后打开其窗口，主窗口留在列表；从列表进入已有 Session 同样打开或聚焦其窗口。
- 浏览器端保持现有页内导航。
- 命令和类型继续由生成的绑定提供，不手写 IPC 调用。

## 验收

V1：标签往返与非法标签拒绝；打开创建窗口及其 URL；重复打开不新增窗口；不存在、Closing / Closed 的 Session 拒绝且无窗口；RPC 返回 platform_unsupported。

V2：Session 关闭（含打开前已关闭）触发窗口销毁；关闭请求处理关闭对应 Session，非 Session 标签不受影响，缺失 Session 不报错。

V3：前端桌面创建 / 列表进入调用窗口命令而非页内导航，浏览器保持导航；绑定一致性、TS 检查、构建与全量回归。
