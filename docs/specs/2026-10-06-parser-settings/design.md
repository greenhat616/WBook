# 解析与渲染设置：设计

- 状态：已实施并验证
- 需求：[requirements.md](requirements.md)；验证：[verification.md](verification.md)

## 数据模型

`wbook-core::settings::Settings` 是全局设置与 Session 设置共用的类型：

```text
Settings
├─ toc: TocSettings        结构模式 + 标记 + 数值参数（原前端 ParserForm）
├─ filters: FilterSettings { ad: bool }
└─ render: RenderSettings  layout / language / templates: TemplateOverrides
```

`TocSettings::to_config()` 生成 `TocParserConfig`，取代前端 `buildParserConfig`。映射挪到后端后，`EXTRA_PATTERN` 等预设只有一份，设置可以在没有前端参与时直接驱动解析。`TocParserConfig` 保持为通用规则层，不随表单收窄。

读取 `settings.toml` 时先把文件表合并到默认值序列化出的表上，再反序列化：旧文件缺少新字段时取默认值，未来加字段不需要迁移。不在类型上使用 `#[serde(default)]`，否则 specta 会把每个类型拆成 `_Serialize` / `_Deserialize` 两份绑定。`Settings::validate()` 编译解析规则与模板，全局保存和 Session 修改共用。

## 全局设置

`Wbook` 持有 `SettingsStore`：路径为 `config_dir/settings.toml`，内存中是 `Mutex<Settings>` 与加载问题描述。写入先写同目录临时文件再 rename，避免半写文件。加载失败保留默认值与错误信息，`get_settings` 一并返回，设置页提示后由用户保存覆盖；不在启动时自动改写用户文件。

使用 `toml` crate 读写；现有未使用的 `config` 依赖只读、面向分层配置，不适合回写，保持不动。

## Session 设置

`Workspace` 以 `Settings` 取代 `ProcessingOptions`。`SessionManager::create(source)` 由调用方传入全局快照，Tauri 命令 `create_session(source)` 从 `SettingsStore` 取当前值。

新增写操作 `set_session_settings(expected, settings)`：校验后提交，变化时 revision +1 并关闭预览（渲染输入变了，旧预览不再代表当前设置）；无变化不递增。读取走 `get_session_settings`，前端在快照 revision 变化时重新查询。Session 在 workspace 之外保留一份设置镜像，每次操作结束后更新，因此操作进行中也能读取，不返回 Busy。

`parse_session`、`render_preview`、`export_epub` 不再接收配置，统一从 workspace 设置派生：`TocSettings::to_config()`、`RenderSettings` → `ExportOptions`。前端“试解析”先提交设置（如有变化）再解析；草稿目录仍按 revision 失效。

过滤器只在初始化时按设置运行；`filters_applied` 语义不变，初始化后修改过滤设置只影响记录值。

## 模板覆写

`ExportOptions` 增加 `templates: TemplateOverrides`（各项 `Option<String>`）。渲染时以内置模板为底构造 `Tera`，再叠加覆写；无覆写时复用静态实例，避免每次渲染重复编译。`style.css` 覆写直接写入 `styles/book.css`。

只开放 document / section / paragraph 与样式表：渲染代码自行写出 `</section>`、`</body></html>`、包文件与导航，覆写这些片段不会破坏结构闭合。新增查询 `default_templates` 返回内置文本，供编辑器作为起点。

预览缓存比较 `ExportOptions` 整体，模板变化自然触发重新生成。模板覆写让 `WorkspaceStatus` 明显变大，`Availability::Available` 因此装箱，序列化结果不变。

## 前端

- `SettingsEditor`：受控编辑 `Settings`，分区为“目录解析”“文本清理”“渲染”“模板”。修改以 updater 函数提交，同一事件中的多处修改不会互相覆盖。`SettingsScreen` 包装保存栏（保存、撤销修改、恢复来源），全局页与 Session 设置页共用。
- 解析字段抽成 `TocFields`，工作台“解析规则”面板与编辑器共用；标记在界面里仍按空格分隔编辑，提交时转数组。
- `/settings`：加载、显示加载问题、保存、恢复默认。
- `/sessions/$sessionId/settings`：读取 Session 设置，保存走带 revision 的写操作，提供“恢复为全局设置”（读取全局后写入 Session）。
- 创建 Session 不再传配置；`parser-config.ts` 中的映射删除，只保留表单与 `TocSettings` 的互转和表单级校验。
- 全局页“恢复默认”读取新增的 `default_settings` 查询，只填充表单，不自动保存。
