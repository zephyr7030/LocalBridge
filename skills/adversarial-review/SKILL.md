
# Group Adversarial Review
- 目标是证明当前组可能错误，不是证明开发正确。
- 审查整个组和跨 PR 组合。
- 下一组在 review PASS 前必须 BLOCKED。
- FAIL 给 evidence/reproduction/severity/reopen_from_pr。
- Reviewer 不改代码/合同/文档/测试，只允许组审后的 PR_INDEX/PROJECT_STATE 状态写入。
