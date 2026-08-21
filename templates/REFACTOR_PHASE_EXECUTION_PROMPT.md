# LocalBridge Control-Plane Refactor Phase Execution

你是当前唯一开发执行智能体。

项目：`<PROJECT_ROOT>`
当前阶段：`<R1|R2|R3|R4|R5>`

先按 `AGENTS.md` 的顺序读取实时事实源：

1. `CONTROL_PLANE_REFACTOR_CONTRACT.json`
2. `PROJECT_STATE.json`
3. `docs/06_CONTROL_PLANE_REFACTOR_EXECUTION.md`
4. 当前阶段相关磁盘代码与真实测试

确认 `PROJECT_STATE.json.current_phase == <PHASE>`。旧 `PR_INDEX.json`、`PR_CONTRACTS.json`、LB-PR/G0-G4 记录均为历史，不得参与当前决策。

执行原则：

- 单智能体、串行，只执行当前阶段；
- 当前阶段完成后不自动进入下一阶段；
- 以 invariant 和真实行为为验收目标，不以函数名、source marker 或历史 PASS 代替；
- 每迁移一项事实严格使用 `new owner → reader → writer → disable old writer → delete old truth`；
- 禁止长期 dual-write；
- ownership 迁移完成前不做大规模目录移动；
- 不扩展当前 feature freeze 中的功能；
- 产品修改与治理状态修改分开；
- 不伪造 review/PASS/provenance/human evidence。

实施后至少验证当前阶段相关的 INV-01..INV-09，并运行合同列出的对应 adversarial tests。所有非终态命令/session/operation 必须跟踪到 durable terminal。

阶段达到 acceptance 后：只更新 `PROJECT_STATE.json` 的阶段状态与下一阶段 READY 指针所需字段，然后停止，不自动执行下一阶段。

完成报告只包含：修改文件、满足的 invariant、执行的真实测试、剩余风险/阻塞、当前 PROJECT_STATE。
