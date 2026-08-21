import { describe, expect, it } from "vitest";
import { accessText, currentActivityDetail, currentActivityText, formatLastToolAge, lastActivityAction, lastActivityOutcome, privilegeText, serviceVisualState, taskText, updateStatusText } from "../presentation";

describe("LB-015 presentation", () => {
  it("maps frozen Chinese wording", () => {
    expect(accessText).toEqual({ edit: "编辑模式", full: "完整模式", admin: "管理员模式" });
    expect(privilegeText.active).toBe("已启用");
    expect(privilegeText.requested).toBe("等待授权");
    expect(privilegeText.awaiting).toBe("等待系统授权");
  });
  it("maps typed service states to one shared visual semantic", () => {
    expect(serviceVisualState).toEqual({ off: "unknown", starting: "starting", online: "ready", recovering: "starting", fault: "fault" });
  });
  it("keeps the current row age-free and renders one separate last-tool age", () => {
    const line = taskText({ kind: "test", summary: "cargo test", state: "running", elapsedMs: 59_000 });
    expect(line).toContain("运行测试");
    expect(line).toContain("cargo test");
    expect(line).toContain("59S");
    expect(line).not.toContain("前");
    expect(taskText(null)).toBe("空闲");
    expect(taskText({ kind: "admin", summary: "安装设备驱动", state: "waiting", elapsedMs: null })).toBe("管理员操作  安装设备驱动  等待授权");
    expect(taskText({ kind: "admin", summary: "安装设备驱动", state: "blocked", elapsedMs: null })).toBe("管理员操作  安装设备驱动  已阻止");
    expect(taskText({ kind: "test", summary: "cargo test", state: "failed", elapsedMs: null })).toBe("运行测试  cargo test  执行失败");
    expect(taskText({ kind: "command", summary: "cargo build", state: "cancelled", elapsedMs: null })).toBe("运行命令  cargo build  已取消");
    expect(formatLastToolAge(59_000)).toBe("59S前");
    expect(formatLastToolAge(59 * 60_000)).toBe("59分钟前");
    expect(formatLastToolAge(60 * 60_000)).toBe("大于1小时");
    expect(formatLastToolAge(3 * 24 * 60 * 60_000)).toBe("大于3天");
  });
  it("keeps schema42 current state separate from command history", () => {
    const base = { kind: "other" as const, summary: null, elapsedMs: null, step: null, progressCurrent: null, progressTotal: null };
    expect(currentActivityText(null)).toBe("空闲");
    expect(currentActivityText({ ...base, state: "waiting" })).toBe("任务等待继续");
    expect(currentActivityText({ ...base, state: "running" })).toBe("任务执行中…");
    expect(currentActivityText({ ...base, kind: "command", state: "running" })).toBe("运行命令…");
    expect(currentActivityText({ ...base, kind: "command", state: "waiting_input" })).toBe("等待输入…");
    expect(currentActivityText({ ...base, kind: "command", state: "cancelling" })).toBe("正在取消…");
    expect(currentActivityDetail({ ...base, state: "running", step: "verify", progressCurrent: 2, progressTotal: 4 })).toBe("验证 2/4");
    const last = { kind: "git" as const, summary: "status", outcome: "completed" as const, completedAtMs: 1 };
    expect(lastActivityAction(last)).toBe("Git 操作");
    expect(lastActivityOutcome(last)).toBe("成功");
  });
  it("renders typed update lifecycle without guessing availability", () => {
    const base = { currentVersion: "1.0.0", latestVersion: null, releaseUrl: "https://github.com/owner/repo/releases", operationId: null, attempt: null, retryable: true };
    expect(updateStatusText({ ...base, state: "checking" })).toContain("正在检查更新");
    expect(updateStatusText({ ...base, state: "current" })).toContain("已是最新版本");
    expect(updateStatusText({ ...base, state: "available", latestVersion: "1.1.0" })).toBe("发现新版本 1.1.0");
    expect(updateStatusText({ ...base, state: "source_unavailable", releaseUrl: null, retryable: false })).toContain("发布源不可用");
  });

});
