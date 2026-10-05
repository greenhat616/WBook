# Tauri / Axum 统一命令：实施与验证

- 日期：2026-10-05
- 状态：已实施；后端、客户端与前端构建验证通过（见 [verification.md](verification.md)）
- 需求：[requirements.md](requirements.md)
- 设计：[design.md](design.md)

## T1：契约与共享命令

- [x] 增加 Tauri DTO、错误转换与安全整数检查。
- [x] 单一声明生成共享实现、IPC wrapper、HTTP dispatch 与 Specta 注册。
- [x] 接入全部 14 个 Session 命令并保留 get_port。

验证：V1、V2；命令错误、长操作元数据、完整读写链路。

## T2：HTTP 与应用生命周期

- [x] Axum RPC 路由与统一 JSON 拒绝。
- [x] 绑定实际监听器、Host / Origin 限制、共享 Wbook 装配。
- [x] Tauri 退出等待 core 与 HTTP 有序关闭。

验证：V1、V2、V3；请求进入限制、共享状态、监听和关闭。

## T3：Specta 与客户端

- [x] 固定依赖、导出脚本、必要替换点强校验和 --check。
- [x] 已生成客户端、IPC / fetch 适配及桌面专属命令。
- [x] 传输测试与绑定相关文件的 TS 检查。
- [x] 全量 TS 检查与前端构建通过；依赖前置配置修复提交。

验证：V4；生成物一致、客户端两端运行、错误回退。

## T4：回归与交付证据

- [x] Rust 跨传输契约、完整命令链、边界测试。
- [x] core 回归、workspace check / clippy、修改文件格式检查。
- [x] verification.md 记录实际命令、结果与限制；更新 spec 状态。

不得将未执行的 GUI / 浏览器手工测试表述为已通过；不为通过检查顺便修改无关模板问题。
