# 前端工作台脚手架：需求

- 日期：2026-10-05
- 状态：已实施并验证
- 前置：接线 PR #907 提供 `WorkspaceStatus.preview_id`，栈顶承接 Snafu PR #908
- 设计：[design.md](design.md)；任务：[tasks.md](tasks.md)；证据：[verification.md](verification.md)

## R1：技术与视觉

使用仓库内 shadcn/ui 组件、Framer Motion 和 TanStack Router，替换原模板页与 Generouted / React Router。MD3 Expressive 风格采用暖纸色背景、墨绿主色、浅绿容器、醒目标题、胶囊按钮和宽松圆角；保持内容层次、键盘焦点和可读对比。动效响应 prefers-reduced-motion，不依赖远程字体或图片。

## R2：可用的基础流程

- 工作台首页列出真实 Session，支持刷新、空状态、连接失败和重试。
- 用显式标签的本机文本路径和分段数创建 Session；不伪装为浏览器文件上传。不提供演示假数据。
- Session 路由提供初始化、实时活动 / 阶段、文档状态、目录和元数据摘要、生成预览、输入目的路径导出 EPUB、取消及关闭。
- 预览加载当前 PreviewInfo 的正文，可切换生成文件，iframe sandbox 禁止脚本；不用 file:// 或任意文件读取。
- 外层命令错误与操作 outcome 错误都必须可见；cleanup warnings 不因失败而丢失。忙碌时禁用冲突操作，取消与关闭仍可用。
- 关闭成功且没有 cleanup warnings 时回首页；有清理告警时停留在已关闭页面，展示告警和手动返回入口，避免跳转丢失诊断。关闭凭据先于 SSE 终态到达时也立即禁用会话操作。取消不报为完成；导出成功显示真实目的路径。当前会话不持久化，空列表不声称保存了历史。

## R3：状态与路由

- 只消费生成的 commands、DTO 和上一 PR 的 bridge 助手，组件不手写 invoke / fetch。
- TanStack Router 使用显式路由树和 hash history，使打包桌面与静态预览的刷新可用。路由为 `/` 和 `/sessions/$sessionId`；无效参数或缺失 Session 有可恢复界面。
- 路由切换取消订阅、忽略旧请求回调；React StrictMode 不重复创建或初始化 Session。SSE 更新快照，操作凭据决定操作结果。单个 Session 的操作按顺序执行。
- 外部 revision / preview 失效时清除过期本地展示。预览同时匹配 `WorkspaceStatus.revision` 与 `preview_id`，即使合并的 SSE 快照具有相同 revision 和导出选项，不同生成 ID 也必须使旧预览失效；迟到的生成凭据不能恢复它。订阅失败显示重新连接入口，不自动重试写操作。

## 范围边界

本期构建基础工作台，不实现文本编辑、目录拖拽、完整解析规则编辑器、元数据编辑、原生文件对话框、远程上传、主题切换、历史数据库或多语言切换。可复用既有依赖，不增加第二套服务缓存或状态管理框架。全量 ESLint 的旧配置需迁移，使新脚手架有可运行的检查入口；不顺带格式化 core EPUB 模板。

## 验收

V1：路由刷新 / 非法参数 / 未知页面、桌面尺寸与窄屏布局、键盘焦点和减少动效。

V2：使用真实本机服务完成创建 → 初始化 → 预览（含 CSS）→ EPUB 导出 → 关闭；校验输出文件与关闭后列表。测试输入输出均在独立临时目录。

V3：受控测试覆盖失败 outcome 与 warnings、忙碌、取消、订阅释放、旧响应隔离和过期预览清除。

V4：类型、绑定一致性、前端单测、构建及修改文件 lint；记录实际执行与不能覆盖的部分。
