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

Schema25 Agent Runtime facade 修订从 **LB-006** 重新打开 G2。LB-006 负责 LocalBridge-owned public ToolRegistry/facade、内部可替换 WorkspaceRuntimeAdapter、mandatory capability negotiation、stable result/error normalization、可信 ShellResolver，以及 DirectProcessExecutor / ShellExecutor 分离；LB-007 负责 stable LocalBridge capability/action classifier、transitive workflow capability enforcement 与 public unknown deny。`elevated_exec` 的现有 Broker/permission 语义保持不变。

G2 adversarial generation 9 已实际消费并判定 **FAIL / REWORK_REQUIRED**。Schema27 将 generation 9 及其后 public plugin 复测暴露的 LB-006 行为缺口提升为当前合同：

- LocalBridge-owned public Session Manager；禁止 raw upstream session/output handle；
- `command_control poll/read/write/kill` 形成真实闭环，poll 不得错误映射 retained-output read；
- session 必须后台收敛到 terminal outcome，private pruning/丢失不得造成永久 Running；
- `workspace_context.workspace` 必须投影真实非空 ordinary active workspace，`default_cwd` 独立为 workspace-relative；
- upstream outputSchema 不足以约束 adapter 所消费字段时，启动前增加 deterministic result semantic probe；
- 非零进程退出统一为 `ProcessFailed`/Failed task，不能因“无输出”包装成 success；
- 已进入 public schema 的 `agent_workflow` / `task_control` / `document_workflow` action 不得恒定 unavailable；
- workspace-bound public path/workdir 输入统一 active-workspace-relative，absolute/traversal fail-closed；
- `git_workflow` 五 action 使用同一 nested-repository resolver，repo 已发现时 `git_diff` 不得 silent non-git fallback。

该基础合同变化使历史 G2 adversarial generation 8 与 G3 generation 6 仅保留为 provenance，不得继续解锁后续。严格顺序固定为：

```text
LB-006 REWORK_REQUIRED
→ LB-007 → ... → LB-012
→ G2 REVIEW_REQUIRED
→ fresh G2 adversarial generation 10
→ 重新执行/验收 G3
→ fresh G3 adversarial review
→ fresh G3→G4 human Gate
```

在此之前 G3/G4 全部保持 BLOCKED。

### Schema28 — LB-006 再次回开 + 测试 Gate 分层

Schema27 implementation acceptance `e717a0f8d69a93c17faa426694e009ad03a80d9e` 保留为历史 provenance，但 public plugin 实测继续发现 LB-006 facade/runtime 行为缺陷，因此该 acceptance 不能继续解锁 LB-007。Schema28 当前指针重新固定：

```text
current_group = G2
current_pr    = LB-006
LB-006        = REWORK_REQUIRED
LB-007..012   = BLOCKED
G2 review     = generation 9 FAIL
generation 10 = NOT CONSUMED
```

LB-006 必须关闭：shell quoting extra-reparse、incremental poll loss/replay、live write SessionUnavailable、kill RuntimeUnavailable→SessionUnavailable、真实 image resize、PowerShell UTF-8 中文输出、Git blame inclusive range、document invalid range，以及所有 adapter-consumed/unmodeled private result semantics 的 serving 前 fail-closed probe。

执行/验收采用：

```text
PR Fast Gate    → 当前 PR targeted unit/fake/static
PR Runtime Gate → 按共享 runtime/PEP/process topology 压缩的真实行为 lifecycle
Formal accept   → 当前 PR 完整 required Gate
Group/Release   → 全组/发布完整 regression
```

重型 fixture 不得为独立 assertion 无理由重复启动；cheap unit 保持独立。source-string marker 不得替代行为测试。每个 runner 有 bounded timeout/cancel，lost session 必须终态结束。开发/测试 console window 不影响本 PR 判定；最终 packaged GUI managed-child no-visible-console 由 LB-018/LB-019 release-style/clean-machine Gate 证明。

### Schema29 — task-state durability 回开 G2 / generation 17 FAIL

Schema29 合同基线 `003e85ed5da5944fba8bc89ba7051e55f23333a1` 加入 A289–A292 后，对当前已提交 G2 package 执行 independent adversarial generation 17。审查不是因为“新增合同自然失败”，而是发现了真实现存状态不一致：最近 100 个 terminal task-history 中有 52 个出现 `command_started` 无匹配 `command_finished`，或 task 已 terminal 但仍保存 running `current_command`；当前 LocalBridge `task_control`/public session terminal truth 仍是进程内投影/HashMap，没有 schema29 所要求的 durable task-state atomic finalizer 与 `(task_id, session_id)` owner CAS。

```text
G2 generation 17 = FAIL / REWORK_REQUIRED / CONSUMED
current_group     = G2
current_pr        = LB-006
LB-006            = REWORK_REQUIRED
LB-007..012       = BLOCKED pending strict reacceptance
G3 / LB-013..017  = BLOCKED
next G2 review    = generation 18, only after LB-006..012 are PASS again
```

LB-006 只负责 A289–A291：terminal finally 原子提交、task-state durable terminal snapshot、`task_id+session_id` CAS/owner isolation。A292“首次前台主窗口默认居中”属于 LB-015，当前只排队，**不是 generation17 的 G2 blocker，也不得提前修改 G3 产品代码**。

### Schema26 管理员模式安全确认修订

Schema26 是对后续 **LB-015 / LB-016** 的 UI/安全合同追加，不改变当前执行入口，也不允许跳过 G2：

```text
current_group = G2
current_pr    = LB-006
```

到达 LB-015/LB-016 时必须实现并验收：

- 所有 Settings / onboarding Screen3 的 `管理员模式` 入口统一橙色 `#ff9500`；
- Broker 未 Active 时，选择/重选管理员模式先打开固定安全警告，而不是直接 UAC；
- 固定警告正文严格使用 schema26 八条后果和 footer；
- 确认按钮整个为红色，从 `确认9` 倒计时到 `确认1`，完整 9000ms 内 disabled，之后仍为红色并显示 enabled `确认`；
- frontend 仅显示倒计时，backend/等价可信单调计时负责 not-before/eligibility，提前/重放/stale confirmation fail-closed；
- only enabled `确认` 后才允许既有安全校验与 Windows UAC；取消/Esc/关闭无副作用，fresh open 重新计时，无 remember/skip；
- background restore 无 warning/UAC；Broker Active 重新选择不得重复 UAC；
- High/Critical 单操作确认仍是独立 Gate；AI/MCP 不能批准模式警告/UAC或改变 control-plane。

Schema26 合同变更本身不构成 LB-015/LB-016 实现 PASS；必须等严格顺序实际到达对应 PR 后修改产品代码与测试。

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
- 5 次失败后一次极简错误窗口；
- schema26 管理员模式橙色入口 + whole-red 9 秒安全确认门。

LB-016：

- 5 屏 onboarding 合同保持；
- Screen3 管理员模式使用与 Settings 相同 schema26 安全确认；
- safety-consent dialog 仅为局部安全层，不得使 onboarding 重新变成居中 modal/card shell。

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

若后续人工实测细审核 FAIL，不得抹掉已经成立的独立对抗审查 PASS provenance；人工 Gate 与独立组审是两个不同维度。状态迁移固定为：

```text
G3.status = REWORK_REQUIRED
G3.review_status = PASS
G3.human_review_status = FAIL
G3.reopen_from_pr = 最早受影响的 G3 PR
reopen_from_pr = REWORK_REQUIRED
其后同组 PR = BLOCKED
current_group = G3
current_pr = reopen_from_pr
G4 = BLOCKED
LB-018 = BLOCKED
```

从 `reopen_from_pr` 起重新按 LB 编号顺序执行；完成返工后必须重新通过所需 G3 审查与人工 Gate，不能仅凭旧的人工截图、旧测试 PASS 或旧 review generation 解锁 G4。

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
