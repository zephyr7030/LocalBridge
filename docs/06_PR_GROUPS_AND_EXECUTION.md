
# 06 — PR Groups & Execution

20 个 PR 不变，严格按编号顺序执行，并增加组级对抗审查 Gate。

```text
组内 PR 全部 PASS
→ GROUP_REVIEW_REQUIRED
→ 独立对抗审查
→ PASS
→ 下一组第一个 PR 才 READY
```

前组审查未 PASS，后组任何 PR 不得开始。

G3→G4 是唯一双重 Gate：G3 独立对抗审查 PASS 后只进入 `human_review_status=REQUIRED`，不得解锁 G4；还必须完成一次人工实测细审核，并由独立审查智能体基于证据接受为 PASS。

## G0 — 上游与可行性

```text
LB-000 Upstream + Security Compatibility Spike
```

## G1 — 基础与数据

```text
LB-001 Repository Bootstrap + Packaging Smoke
LB-002 Domain Contracts
LB-003 Settings & App Data
LB-004 Windows Process Supervisor
LB-005 Credentials
```

## G2 — 运行时与安全执行

```text
LB-006 Portable Coding Tools Runtime
LB-007 MCP Policy Enforcement
LB-008 Tunnel Runtime
LB-009 Orchestrator Core
LB-010 Recovery + Workspace Switch
LB-011 Privileged Broker IPC Foundation
LB-012 Elevated Permission Mode
```

自动重连职责：

- LB-008：Tunnel reconnect primitive；
- LB-009：outage generation/状态；
- LB-010：5 次 budget、1/2/5/10/30s、最小层重启、最终单次错误事件。

## G3 — 桌面与极简 UI

```text
LB-013 Tray + Background
LB-014 Autostart + Single Instance
LB-015 UI Shell
LB-016 First-run Wizard
LB-017 Diagnostics
```

LB-015：

- Apple-inspired；
- 只用 React/Tauri + 原生 CSS/SVG/system fonts；
- 不新增 UI/动画/图标/CSS/字体依赖；
- 单行绿色脉冲当前执行；
- 重连前 5 次无新增 UI；
- 5 次失败后一次极简错误窗口。

## G4 — 打包与发布

```text
LB-018 Runtime Packaging
LB-019 Release / Clean-machine / Reboot E2E
```

### G3 → G4 人工实测细审核 Gate

G3 独立对抗审查 PASS 时必须：

```text
G3.status = PASS
G3.review_status = PASS
G3.human_review_status = REQUIRED
current_group = G3
current_pr = null
G4 = BLOCKED
LB-018 = BLOCKED
```

人工 Gate 的事实证据默认不可信：审查智能体可质疑、要求复测、独立验证或拒绝采信执行智能体和用户提供的日志、截图、口头结论与测试描述。用户/执行智能体陈述不是自动 PASS。

执行智能体允许预授权，但每一项实际使用的预授权必须具体记录：

```text
authorization_id
scope
actions
evidence_ref
recorded_by
user_audit_status
```

人工 Gate PASS 前，所有记录的执行智能体预授权都必须 `user_audit_status = PASS`。人工 PASS/FAIL 均必须绑定独立 `human_review_provenance` 和仅修改 `PR_INDEX.json` / `PROJECT_STATE.json` 的 decision commit。

人工 PASS 才允许：`G4 = READY`、`LB-018 = READY`、`current_group = G4`。人工 FAIL 必须回开具体 G3 PR，并把 G3 智能体审查重新置为 REQUIRED；修复后必须重新执行 G3 独立对抗审查，旧 PASS 不得沿用。

## 组内推进

当前 PR PASS：

- 若组内还有 PR：只解锁下一编号 PR；
- 若是组末：
  - group → REVIEW_REQUIRED；
  - current_pr = null；
  - 下一组继续 BLOCKED；
  - 开发智能体停止。

## 独立组审

审查整个组当前磁盘状态：

```text
contracts
architecture
all acceptance
cross-PR integration
negative/adversarial cases
security boundaries
scope
```

### PASS

Reviewer 不得改代码/合同/文档/测试。

只允许受限写：

```text
PR_INDEX.json
PROJECT_STATE.json
```

仅推进当前 group review/status，并解锁下一组首 PR。

### Review provenance

每次非 `BLOCKED` 的组审结论必须绑定独立 review commit：

```text
review_provenance.generation == review_generation
review_provenance.kind = independent_adversarial
review_provenance.commit = exact Git commit
```

合法 review commit 必须：

- 从该组 `REVIEW_REQUIRED` 状态出发；
- 只修改 `PR_INDEX.json` / `PROJECT_STATE.json`；
- 不得与生产代码、测试、合同、架构规则修改混在同一提交；
- 必须是当前 HEAD 的祖先提交。

若历史 review generation 被发现由开发提交污染，必须显式记录为 invalidated generation；该 generation 不得再作为独立审查证据。ARCH-024 对以上约束 fail-closed。

### FAIL

必须给：

```text
severity
evidence
reproduction
reopen_from_pr
```

状态：

```text
group = REWORK_REQUIRED
review_status = FAIL
reopen_from_pr = REWORK_REQUIRED
其后同组 PR = BLOCKED
下一组 = BLOCKED
```

从 reopen_from_pr 起重新顺序执行，再重新组审。

## 外部凭据

LB-000 real Tunnel 子 Gate 若唯一阻塞是缺真实 credential：

- deterministic/spike 仍必须完成；
- 明确记录 external blocker；
- 不伪造 PASS。
