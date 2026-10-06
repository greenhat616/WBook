# 前端工作台脚手架：设计

- 状态：已实施并验证，待发布独立 stacked PR
- 验证：[verification.md](verification.md)

## 分层

`src/router.tsx` 使用 TanStack Router 的代码路由和 hash history；首页与 Session 页位于 `src/pages/`。`src/components/app-shell.tsx` 提供导航、跳到内容和布局，页面保留各自数据逻辑。路由未知项提供返回首页入口。

`src/features/sessions/` 封装生成客户端的错误解包和两个 hooks。useSessions 提供列表、刷新与创建；useSession 提供快照、结果、预览、状态订阅和业务动作。避免引入新全局 store：会话真相在 core，列表每次进入重新查询，当前页只订阅当前 Session。

长操作先检查生成 Result，再记录 OperationResponse.warnings，然后检查 outcome。写操作均由用户触发；路由 effect 只查询和订阅。操作完成后串行读取结果 / 最新快照，不与同一 Session 的写请求竞争。SSE 只更新快照和作废不再有效的缓存，不因每条事件读取结果而形成循环。路由或请求代次变更后忽略旧响应。

快照按 Session 与递增 seq 接受，旧查询结果不能覆盖更新的订阅状态。预览缓存必须同时匹配当前 revision 与 preview_id；生成选项相同不代表生成物仍然相同。生成操作收到凭据后再次核对当前 ID，避免刷新或 SSE 已观察到替换预览时重新安装旧 URL。结果读取只有在确认快照仍为 Open 且 revision 一致时才更新本地结果，关闭状态不会被迟到的读取重新填充。

## 页面与组件

首页包含导入表单、简短的流程提示、真实会话列表。Session 页顶部显示文件名和返回链接，主体为工作区摘要 / 目录、活动状态、预览区与导出区域。初始化与生成预览分别为显式按钮；默认按指定数量均分文本，导出为 EPUB、SingleHtml、zh-CN，不引入完整解析器表单。

关闭动作将生成的 ClosedSession 报告返回页面，失败或页面已卸载时返回 null。页面只在成功报告没有 cleanup_failures 时自动回首页；存在告警则保留已关闭页面，显示清理失败路径与原因，并保留手动返回链接。可操作状态同时考虑快照 lifecycle 和本地 connection，避免关闭响应早于 SSE 终态时继续显示可用的预览 / 导出操作。

复用并调整 shadcn Button，补 Input / Card / Badge 等本期实际使用的基础组件。表单使用原生 label 和 select，减少额外依赖。所有主要交互有可见文字，错误 role=alert，状态 aria-live。Framer Motion 用在页面入场和容器过渡，MotionConfig reducedMotion=user 配合 useReducedMotion 禁用不必要位移；CSS 媒体查询关闭过渡 / 无限旋转。

跳到主要内容链接通过聚焦 main 元素实现；阻止默认锚点跳转，避免覆盖 hash history 的路由位置。宽屏使用左侧导航，窄屏转为顶部导航，不提供未实现的设置或假数据入口。预览 iframe 禁止脚本，通过已有 bridge 助手访问当前生成文件与 CSS。

MD3 Expressive 是本项目视觉适配，不宣称采用完整 Material 组件实现：背景暖白、墨绿 CTA、浅绿 surface、强调色少量用于阶段和提示。大圆角与形状变化围绕信息分组，长路径换行、窄屏单列，800×600 桌面可用，滚动承载长内容。

## 开发与验证

保留已有 Vite 1420 端口，浏览器接现有 Tauri 宿主，通过 WBOOK_RPC_PORT / VITE_WBOOK_RPC_URL 设置同一本机服务。前端显示真实连接错误，不把未启动的后端替换为成功假响应。浏览器验收可使用临时本机测试宿主启动同一 AppRuntime，不交付独立服务器功能。

Vite 文件监听排除 backend，避免 Windows 上 Cargo 构建产物被锁定时触发 EBUSY 并终止开发服务。此配置保留前端热更新。

迁移 ESLint flat config 并明确生成文件 / 构建目录忽略项。TypeScript 的 npm 别名将 `typescript` 指向 `@typescript/typescript6@6.0.2`，为 typescript-eslint 提供编译器 API；`@typescript/native` 指向 `typescript@7.0.2`，保留原生 `tsc` 7.0.2。React lint 插件使用官方 `@eslint/compat` 适配 ESLint 10 移除的规则 API，保留原有推荐规则。删除由本次路由替换造成的模板入口和生成器配置，保留无关依赖与 core 代码。

## 参考

- https://tanstack.com/router/latest/docs/framework/react/guide/code-based-routing
- https://tanstack.com/router/latest/docs/framework/react/guide/history-types
- https://ui.shadcn.com/docs/tailwind-v4
- https://motion.dev/docs/react-use-reduced-motion
- https://m3.material.io/styles/motion
- https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/#running-side-by-side-with-typescript-6.0
- https://eslint.org/blog/2024/05/eslint-compatibility-utilities/
