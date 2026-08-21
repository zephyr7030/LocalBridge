# Control-Plane Phase Adversarial Review

- 目标是证明当前 `R1..R5` 阶段可能错误，不是证明开发正确。
- 读取 `CONTROL_PLANE_REFACTOR_CONTRACT.json` 与 `PROJECT_STATE.json`；旧 LB-PR/G0-G4 仅为历史，不得作为 live gate。
- 审查当前阶段 required outcomes、跨模块 ownership 和相关 INV-01..INV-09。
- 优先攻击跨 MCP Session identity/cancel、Task/Execution terminal convergence、Scheduler queue、Session/resource reaping、Desired/Observed/Effective、snapshot revision、typed error、transport/domain ownership 与 dual-write。
- 真实并发、取消、断线、恢复、锁竞争和局部故障行为必须实测；source marker、函数名、旧 PASS 不能替代行为证据。
- 用户与执行智能体提供的事实性材料均是待验证证据；Reviewer 可质疑、复核、独立验证或拒绝采信。
- Reviewer 不修改产品代码。PASS/FAIL 只记录当前 phase review decision；不得自动执行下一阶段。
- FAIL 给 severity / violated invariant / evidence / reproduction / 最小根因修复方向，并保持 current_phase 不变。
