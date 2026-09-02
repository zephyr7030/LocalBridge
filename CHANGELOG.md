# Changelog

All notable user-visible changes to LocalBridge are recorded here.

## [Unreleased]

No user-visible changes yet.

## [0.1.5] - 2026-09-03

- 本机 MCP 入口增加每实例认证；公开资源标识改用密码学随机数，执行接管必须满足原 owner 已离线及一次性转移凭证校验。
- 修复排队请求取消、服务停止后迟到执行，以及工作流和 detached command 的跨 Session 隔离。
- DOCX 局部编辑保留原 ZIP 文档包及未修改内容；不能保真的编辑明确拒绝。多文件 Patch 暂时明确拒绝，避免无崩溃恢复保证的部分提交。
- 修复权限降级时的 Broker 清理、启动权限投影、前端旧快照与结构化错误展示，以及 Git 特殊文件名解析。
- 收紧 Windows 子进程句柄继承和 WebView 内容安全策略；增加使用端黑盒、排队取消、文档保真和前后端合同回归。

## [0.1.4] - 2026-09-01

- 将 `document_workflow` 收敛为 `inspect / search / create / edit / convert / rebuild` 六个稳定 action，并统一经过 typed `DocumentIR`。
- 文档局部编辑仅保留 replace、insert-before、insert-after、delete 四种原子操作；编辑和整体重建均使用 SHA-256 乐观并发保护。
- 增加原生 DOCX 创建、读取、搜索和编辑，以及 PDF 读取、搜索和转 TXT/Markdown；不支持的有损修改会明确拒绝。
- 修复启动和恢复期间权限、工作区、活动与故障投影不一致的问题，使前端继续只消费同一 revision 的后端快照。

## [0.1.3] - 2026-08-23

- 完成本轮 schema44 控制面缺陷修复，并将公开工具 API 提升至 revision 47；统一架构验收仍以项目状态记录为准。
- 修复 durable workflow 与 detached command 跨 MCP Session 的恢复、观察和定向取消。
- 统一 Task/Execution 终态、权限 Desired/Observed/Effective 投影及 workspace Path Authority。
- 修复 filesystem 根目录访问、默认递归搜索、Git/document/output 错误分类和控制调用时间预算。
- 五屏引导改为读取 revisioned 后端权限真相；更新检查和 GitHub Releases 返回类型化结果并固定官方发布源。
- 重构本地/CI 测试基座并新增 ChatGPT 侧双 Session 黑盒回归。

## [0.1.2] - 2026-08-21

### Improved
- Multiple MCP sessions and app windows now share bounded, fair work scheduling while retaining session-scoped request and cancellation isolation.
- Detached commands and long-running tasks now keep stable identities and converge to explicit terminal outcomes across disconnects and runtime restarts.
- Permission and workspace changes now reconcile desired and observed state before exposing effective authority, preventing partially applied control-plane state.
- The app now renders one revisioned live-state snapshot, including scheduler pressure and actionable faults, without guessing task completion or runtime activity.

### Reliability
- Runtime restart recovery marks orphaned executions as lost and preserves unaffected sessions and tasks.
- Lock contention and unavailable observations are reported as stale or unavailable instead of fabricated running state.

## [0.1.1] - 2026-08-21

### Added
- Added a structured `filesystem` tool for common file operations such as listing, reading, writing, searching, copying, moving, deleting, and hashing files without relying on ad-hoc Shell commands.

### Improved
- Multiple MCP clients can now stay connected at the same time without a new session invalidating an existing one.
- Long-running and interrupted tasks now converge to a reliable terminal state instead of remaining permanently stuck as running or waiting.
- Packaged background runtime, Tunnel, recovery, autostart, and managed command paths run without unwanted visible console windows.
- Windows development command compatibility was improved, including ordinary workspace cleanup such as `rmdir /s /q`.

### Security
- Hardened Elevated filesystem authorization against hard-link, junction/reparse, final-object identity, race, and control-plane alias bypasses.
- Tightened Elevated process/Shell authorization so unreviewable administrator execution cannot bypass LocalBridge control-plane protections.
- Runtime API Key remains in Windows Credential Manager and is removed by the uninstaller.
- Release provenance is now bound to a fresh installer build instead of allowing an older installer to be relabeled as the current source revision.

> Release notes intentionally describe user-visible behavior rather than raw commit history.
