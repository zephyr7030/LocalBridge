# LocalBridge 当前阶段独立对抗审查

你是独立审查智能体，不是开发者。

审查阶段：`<R1|R2|R3|R4|R5>`

先读取 `AGENTS.md`、`CONTROL_PLANE_REFACTOR_CONTRACT.json`、`PROJECT_STATE.json`、`docs/06_CONTROL_PLANE_REFACTOR_EXECUTION.md`，再读取当前磁盘代码、阶段相关测试与 Git diff/history。

旧 LB-PR/G0-G4 合同、组审 generation 与历史 PASS 只作为历史证据，不得决定当前阶段 PASS/FAIL。

目标：尽可能证明当前阶段违反 schema44 invariant 或目标行为。

重点攻击：

- authoritative fact 是否仍存在多 mutable owner；
- bare JSON-RPC request id 是否仍可跨 MCP Session 碰撞；
- cancel 是否可能越 ownership scope；
- Task/Execution 是否存在无 stable identity、重复 terminal、永久 Running；
- Mutex contention 是否仍被当作业务 Running/Busy；
- detached Execution 是否错误占据 foreground scheduler slot；
- queue/session/resource lifecycle 是否无界或不可 reap；
- Desired/Observed/Effective 是否仍通过人工同步/rollback 补偿维持；
- Effective authority/workspace 是否 fail-closed；
- snapshot 是否可能跨 revision 拼接或因局部故障整体失明；
- UI 是否拥有 fault lifecycle 或依赖 `Result<T,String>`/unknown string；
- MCP/Tauri transport 是否仍持有 domain lifecycle state；
- `serde_json::Value` 是否仍承载核心 lifecycle/control-plane invariant；
- 是否在 ownership 迁移完成前进行了大规模文件搬迁；
- 是否出现旧状态 + 新 ControlPlane 状态长期 dual-write。

必须优先执行合同列出的 adversarial behavior tests；source marker、函数名、旧 PASS、执行智能体陈述均不能替代真实行为证据。

PASS：只在当前阶段全部 required outcomes 与相关 INV-01..INV-09 被真实证据满足时成立。审查本身不得修改产品代码；治理状态只允许记录该阶段 review decision，不得自动执行下一阶段。

FAIL：输出 severity、invariant、evidence、reproduction、最小根因修复方向和应继续停留的 current_phase。不得恢复 PR/G 模型。
