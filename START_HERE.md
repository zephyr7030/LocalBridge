
# LocalBridge v15 FINAL — Start Here

> 本文件是开发前 **冻结启动快照（historical bootstrap snapshot）**，用于说明读取顺序与冻结约束，不承载推进中的实时 PR/Gate 状态。
> 实时 `current_group`、`current_pr`、group review 状态只读取 `PR_INDEX.json` 与 `PROJECT_STATE.json`；若二者互相冲突，立即停止并报告。

```text
bootstrap_phase        = PRE_CODE
bootstrap_group        = G0
bootstrap_pr           = LB-000
predevelopment_review  = PASS
```

读取顺序：

1. `AGENTS.md`
2. `docs/01_PRODUCT_UX.md`
3. `docs/02_ARCHITECTURE_RUNTIME.md`
4. `docs/03_SECURITY.md`
5. `docs/04_UPSTREAM_DISTRIBUTION.md`
6. `docs/05_MAINTENANCE_RELEASE.md`
7. `docs/06_PR_GROUPS_AND_EXECUTION.md`
8. `docs/07_ACCEPTANCE.md`
9. `docs/08_FINAL_REVIEW.md`
10. `skills/**`
11. `PR_INDEX.json`
12. `PR_CONTRACTS.json`
13. `PROJECT_STATE.json`
14. `ARCHITECTURE_RULES.json`
15. `COMPATIBILITY_BASELINE.json`
16. `FINAL_REVIEW.json`
17. `runtime-manifest.toml`
18. `runtime-policy.toml`

机器合同与文档冲突：立即停止并报告。冻结/历史快照中的旧状态字段不属于实时状态冲突。

历史启动任务（冻结快照）：

```text
G0
LB-000 — Upstream + Security Compatibility Spike
```

LB-000 PASS 后不能直接执行 LB-001：

```text
G0 PR PASS
→ G0 独立对抗审查 PASS
→ 才解锁 G1 / LB-001
```

冻结：

```text
自动重连       = 5 次；1/2/5/10/30s
重连 UI        = 5 次内不新增；全失败后一次错误窗口
UI             = Apple-inspired 极简
视觉新增依赖   = 禁止
Runtime API Key= secure store；无明文/CLI
授权项目       = 同时最多一个
项目移除       = 永不删除磁盘文件
PR             = 20
Groups         = 5
组间审查       = mandatory independent adversarial PASS
```

UI 冻结补充：

```text
首次启动       = 6 屏
顺序           = 欢迎 → OpenAI → 项目与权限 → Local Bridge 设置 → Local Bridge 使用确认 → 启动检查
第 7 屏        = 禁止
第 6 屏确认    = 三项全绿后才启用
自动进入主界面 = 禁止
Local Bridge设置= 系统浏览器固定 deep link；禁止 WebView/任意 URL
项目选择       = 原生 Windows 文件夹选择器为主交互
按钮           = 统一且可辨识；禁止白底白按钮
窗口           = 默认 900×620；最小 720×500；可缩放；向导随 viewport 高度响应且不依赖固定卡片高度
WebView同步     = resize/maximize 后始终铺满 native client area；Dashboard/onboarding 随 live viewport 重排
resize验收      = 必须真实运行 Windows Tauri/WebView2：至少两个 native size + maximize；静态 CSS/Tauri 配置断言不能单独 PASS
```

```text
品牌图标       = assets/icons/localbridge.ico
品牌 PNG       = assets/icons/localbridge.png
替换/重绘      = 未经明确合同禁止
```
