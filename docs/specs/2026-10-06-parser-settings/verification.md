# 解析与渲染设置：验证

- 日期：2026-10-06
- 状态：已实施并验证
- 对应：[requirements.md](requirements.md)；[design.md](design.md)；[tasks.md](tasks.md)

## 自动检查

| 检查                            | 结果                                                  |
| ------------------------------- | ----------------------------------------------------- |
| `pnpm test:backend`             | 226 项通过                                            |
| `pnpm lint:backend`             | 无代码告警（仅既有的未使用依赖 / 字段 manifest 告警） |
| `pnpm bindings:check`           | 通过                                                  |
| `pnpm lint:ts`                  | 通过                                                  |
| `pnpm exec eslint --no-cache .` | 通过                                                  |
| `pnpm exec vitest run`          | 6 个测试文件、57 项通过                               |
| `pnpm build`                    | 通过                                                  |

新增覆盖：

- core：各模式默认 `TocSettings` 与对应预设的规则配置相等；空标记、零上限被拒绝；不分卷时无需分卷标记。
- core：设置 toml 往返；只含部分字段的文件按默认值补齐；语法错误、未知枚举、非法规则、非法语言四类文件回落到默认值并报告，文件内容不被改写，随后保存可覆盖；非法模板在写入前被拒绝。
- core：模板覆写替换片段与样式表，且仍转义正文；内置模板作为覆写时渲染结果不变；非法模板在校验与渲染阶段被拒绝。
- core：Session 设置变更递增 revision 并关闭预览，相同设置不递增，非法设置与过期 revision 不改变状态；设置驱动试解析（仅章节 / 卷 › 章）和预览选项；操作进行中仍可读取设置；关闭后读取被拒绝。
- tauri：IPC 与 HTTP 共用的完整流程覆盖 `get_settings`、`save_settings`、`default_settings`、`builtin_templates`、创建 Session 复制全局设置、`get/set_session_settings`、设置驱动的预览语言；非法全局设置返回 `invalid_config` 且不落盘。
- 前端：全局设置页保存解析 / 过滤 / 模板修改，模板从内置源开始编辑；设置文件问题提示与“恢复默认”只填充表单；非法表单禁止保存，后端拒绝信息可见。Session 解析面板先保存变化的规则再试解析；Session 设置页按当前 revision 保存并可从全局设置开始。

## 真实本机流程

临时测试宿主（未提交的 ignored 测试）在 127.0.0.1:1421 启动真实 AppRuntime，配置目录与输入均位于 `G:/Programs/Rust/.ccg/WBook/settings-qa-20261006`，不触碰用户配置。Vite 运行在 1420，页面通过 headless Chrome 的 CDP 驱动（agent-browser CLI 在本机安装损坏，无法使用）。

1. `/settings` 选择“仅章节”、开启广告过滤、自定义段落模板后保存；`config/settings.toml` 写入对应字段，段落模板以多行字符串保存。
2. 创建 Session 后整理文本：目录为 3 章（未分卷），状态栏显示清理 1/1，网址行被删除，说明 Session 复制了全局设置。
3. 解析规则面板切换为“卷章 / 不分卷”试解析：草稿为 3 章、无分卷；本书设置页随之显示“不分卷”。
4. 本书设置页把布局改为“每章一个文件”并保存；回到工作区生成预览，预览按章节拆分为多个文件。
5. 1100×900 与 390×800 下设置页布局正常，保存栏吸附在底部；流程中无 JavaScript 异常。

QA 中发现：编辑器以闭包中的整份表单提交修改，同一事件内连续修改会互相覆盖（脚本在一次执行中先后切换模式与过滤开关时，模式丢失）。已改为 updater 函数提交并复验。
