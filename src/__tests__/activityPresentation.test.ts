import { describe, expect, it } from "vitest";
import { activityAction, activityErrorReason, activityOutcome, activityRiskText, activityTone } from "../features/activity/presentation";
import type { ActivityEntry } from "../features/activity/api";

const entry = (overrides: Partial<ActivityEntry> = {}): ActivityEntry => ({
  source: "tool",
  timestampMs: 1,
  action: "document_workflow",
  operation: "edit",
  target: "README.md",
  outcome: "success",
  errorCode: null,
  requestId: "req-1",
  connectionId: "conn-1",
  attempt: 1,
  phase: null,
  cause: null,
  httpStatus: null,
  workdir: null,
  durationMs: null,
  exitCode: null,
  risk: [],
  ...overrides,
});

describe("activity presentation", () => {
  it("keeps the second-level operation inside the existing action text", () => {
    expect(activityAction(entry())).toBe("文档 写入");
    expect(activityAction(entry({ operation: "inspect" }))).toBe("文档 读取");
    expect(activityAction(entry({ action: "filesystem", operation: "search_content" }))).toBe("文件 内容搜索");
    expect(activityAction(entry({ action: "git_workflow", operation: "diff" }))).toBe("Git 差异");
  });

  it("does not leak unknown backend tool or operation names", () => {
    expect(activityAction(entry({ action: "future_tool", operation: "future_action" }))).toBe("其他操作");
    expect(activityAction(entry({ operation: "future_action" }))).toBe("文档");
    expect(activityOutcome(entry({ outcome: "future_outcome" }))).toBe("状态未知");
    expect(activityRiskText("future_risk")).toBe("需注意");
  });

  it("maps result semantics to green, yellow, and red tones", () => {
    expect(activityOutcome(entry({ outcome: "success" }))).toBe("完成");
    expect(activityTone(entry({ outcome: "completed" }))).toBe("positive");
    expect(activityTone(entry({ outcome: "blocked" }))).toBe("neutral");
    expect(activityTone(entry({ outcome: "cancelled" }))).toBe("neutral");
    expect(activityTone(entry({ outcome: "failed" }))).toBe("negative");
    expect(activityTone(entry({ outcome: "timed_out" }))).toBe("negative");
  });

  it("uses specific diagnostic causes before generic error codes", () => {
    expect(activityErrorReason(entry({ outcome: "failed", errorCode: "Denied", cause: "workspace_denied" }))).toBe("工作区拒绝");
    expect(activityErrorReason(entry({ outcome: "failed", errorCode: "Timeout" }))).toBe("执行超时");
    expect(activityErrorReason(entry({ source: "administrator", outcome: "failed", requestId: null, connectionId: null, attempt: null, exitCode: 5 }))).toBe("退出码 5");
    expect(activityErrorReason(entry({ outcome: "failed", errorCode: "FutureFailure" }))).toBe("执行失败");
    expect(activityErrorReason(entry({ outcome: "cancelled", errorCode: "Cancelled", exitCode: 1 }))).toBeNull();
  });
});
