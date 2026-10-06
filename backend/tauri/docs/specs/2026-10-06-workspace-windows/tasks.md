# Session 工作区多窗口：任务

- 状态：已实施；验证见 [verification.md](verification.md)

1. [x] windows 模块：标签映射、打开 / 聚焦、看守与关闭请求处理，及单元测试 → V1、V2。
2. [x] open_session_window 命令、窗口事件接线、capability；重新生成绑定 → V1。
3. [x] 主页创建与列表接入窗口命令，及测试 → V3。
4. [x] 回归、类型检查、构建、桌面手工检查和 verification 记录；提交与 PR。
5. [x] 命令宏支持仅桌面命令对运行时泛型，open_session_window 改为直接接收 AppHandle → V1。
6. [x] 窗口隐藏创建、window_ready、超时兜底与前端就绪钩子；macOS 调查记录 → V4。
