import { describe, expect, it } from "vitest";
import { APP_NAME, bootstrapMessage } from "../appModel";

describe("LB-001 frontend baseline", () => {
  it("keeps the frozen product name and Chinese bootstrap copy", () => {
    expect(APP_NAME).toBe("LocalBridge");
    expect(bootstrapMessage()).toBe("本地运行环境已建立");
  });
});
