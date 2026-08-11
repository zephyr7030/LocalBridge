
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

若当前组为 G4，还必须额外确认：

```text
G3.human_review_status = PASS
```

执行智能体无权自行把 G3 人工 Gate 标为 PASS。若依赖预授权执行人工测试相关操作，必须逐项记录具体授权并等待用户审核；不能用笼统“已预授权”代替记录。

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
