
# LocalBridge 单 PR 执行

你是唯一开发执行智能体。

项目：`<PROJECT_ROOT>`
当前组：`<GROUP>`
当前 PR：`<LB-ID> — <TITLE>`

先读取 `START_HERE.md` 指定的全部事实源。

确认：

```text
current_group == <GROUP>
current_pr    == <LB-ID>
```

若存在前一组，必须：

```text
group.status = PASS
group.review_status = PASS
```

执行：

1. 读当前 PR contract。
2. 检查 writable/forbidden/non-goals。
3. 当前 PR → IN_PROGRESS。
4. 实现当前 PR 最小完整长期方案。
5. 跑 required tests + 回归/架构检查。
6. 审查 diff。
7. PASS 后：
   - 组内还有 PR：只解锁下一编号 PR；
   - 组末：group → REVIEW_REQUIRED，current_pr=null，立即停止。
8. 不自动开始下一 PR/组。

硬规则以 `AGENTS.md` 为准；合同冲突立即停止。

完成报告：修改文件、合同实现、测试、风险/阻塞、下一状态。
