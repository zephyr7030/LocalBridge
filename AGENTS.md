# LocalBridge Agent Rules

## 1. 启动与事实源

每次任务先完整读取本文件，然后按需读取：

1. `START_HERE.md`
2. `PR_INDEX.json` + `PROJECT_STATE.json`（唯一实时 PR / Group / Review 状态源）
3. `PR_CONTRACTS.json` 当前 PR 合同
4. `START_HERE.md` 列出的 8 份 numbered human authority 中与任务相关的文档
5. 与任务相关的 `skills/**`
6. `ARCHITECTURE_RULES.json`、`COMPATIBILITY_BASELINE.json`、`FINAL_REVIEW.json`
7. `runtime-manifest.toml`、`runtime-policy.toml`
8. 当前磁盘代码、测试与 Git 历史

`START_HERE.md` 中 frozen/historical 状态不得覆盖实时状态；supplemental/历史文档不得覆盖当前 human authority、机器合同或当前代码。事实源冲突时停止并报告。

## 2. 执行纪律

- 单智能体串行；只执行 `current_pr`；LB-000→LB-019 严格顺序。
- 只写当前 PR `writable_paths` 和合同明确例外；forbidden/unlisted path 不写。
- PR PASS 只推进治理并停止，不自动开始下一 PR。
- 组末必须独立 adversarial review PASS 才解锁下一组；G3→G4 还必须额外通过人工实测 Gate。
- 聊天、旧报告、旧 PASS、用户或执行智能体陈述只作为线索；结论以当前磁盘代码、合同和真实测试为准。

## 3. First Principles + Ponytail（默认 full，稳定内联）

先理解问题和真实调用链，再选最小正确方案：

1. 不需要存在的需求不实现（YAGNI）。
2. 优先复用项目已有 helper/type/pattern。
3. 再用标准库 / 原生平台能力。
4. 再用已安装依赖；不要为几行代码新增依赖。
5. 最后才写最小完整代码；优先删除而不是增加、简单而不是聪明、少文件而不是扩散修改。
6. Bug 修根因，不修单一路径症状；先检查共享调用点与相邻调用者。
7. 非平凡逻辑至少留下一个能真正失败的最小 runnable check。

Ponytail 永远不能简化掉：用户显式需求、信任边界输入校验、防数据丢失错误处理、安全措施、可访问性、当前 PR 边界、必要测试或长期可维护性。不得用“一次性 patch”绕过稳定架构边界。

默认 `full`；用户可要求 `lite` / `ultra`，或用 `stop ponytail` / `normal mode` 停用。完整参考文本在 `skills/ponytail/SKILL.md`，但执行不依赖再次加载该文件。

## 4. 核心安全与架构不变量

- Public Agent API 由 LocalBridge 自有并版本化；禁止 upstream tool/schema/error/session/output handle 直接穿透。
- 所有 public call：LocalBridge registry → stable capability/action → PEP → adapter/executor；unknown fail-closed；高层 workflow 检查全部 transitive capability。
- 同时最多一个 active workspace root；remembered project 不等于授权；MCP 永远不能修改 WorkspaceRegistry / active workspace control-plane。
- workspace-bound public path 必须 active-root-relative；absolute / `..` / reparse escape fail-closed；`\\?\` 只允许内部 filesystem identity 校验，不能进入 UI/MCP/Broker/process cwd/workdir。
- Edit 无 process exec；Full 为当前用户权限；管理员能力只走独立 Broker；whole-app elevation 禁止；AI/MCP 不得自批准 UAC/管理员警告或扩大权限。
- Runtime API Key 只进安全凭据存储；禁止明文 settings/log/diagnostics/browser storage/CLI。
- listener 只允许 loopback；进程所有权优先 Windows Job Object，禁止 PID-only 最终所有权。

## 5. Runtime / UI 边界

- Rust/backend 持有 lifecycle、readiness、retry、权限和 CurrentTask truth；React/WebView 只做 typed projection + typed user intent。
- 可能阻塞的 process/file/credential/UAC/recovery/lifecycle 工作不得占用 UI/WebView 线程。
- 前端不得自己维护 runtime 启停/readiness/retry 状态机。
- UI、onboarding、窗口、管理员 9 秒安全确认、状态颜色、按钮对齐、中文术语等精确冻结语义只以 `START_HERE.md` + 当前机器合同为准；不要在本文件复制第二份容易漂移的规格。

## 6. 测试与审查

- 开发内循环先 targeted deterministic tests；真实 bundled runtime/process 测试属于 PR Runtime Gate；完整 cargo/npm/build/clippy/architecture 属 Group/Release Gate。
- 静态 marker 不能替代真实行为；安全边界必须有 negative/adversarial 验证。
- 所有长任务必须 bounded timeout/cancel；lost session 必须终态失败，不能永久 Running。
- 不为通过单个测试破坏更高层合同；若测试与当前机器合同冲突，先确认合同而不是迎合测试。

## 7. 治理

- 产品实现与治理状态变更分 commit。
- 只 stage 自己拥有的文件；不清理未知 untracked artifacts。
- 历史 review FAIL / provenance 不改写成 PASS；修复后新增 acceptance 记录。
- 任何预授权必须记录 `authorization_id/scope/actions/evidence_ref/recorded_by/user_audit_status`，且不得扩大未来 PR writable paths。
