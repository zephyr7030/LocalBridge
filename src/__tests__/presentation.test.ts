import { describe, expect, it } from "vitest";
import { accessText, formatLastToolAge, lastToolText, privilegeText, serviceVisualState, taskText } from "../presentation";

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
    expect(taskText(null)).toBe("等待命令");
    const last = { kind: "command" as const, summary: "git status", ageMs: 59_000 };
    expect(lastToolText(last)).toBe("上次执行工具：运行命令  git status");
    expect(formatLastToolAge(59_000)).toBe("59S前");
    expect(formatLastToolAge(59 * 60_000)).toBe("59分钟前");
    expect(formatLastToolAge(60 * 60_000)).toBe("大于1小时");
    expect(formatLastToolAge(3 * 24 * 60 * 60_000)).toBe("大于3天");
  });
});
