
# Group Adversarial Review
- 目标是证明当前组可能错误，不是证明开发正确。
- 审查整个组和跨 PR 组合。
- 下一组在 review PASS 前必须 BLOCKED。
- G3→G4 例外：G3 adversarial PASS 后仍保持 G4/LB-018 BLOCKED，进入独立人工实测细审核 Gate；人工 PASS 后才解锁 G4。
- 用户与执行智能体提供的事实性材料均是待验证证据，Reviewer 可质疑、复核、独立验证或拒绝采信。
- 执行智能体可预授权，但必须逐项记录 authorization_id/scope/actions/evidence_ref/recorded_by/user_audit_status；人工 PASS 要求所有记录均经用户审核 PASS。
- FAIL 给 evidence/reproduction/severity/reopen_from_pr。
- Reviewer 不改代码/合同/文档/测试，只允许组审后的 PR_INDEX/PROJECT_STATE 状态写入。
