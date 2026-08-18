import { describe, expect, it } from "vitest";
import { accessText, currentActivityText, formatLastToolAge, lastCommandText, lastToolText, privilegeText, serviceVisualState, taskText } from "../presentation";

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
    const last = { kind: "command" as const, summary: "git status", ageMs: 59_000 };
    expect(lastToolText(last)).toBe("上次执行工具：运行命令  git status");
    expect(formatLastToolAge(59_000)).toBe("59S前");
    expect(formatLastToolAge(59 * 60_000)).toBe("59分钟前");
    expect(formatLastToolAge(60 * 60_000)).toBe("大于1小时");
    expect(formatLastToolAge(3 * 24 * 60 * 60_000)).toBe("大于3天");
  });
  it("keeps schema42 current state separate from command history", () => {
    expect(currentActivityText(null, null)).toBe("空闲");
    expect(currentActivityText({ state: "waiting" }, null)).toBe("任务等待继续");
    expect(currentActivityText({ state: "running" }, null)).toBe("任务执行中…");
    expect(currentActivityText(null, { state: "running" })).toBe("运行命令…");
    expect(currentActivityText({ state: "waiting" }, { state: "waiting_input" })).toBe("等待输入…");
    expect(currentActivityText({ state: "waiting" }, { state: "cancelling" })).toBe("正在取消…");
    expect(lastCommandText({ status: "completed", ageMs: 1 })).toContain("结果：成功");
    expect(lastCommandText({ status: "cancelled", ageMs: 1 })).toContain("结果：已取消");
    expect(currentActivityText(null, null)).toBe("空闲");
  });

});
