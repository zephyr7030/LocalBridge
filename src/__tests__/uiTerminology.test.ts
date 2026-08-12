import { describe, expect, it } from "vitest";
import { uiText } from "../presentation";

describe("LB-015 UI terminology", () => {
  it("centralizes the shell terminology in Chinese", () => {
    expect(uiText).toEqual({ dashboard: "主控界面", settings: "设置", diagnostics: "诊断" });
  });
});
