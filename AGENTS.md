# LocalBridge Agent Rules

1. **事实源**：先读 `START_HERE.md`；实时状态只认 `PR_INDEX.json + PROJECT_STATE.json`；实现只认当前磁盘代码、当前合同和真实测试。发生冲突立即停止并报告。
2. **执行边界**：单 Agent 串行，只做 `current_pr`；只写该 PR `writable_paths`/明确例外；只 stage 自己的文件，不清理未知 untracked；产品与治理分 commit；PR 验收后推进状态并立即停止，不自动做下一 PR。
3. **Ponytail（内置常驻，默认 full）**：所有 coding / bugfix / refactor / review / design / dependency-selection 任务始终启用，无需再次读取 skill。先理解真实调用链和根因，再按 `不需要就不做 → 复用现有 → stdlib → 原生平台能力 → 已装依赖 → 能一行就一行 → 最小完整实现` 停在第一个可行层；优先删除、少文件、少依赖、无投机抽象/脚手架，Bug 修共享根因而非单一路径症状。不得简化显式需求、安全/信任边界、防数据丢失、可访问性、必要测试和长期可维护性。仅用户明确说 `stop ponytail` / `normal mode` 才停用，可切换 `lite/full/ultra`。完整参考：`skills/ponytail/SKILL.md`。
4. **安全/架构硬边界**：Public API 与 policy fail-closed；workspace 权限不得越过 active root，control-plane 永久 deny；secret 不得明文泄漏；管理员能力只走既定 Broker/UAC 边界；Rust/backend 持有 lifecycle/readiness/retry/权限/CurrentTask truth，前端只做 typed projection + typed intent。
5. **按需读取与验收**：任务需要时再读 `PR_CONTRACTS.json`、`START_HERE.md` 指向的 authority docs、`ARCHITECTURE_RULES.json`、runtime manifest/policy、相关 `skills/**`、测试与 Git 历史。旧报告/旧 PASS/聊天只作线索；静态 marker 不能替代真实行为；先 targeted tests，再按合同运行 PR/Group Gate；历史 FAIL/provenance 不改写，修复后追加 acceptance。
